use std::{env, fmt, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{header::ACCEPT, redirect::Policy, Client, Method, RequestBuilder, Response};
use tokio::sync::mpsc::UnboundedSender;

use super::{
    payload::{validate_base_url, validate_opaque_token, BuildKind, BuildPayload},
    response::{
        http_error, parse_build_response, parse_runner_event, BuildLogEvent, BuildResponse,
        RunnerEvent,
    },
};

const ENV_BUILDRUNNER_URL: &str = "HAKOBI_BUILDRUNNER_URL";
const ENV_BUILDRUNNER_TOKEN: &str = "HAKOBI_BUILDRUNNER_TOKEN";
const ENV_BUILDRUNNER_TIMEOUT: &str = "HAKOBI_BUILDRUNNER_TIMEOUT_SECS";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(15);
/// Default build budget; override with `HAKOBI_BUILDRUNNER_TIMEOUT_SECS`.
const DEFAULT_BUILD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MIN_BUILD_TIMEOUT_SECS: u64 = 1;
const MAX_BUILD_TIMEOUT_SECS: u64 = 6 * 60 * 60;

#[derive(Clone)]
pub struct BuildRunnerClient {
    client: Client,
    pub(super) base_url: String,
    token: Option<String>,
    build_timeout: Duration,
}

impl fmt::Debug for BuildRunnerClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuildRunnerClient")
            .field("base_url", &self.base_url)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[derive(Clone)]
pub struct BuildRequest {
    pub git_url: String,
    pub branch: String,
    pub image_tag: String,
    pub dockerfile_path: Option<String>,
    pub context_dir: Option<String>,
    pub token: Option<String>,
}

impl fmt::Debug for BuildRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BuildRequest")
            .field("git_url", &self.git_url)
            .field("branch", &self.branch)
            .field("image_tag", &self.image_tag)
            .field("dockerfile_path", &self.dockerfile_path)
            .field("context_dir", &self.context_dir)
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

impl BuildRunnerClient {
    /// Read the runner location from `HAKOBI_BUILDRUNNER_URL` and
    /// `HAKOBI_BUILDRUNNER_TOKEN`.
    ///
    /// Returns `Ok(None)` when the URL is unset or blank, which means the
    /// optional runner integration is not configured.
    pub fn from_env() -> Result<Option<Self>> {
        let url = read_env(ENV_BUILDRUNNER_URL)?;
        match url.as_deref() {
            Some(value) if !value.trim().is_empty() => {}
            _ => return Ok(None),
        }
        let token = read_env(ENV_BUILDRUNNER_TOKEN)?;
        let timeout = read_env(ENV_BUILDRUNNER_TIMEOUT)?;
        let parsed_timeout = match timeout.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(raw) => {
                let parsed: u64 = raw.parse().map_err(|_| {
                    anyhow!("{ENV_BUILDRUNNER_TIMEOUT} must be a whole number of seconds")
                })?;
                if !(MIN_BUILD_TIMEOUT_SECS..=MAX_BUILD_TIMEOUT_SECS).contains(&parsed) {
                    bail!("{ENV_BUILDRUNNER_TIMEOUT} must be between {MIN_BUILD_TIMEOUT_SECS} and {MAX_BUILD_TIMEOUT_SECS}");
                }
                Some(Duration::from_secs(parsed))
            }
            None => None,
        };
        let mut client = Self::from_optional(url.as_deref(), token.as_deref())?;
        if let (Some(client), Some(timeout)) = (client.as_mut(), parsed_timeout) {
            client.build_timeout = timeout;
        }
        Ok(client)
    }

    pub fn new(base_url: &str, token: Option<&str>) -> Result<Self> {
        let base_url = validate_base_url(base_url)?;
        let token = match token {
            Some(token) => {
                validate_opaque_token("build runner token", token)?;
                Some(token.to_owned())
            }
            None => None,
        };
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            // A build API has no business redirecting; refusing redirects also
            // keeps the bearer token from ever being replayed to another host.
            .redirect(Policy::none())
            .user_agent(concat!("hakobi/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("could not create the build runner HTTP client")?;
        Ok(Self {
            client,
            base_url,
            token,
            build_timeout: DEFAULT_BUILD_TIMEOUT,
        })
    }

    pub async fn health(&self) -> Result<()> {
        let response = self
            .request(Method::GET, "/healthz")
            .timeout(HEALTH_TIMEOUT)
            .send()
            .await
            .context("build runner health check failed")?;
        if !response.status().is_success() {
            return Err(http_error(response).await);
        }
        Ok(())
    }

    pub async fn build_compose(&self, request: &BuildRequest) -> Result<BuildResponse> {
        self.build(BuildKind::Compose, request).await
    }

    pub async fn build_dockerfile(&self, request: &BuildRequest) -> Result<BuildResponse> {
        self.build(BuildKind::Dockerfile, request).await
    }

    pub async fn build_nixpacks(&self, request: &BuildRequest) -> Result<BuildResponse> {
        self.build(BuildKind::Nixpacks, request).await
    }

    pub async fn build_compose_stream(
        &self,
        request: &BuildRequest,
        logs: UnboundedSender<BuildLogEvent>,
    ) -> Result<BuildResponse> {
        self.build_stream(BuildKind::Compose, request, logs).await
    }

    pub async fn build_dockerfile_stream(
        &self,
        request: &BuildRequest,
        logs: UnboundedSender<BuildLogEvent>,
    ) -> Result<BuildResponse> {
        self.build_stream(BuildKind::Dockerfile, request, logs)
            .await
    }

    pub async fn build_nixpacks_stream(
        &self,
        request: &BuildRequest,
        logs: UnboundedSender<BuildLogEvent>,
    ) -> Result<BuildResponse> {
        self.build_stream(BuildKind::Nixpacks, request, logs).await
    }

    pub(super) fn from_optional(url: Option<&str>, token: Option<&str>) -> Result<Option<Self>> {
        let Some(url) = url.map(str::trim).filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let token = token.map(str::trim).filter(|value| !value.is_empty());
        Self::new(url, token).map(Some)
    }

    async fn build(&self, kind: BuildKind, request: &BuildRequest) -> Result<BuildResponse> {
        let payload = BuildPayload::for_request(kind, request)?;
        let response = self
            .request(Method::POST, kind.endpoint())
            .timeout(self.build_timeout)
            .json(&payload)
            .send()
            .await
            .context("build runner request failed")?;
        if !response.status().is_success() {
            return Err(http_error(response).await);
        }
        let body = response
            .bytes()
            .await
            .context("could not read the build runner response")?;
        parse_build_response(&body)
    }

    async fn build_stream(
        &self,
        kind: BuildKind,
        request: &BuildRequest,
        logs: UnboundedSender<BuildLogEvent>,
    ) -> Result<BuildResponse> {
        let payload = BuildPayload::for_request(kind, request)?;
        let response = self
            .request(Method::POST, kind.endpoint())
            .timeout(self.build_timeout)
            .header(ACCEPT, "application/x-ndjson")
            .json(&payload)
            .send()
            .await
            .context("build runner request failed")?;
        if !response.status().is_success() {
            return Err(http_error(response).await);
        }
        if is_ndjson(&response) {
            read_ndjson(response, logs).await
        } else {
            read_json_fallback(response, logs).await
        }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let builder = self.client.request(method, self.endpoint(path));
        match self.token.as_deref() {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    pub(super) fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

fn is_ndjson(response: &Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/x-ndjson"))
}

async fn read_json_fallback(
    response: Response,
    logs: UnboundedSender<BuildLogEvent>,
) -> Result<BuildResponse> {
    let body = response
        .bytes()
        .await
        .context("could not read the build runner response")?;
    let outcome = parse_build_response(&body)?;
    if !outcome.log_tail.is_empty() {
        let _ = logs.send(BuildLogEvent {
            stage: "runner".to_owned(),
            message: outcome.log_tail.clone(),
        });
    }
    Ok(outcome)
}

async fn read_ndjson(
    mut response: Response,
    logs: UnboundedSender<BuildLogEvent>,
) -> Result<BuildResponse> {
    let mut buffered = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("could not read the build runner event stream")?
    {
        buffered.extend_from_slice(&chunk);
        let mut start = 0;
        while let Some(relative) = buffered[start..].iter().position(|byte| *byte == b'\n') {
            let end = start + relative;
            if let Some(outcome) = handle_event_line(&buffered[start..end], &logs)? {
                return Ok(outcome);
            }
            start = end + 1;
        }
        if start > 0 {
            buffered.drain(..start);
        }
    }
    if !buffered.is_empty() {
        if let Some(outcome) = handle_event_line(&buffered, &logs)? {
            return Ok(outcome);
        }
    }
    bail!("build runner event stream ended without a complete event")
}

fn handle_event_line(
    line: &[u8],
    logs: &UnboundedSender<BuildLogEvent>,
) -> Result<Option<BuildResponse>> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    match parse_runner_event(line)? {
        RunnerEvent::Log { stage, message } => {
            let _ = logs.send(BuildLogEvent { stage, message });
            Ok(None)
        }
        RunnerEvent::Complete { outcome } => Ok(Some(outcome)),
    }
}

fn read_env(name: &str) -> Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => bail!("{name} must be valid UTF-8"),
    }
}
