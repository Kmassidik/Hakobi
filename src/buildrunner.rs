//! HTTP client for the standalone Hakobi build runner.
//!
//! The runner is an optional service that clones a repository and builds an
//! image with `docker build` or `nixpacks`. This module only speaks HTTP: it
//! never starts, stops, or configures the runner, and it never sends the
//! bearer token as part of a URL.

use std::{env, fmt, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use reqwest::{redirect::Policy, Client, Method, RequestBuilder, Response, StatusCode};
use serde::{Deserialize, Deserializer, Serialize};
use url::Url;

const ENV_BUILDRUNNER_URL: &str = "HAKOBI_BUILDRUNNER_URL";
const ENV_BUILDRUNNER_TOKEN: &str = "HAKOBI_BUILDRUNNER_TOKEN";
const ENV_BUILDRUNNER_TIMEOUT: &str = "HAKOBI_BUILDRUNNER_TIMEOUT_SECS";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(15);
/// Default build budget; override with `HAKOBI_BUILDRUNNER_TIMEOUT_SECS`.
const DEFAULT_BUILD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MIN_BUILD_TIMEOUT_SECS: u64 = 1;
const MAX_BUILD_TIMEOUT_SECS: u64 = 6 * 60 * 60;

const MAX_ERROR_BODY: usize = 8 * 1024;
const ERROR_EXCERPT_BYTES: usize = 512;

const MAX_GIT_URL_LEN: usize = 2_048;
const MAX_BRANCH_LEN: usize = 255;
const MAX_IMAGE_TAG_LEN: usize = 255;
const MAX_PATH_LEN: usize = 1_024;
const MAX_TOKEN_LEN: usize = 8_192;

/// The runner reports a null `exit_code` when the process never exited
/// (typically a timeout). The public contract uses `i32`, so map that to a
/// sentinel that cannot be a POSIX exit status.
const NO_EXIT_CODE: i32 = -1;

#[derive(Clone)]
pub struct BuildRunnerClient {
    client: Client,
    base_url: String,
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

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BuildResponse {
    pub status: String,
    #[serde(
        default = "missing_exit_code",
        deserialize_with = "deserialize_exit_code"
    )]
    pub exit_code: i32,
    pub duration_ms: u64,
    pub log_tail: String,
    pub truncated: bool,
    #[serde(default)]
    pub detail: Option<String>,
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

    fn from_optional(url: Option<&str>, token: Option<&str>) -> Result<Option<Self>> {
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

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let builder = self.client.request(method, self.endpoint(path));
        match self.token.as_deref() {
            Some(token) => builder.bearer_auth(token),
            None => builder,
        }
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuildKind {
    Compose,
    Dockerfile,
    Nixpacks,
}

impl BuildKind {
    fn endpoint(self) -> &'static str {
        match self {
            Self::Compose => "/build/compose",
            Self::Dockerfile => "/build/dockerfile",
            Self::Nixpacks => "/build/nixpacks",
        }
    }
}

#[derive(Serialize)]
struct BuildPayload<'a> {
    git_url: &'a str,
    branch: &'a str,
    image_tag: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dockerfile_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_dir: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<&'a str>,
}

impl<'a> BuildPayload<'a> {
    fn for_request(kind: BuildKind, request: &'a BuildRequest) -> Result<Self> {
        validate_request_common(request)?;
        let (dockerfile_path, context_dir) = match kind {
            BuildKind::Compose | BuildKind::Dockerfile => {
                if kind == BuildKind::Compose && request.dockerfile_path.is_some() {
                    bail!("compose builds do not accept dockerfile_path");
                }
                if let Some(path) = request.dockerfile_path.as_deref() {
                    validate_repo_path("dockerfile_path", path, false)?;
                }
                if let Some(directory) = request.context_dir.as_deref() {
                    validate_repo_path("context_dir", directory, true)?;
                }
                (
                    request.dockerfile_path.as_deref(),
                    request.context_dir.as_deref(),
                )
            }
            BuildKind::Nixpacks => {
                if request.dockerfile_path.is_some() || request.context_dir.is_some() {
                    bail!("nixpacks builds do not accept dockerfile_path or context_dir");
                }
                (None, None)
            }
        };
        Ok(Self {
            git_url: &request.git_url,
            branch: &request.branch,
            image_tag: &request.image_tag,
            dockerfile_path,
            context_dir,
            token: request.token.as_deref(),
        })
    }
}

fn read_env(name: &str) -> Result<Option<String>> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => bail!("{name} must be valid UTF-8"),
    }
}

fn validate_base_url(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("build runner URL must not be empty");
    }
    if value.chars().any(char::is_control) {
        bail!("build runner URL must not contain control characters");
    }
    let url = Url::parse(value).context("build runner URL is not a valid URL")?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => bail!("build runner URL must use http or https, not {scheme}"),
    }
    if url.host_str().is_none() {
        bail!("build runner URL must include a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("build runner URL must not contain credentials");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("build runner URL must not contain a query or fragment");
    }
    Ok(value.trim_end_matches('/').to_owned())
}

fn validate_opaque_token(field: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{field} must not be empty when provided");
    }
    if value.len() > MAX_TOKEN_LEN {
        bail!("{field} must be at most {MAX_TOKEN_LEN} bytes");
    }
    if !value.is_ascii() {
        bail!("{field} must be ASCII");
    }
    if value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        bail!("{field} must not contain whitespace or control characters");
    }
    Ok(())
}

fn validate_request_common(request: &BuildRequest) -> Result<()> {
    validate_git_url(&request.git_url)?;
    validate_branch(&request.branch)?;
    validate_image_tag(&request.image_tag)?;
    if let Some(token) = request.token.as_deref() {
        validate_opaque_token("token", token)?;
        if !is_https_url(&request.git_url) {
            bail!("token is only supported for https git URLs");
        }
    }
    Ok(())
}

fn validate_field(field: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() {
        bail!("{field} must not be empty");
    }
    if value.len() > max {
        bail!("{field} must be at most {max} bytes");
    }
    if value.chars().any(char::is_control) {
        bail!("{field} must not contain control characters");
    }
    Ok(())
}

fn validate_git_url(value: &str) -> Result<()> {
    validate_field("git_url", value, MAX_GIT_URL_LEN)?;
    if value.chars().any(char::is_whitespace) {
        bail!("git_url must not contain whitespace");
    }
    if value.starts_with('-') {
        bail!("git_url must not start with '-'");
    }
    // Check the raw value: `Url::parse` silently collapses `..` segments.
    if value.split('/').any(|segment| segment == "..") {
        bail!("git_url must not contain '..' path segments");
    }

    if let Some((scheme, _)) = value.split_once("://") {
        if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("ssh") {
            bail!("git_url must use https:// or ssh://");
        }
        let url = Url::parse(value).context("git_url is not a valid URL")?;
        if url.host_str().is_none() {
            bail!("git_url must include a host");
        }
        if url.password().is_some() {
            bail!("git_url must not embed a password or token");
        }
        if url.query().is_some() || url.fragment().is_some() {
            bail!("git_url must not contain a query or fragment");
        }
        if url.username().starts_with('-') {
            bail!("git_url has an invalid user name");
        }
        if url.path().is_empty() || url.path() == "/" {
            bail!("git_url must include a repository path");
        }
        return Ok(());
    }

    // `user@host:path` (scp-like) is the other transport the runner accepts.
    let remote_error = "git_url must be an https, ssh://, or user@host:path remote";
    let (authority, path) = value.split_once(':').context(remote_error)?;
    let (user, host) = authority.split_once('@').context(remote_error)?;
    if user.is_empty() || host.is_empty() || path.is_empty() {
        bail!(remote_error);
    }
    if !user
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        bail!("git_url has an invalid user name");
    }
    if !host
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
        || host.starts_with('-')
        || host.starts_with('.')
        || host.ends_with('.')
        || host.ends_with('-')
    {
        bail!("git_url has an invalid host");
    }
    if path.split('/').any(|segment| segment == "..") {
        bail!("git_url must not contain '..' path segments");
    }
    if path.contains('?') || path.contains('#') {
        bail!("git_url must not contain a query or fragment");
    }
    Ok(())
}

fn validate_branch(value: &str) -> Result<()> {
    validate_field("branch", value, MAX_BRANCH_LEN)?;
    if value.chars().any(char::is_whitespace) {
        bail!("branch must not contain whitespace");
    }
    if value.starts_with('-') {
        bail!("branch must not start with '-'");
    }
    if value == "@" {
        bail!("branch must not be '@'");
    }
    if value.starts_with('/') || value.ends_with('/') || value.contains("//") {
        bail!("branch must not start or end with '/', or contain '//'");
    }
    if value.starts_with('.') || value.ends_with('.') {
        bail!("branch must not start or end with '.'");
    }
    if value.contains("..") {
        bail!("branch must not contain '..'");
    }
    if value.contains("@{") {
        bail!("branch must not contain '@{{'");
    }
    if value
        .chars()
        .any(|character| matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
    {
        bail!("branch contains a character Git refs cannot use");
    }
    for component in value.split('/') {
        if component.is_empty() {
            bail!("branch contains an empty path component");
        }
        if component.starts_with('.') {
            bail!("branch has a path component starting with '.'");
        }
        if component.ends_with(".lock") {
            bail!("branch has a path component ending with '.lock'");
        }
    }
    Ok(())
}

fn validate_image_tag(value: &str) -> Result<()> {
    validate_field("image_tag", value, MAX_IMAGE_TAG_LEN)?;
    if value.chars().any(char::is_whitespace) {
        bail!("image_tag must not contain whitespace");
    }
    if value.contains('@') {
        bail!("image_tag must not contain a digest");
    }
    if value.starts_with('/') || value.ends_with('/') {
        bail!("image_tag must not start or end with '/'");
    }
    if !value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        bail!("image_tag must start with a letter or digit");
    }
    if !value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/' | ':')
    }) {
        bail!("image_tag contains unsupported characters");
    }
    if value.contains("..") || value.contains("//") {
        bail!("image_tag contains an invalid separator sequence");
    }

    // A colon may only appear in the registry (first component, as a numeric
    // port) or in the final component (as the tag).
    let components: Vec<&str> = value.split('/').collect();
    let last_index = components.len() - 1;
    for (index, component) in components.iter().enumerate() {
        let Some((head, tail)) = component.split_once(':') else {
            continue;
        };
        if index == 0 && last_index > 0 {
            if head.is_empty()
                || tail.is_empty()
                || !tail.chars().all(|character| character.is_ascii_digit())
            {
                bail!("image_tag has an invalid registry port");
            }
            continue;
        }
        if index != last_index {
            bail!("image_tag has an invalid ':' placement");
        }
    }

    let final_component = components[last_index];
    if let Some((name, tag)) = final_component.split_once(':') {
        if name.is_empty() {
            bail!("image_tag has an empty repository name");
        }
        if tag.is_empty() {
            bail!("image_tag has an empty tag");
        }
        if tag.len() > 128 {
            bail!("image_tag tag is too long");
        }
        if !tag
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            bail!("image_tag has an invalid tag");
        }
        if !tag.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        }) {
            bail!("image_tag has an invalid tag");
        }
    }
    Ok(())
}

fn validate_repo_path(field: &str, value: &str, allow_root: bool) -> Result<()> {
    validate_field(field, value, MAX_PATH_LEN)?;
    if value.chars().any(char::is_whitespace) {
        bail!("{field} must not contain whitespace");
    }
    if value.contains('\\') {
        bail!("{field} must use '/' separators");
    }
    if value.starts_with('/') || value.starts_with('~') {
        bail!("{field} must be relative to the repository root");
    }
    if value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        bail!("{field} must not be an absolute Windows path");
    }
    if value.contains("//") {
        bail!("{field} must not contain empty path components");
    }
    if value.split('/').any(|component| component == "..") {
        bail!("{field} must not contain '..' path segments");
    }
    let is_root = value
        .split('/')
        .all(|component| component.is_empty() || component == ".");
    if is_root && !allow_root {
        bail!("{field} must point to a file, not the repository root");
    }
    Ok(())
}

fn is_https_url(value: &str) -> bool {
    value
        .as_bytes()
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"https://"))
}

fn parse_build_response(body: &[u8]) -> Result<BuildResponse> {
    serde_json::from_slice(body).context("build runner returned an invalid JSON response")
}

fn missing_exit_code() -> i32 {
    NO_EXIT_CODE
}

fn deserialize_exit_code<'de, D>(deserializer: D) -> std::result::Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<i32>::deserialize(deserializer)?;
    Ok(value.unwrap_or(NO_EXIT_CODE))
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

async fn http_error(response: Response) -> anyhow::Error {
    let status = response.status();
    let body = read_bounded(response, MAX_ERROR_BODY)
        .await
        .unwrap_or_default();
    anyhow!("{}", describe_http_error(status, &body))
}

async fn read_bounded(mut response: Response, limit: usize) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while body.len() < limit {
        let chunk = response
            .chunk()
            .await
            .context("could not read the build runner error response")?;
        let Some(chunk) = chunk else {
            break;
        };
        if chunk.is_empty() {
            break;
        }
        let take = (limit - body.len()).min(chunk.len());
        body.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            break;
        }
    }
    Ok(body)
}

fn describe_http_error(status: StatusCode, body: &[u8]) -> String {
    if let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(body) {
        let code = sanitize_excerpt(&envelope.error.code, 64);
        let message = sanitize_excerpt(&envelope.error.message, ERROR_EXCERPT_BYTES);
        if !message.is_empty() {
            return if code.is_empty() {
                format!("build runner returned HTTP {status}: {message}")
            } else {
                format!("build runner returned HTTP {status} ({code}): {message}")
            };
        }
    }

    let excerpt = sanitize_excerpt(&String::from_utf8_lossy(body), ERROR_EXCERPT_BYTES);
    if excerpt.is_empty() {
        format!("build runner returned HTTP {status}")
    } else {
        format!("build runner returned HTTP {status}: {excerpt}")
    }
}

fn sanitize_excerpt(value: &str, max_bytes: usize) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if result.len() + character.len_utf8() > max_bytes {
            break;
        }
        if character.is_control() {
            result.push(' ');
        } else {
            result.push(character);
        }
    }
    result.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn sample_request() -> BuildRequest {
        BuildRequest {
            git_url: "https://github.com/acme/app.git".to_owned(),
            branch: "main".to_owned(),
            image_tag: "acme/app:latest".to_owned(),
            dockerfile_path: None,
            context_dir: None,
            token: None,
        }
    }

    fn payload_value(kind: BuildKind, request: &BuildRequest) -> Value {
        serde_json::to_value(BuildPayload::for_request(kind, request).unwrap()).unwrap()
    }

    fn rejection(kind: BuildKind, request: &BuildRequest) -> String {
        BuildPayload::for_request(kind, request)
            .err()
            .expect("request should have been rejected")
            .to_string()
    }

    #[test]
    fn base_url_is_validated_and_trimmed() {
        let client = BuildRunnerClient::new("https://runner.example.com/", None).unwrap();
        assert_eq!(client.base_url, "https://runner.example.com");
        assert_eq!(
            client.endpoint("/build/dockerfile"),
            "https://runner.example.com/build/dockerfile"
        );

        let client = BuildRunnerClient::new("http://127.0.0.1:8787///", None).unwrap();
        assert_eq!(client.base_url, "http://127.0.0.1:8787");
        assert_eq!(client.endpoint("/healthz"), "http://127.0.0.1:8787/healthz");

        let client = BuildRunnerClient::new(" https://runner.example.com/base ", None).unwrap();
        assert_eq!(client.base_url, "https://runner.example.com/base");
        assert_eq!(
            client.endpoint("/build/nixpacks"),
            "https://runner.example.com/base/build/nixpacks"
        );
    }

    #[test]
    fn base_url_rejects_non_http_and_unsafe_forms() {
        for value in [
            "",
            "   ",
            "runner.example.com",
            "ftp://runner.example.com",
            "file:///tmp/runner.sock",
            "https://user:secret@runner.example.com",
            "https://user@runner.example.com",
            "https://runner.example.com/?token=secret",
            "https://runner.example.com/#frag",
            "https://runner.example.com/\u{0}",
        ] {
            assert!(
                BuildRunnerClient::new(value, None).is_err(),
                "expected rejection for {value:?}"
            );
        }

        for value in ["https://runner.example.com", "http://127.0.0.1:8787"] {
            assert!(
                BuildRunnerClient::new(value, None).is_ok(),
                "expected acceptance for {value:?}"
            );
        }
    }

    #[test]
    fn client_token_is_validated_and_redacted() {
        for token in ["", "short token", "line\nbreak", "tökén", "tab\tin-token"] {
            assert!(
                BuildRunnerClient::new("https://runner.example.com", Some(token)).is_err(),
                "expected rejection for token {token:?}"
            );
        }
        let client =
            BuildRunnerClient::new("https://runner.example.com", Some("0123456789abcdef")).unwrap();
        let debug = format!("{client:?}");
        assert!(!debug.contains("0123456789abcdef"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn from_optional_requires_only_a_url() {
        assert!(BuildRunnerClient::from_optional(None, None)
            .unwrap()
            .is_none());
        assert!(
            BuildRunnerClient::from_optional(Some("   "), Some("ignored"))
                .unwrap()
                .is_none()
        );
        assert!(
            BuildRunnerClient::from_optional(Some("https://runner.example.com"), Some("   "))
                .unwrap()
                .is_some()
        );
        assert!(BuildRunnerClient::from_optional(
            Some("https://runner.example.com"),
            Some("bad token")
        )
        .is_err());
        assert!(BuildRunnerClient::from_optional(Some("ftp://runner.example.com"), None).is_err());
    }

    #[test]
    fn dockerfile_payload_contains_optional_fields() {
        let mut request = sample_request();
        request.dockerfile_path = Some("deploy/Dockerfile".to_owned());
        request.context_dir = Some("deploy".to_owned());
        request.token = Some("ghp_example".to_owned());

        let value = payload_value(BuildKind::Dockerfile, &request);
        assert_eq!(value["git_url"], "https://github.com/acme/app.git");
        assert_eq!(value["branch"], "main");
        assert_eq!(value["image_tag"], "acme/app:latest");
        assert_eq!(value["dockerfile_path"], "deploy/Dockerfile");
        assert_eq!(value["context_dir"], "deploy");
        assert_eq!(value["token"], "ghp_example");
    }

    #[test]
    fn payload_omits_absent_optional_fields() {
        let request = sample_request();
        let value = payload_value(BuildKind::Dockerfile, &request);
        assert!(value.get("dockerfile_path").is_none());
        assert!(value.get("context_dir").is_none());
        assert!(value.get("token").is_none());
        assert_eq!(value.as_object().unwrap().len(), 3);

        let nixpacks = payload_value(BuildKind::Nixpacks, &request);
        assert_eq!(nixpacks.as_object().unwrap().len(), 3);
        assert!(nixpacks.get("token").is_none());
    }

    #[test]
    fn nixpacks_payload_ignores_nothing_and_rejects_dockerfile_options() {
        let mut request = sample_request();
        request.token = Some("ghp_example".to_owned());
        let value = payload_value(BuildKind::Nixpacks, &request);
        assert_eq!(value.as_object().unwrap().len(), 4);
        assert_eq!(value["token"], "ghp_example");

        request.dockerfile_path = Some("Dockerfile".to_owned());
        let error = rejection(BuildKind::Nixpacks, &request);
        assert!(error.contains("nixpacks builds"), "{error}");

        let mut request = sample_request();
        request.context_dir = Some(".".to_owned());
        assert!(BuildPayload::for_request(BuildKind::Nixpacks, &request).is_err());
    }

    #[test]
    fn request_token_is_redacted_from_debug() {
        let mut request = sample_request();
        request.token = Some("super-secret-value".to_owned());
        let debug = format!("{request:?}");
        assert!(!debug.contains("super-secret-value"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn request_fields_must_be_present_and_sane() {
        let mut request = sample_request();
        request.git_url.clear();
        assert!(rejection(BuildKind::Dockerfile, &request).contains("git_url"));

        let mut request = sample_request();
        request.branch.clear();
        assert!(rejection(BuildKind::Dockerfile, &request).contains("branch"));

        let mut request = sample_request();
        request.image_tag.clear();
        assert!(rejection(BuildKind::Dockerfile, &request).contains("image_tag"));

        let mut request = sample_request();
        request.branch = "ma\u{0}in".to_owned();
        assert!(rejection(BuildKind::Dockerfile, &request).contains("control"));

        let mut request = sample_request();
        request.image_tag = "acme/app\u{7f}".to_owned();
        assert!(rejection(BuildKind::Dockerfile, &request).contains("control"));

        let mut request = sample_request();
        request.branch = "a".repeat(MAX_BRANCH_LEN + 1);
        assert!(rejection(BuildKind::Dockerfile, &request).contains("at most"));
    }

    #[test]
    fn git_url_transport_is_restricted() {
        for value in [
            "-https://github.com/acme/app.git",
            "file:///srv/git/app.git",
            "git://github.com/acme/app.git",
            "http://github.com/acme/app.git",
            "https://user:secret@github.com/acme/app.git",
            "https://github.com/acme/app.git?token=secret",
            "https://github.com/acme/app.git#main",
            "https://github.com/",
            "https://github.com/acme/../app.git",
            "git@github.com:../app.git",
        ] {
            let mut request = sample_request();
            request.git_url = value.to_owned();
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
                "expected rejection for {value:?}"
            );
        }

        for value in [
            "https://github.com/acme/app.git",
            "ssh://git@github.com/acme/app.git",
            "git@github.com:acme/app.git",
        ] {
            let mut request = sample_request();
            request.git_url = value.to_owned();
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok(),
                "expected acceptance for {value:?}"
            );
        }
    }

    #[test]
    fn branches_follow_git_ref_rules() {
        for value in [
            "-main",
            "@",
            "/main",
            "main/",
            "feature//branch",
            ".hidden",
            "release.",
            "release..candidate",
            "release@{1}",
            "feature~1",
            "feature:1",
            "feature?",
            "feature*",
            "feature[1]",
            "feature\\1",
            "feature/.hidden",
            "feature/main.lock",
            "feature branch",
        ] {
            let mut request = sample_request();
            request.branch = value.to_owned();
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
                "expected rejection for {value:?}"
            );
        }

        for value in ["main", "release/v1.2", "feature-ABC_1"] {
            let mut request = sample_request();
            request.branch = value.to_owned();
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok(),
                "expected acceptance for {value:?}"
            );
        }
    }

    #[test]
    fn image_tags_are_checked() {
        for value in [
            "-app",
            "app latest",
            "app@sha256:abc123",
            "app//nested",
            "app/../nested",
            "/app",
            "app/",
            "app:",
            "app:-tag",
            "app:tag with space",
            "app:tag/extra",
        ] {
            let mut request = sample_request();
            request.image_tag = value.to_owned();
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
                "expected rejection for {value:?}"
            );
        }

        for value in [
            "app",
            "app:latest",
            "acme/app:v1.2.3",
            "localhost:5000/team/app:v1",
            "ghcr.io/acme/app",
        ] {
            let mut request = sample_request();
            request.image_tag = value.to_owned();
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok(),
                "expected acceptance for {value:?}"
            );
        }
    }

    #[test]
    fn repository_paths_are_relative_and_safe() {
        for value in [
            "/etc/passwd",
            "~/Dockerfile",
            "../Dockerfile",
            "deploy/../../Dockerfile",
            "deploy\\Dockerfile",
            "C:\\Dockerfile",
            "deploy//Dockerfile",
            "deploy Dockerfile",
            "deploy/\u{0}Dockerfile",
            ".",
            "..",
        ] {
            let mut request = sample_request();
            request.dockerfile_path = Some(value.to_owned());
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
                "expected dockerfile_path rejection for {value:?}"
            );
        }

        for value in [
            "Dockerfile",
            "./Dockerfile",
            "deploy/Dockerfile",
            "dir/Dockerfile",
        ] {
            let mut request = sample_request();
            request.dockerfile_path = Some(value.to_owned());
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok(),
                "expected dockerfile_path acceptance for {value:?}"
            );
        }

        let mut request = sample_request();
        request.context_dir = Some(".".to_owned());
        assert!(BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok());

        for value in ["/", "..", "../outside", "a/../b", "C:\\repo"] {
            let mut request = sample_request();
            request.context_dir = Some(value.to_owned());
            assert!(
                BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
                "expected context_dir rejection for {value:?}"
            );
        }
    }

    #[test]
    fn per_build_token_requires_https_and_json_only() {
        let mut request = sample_request();
        request.token = Some("token with space".to_owned());
        assert!(BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err());

        let mut request = sample_request();
        request.git_url = "ssh://git@github.com/acme/app.git".to_owned();
        request.token = Some("ghp_example".to_owned());
        let error = rejection(BuildKind::Dockerfile, &request);
        assert!(error.contains("https"), "{error}");

        let mut request = sample_request();
        request.token = Some("ghp_example".to_owned());
        assert!(BuildPayload::for_request(BuildKind::Nixpacks, &request).is_ok());
    }

    #[test]
    fn build_response_parses_status_and_optional_exit_code() {
        let response = parse_build_response(
            br#"{
                "status": "succeeded",
                "image_tag": "acme/app:latest",
                "exit_code": 0,
                "duration_ms": 1234,
                "log_tail": "ok",
                "truncated": false,
                "detail": null
            }"#,
        )
        .unwrap();
        assert_eq!(response.status, "succeeded");
        assert_eq!(response.exit_code, 0);
        assert_eq!(response.duration_ms, 1234);
        assert_eq!(response.log_tail, "ok");
        assert!(!response.truncated);
        assert!(response.detail.is_none());

        let timed_out = parse_build_response(
            br#"{
                "status": "timed_out",
                "image_tag": "acme/app:latest",
                "exit_code": null,
                "duration_ms": 9,
                "log_tail": "",
                "truncated": true,
                "detail": "build timed out after 1800s"
            }"#,
        )
        .unwrap();
        assert_eq!(timed_out.exit_code, NO_EXIT_CODE);
        assert!(timed_out.truncated);
        assert_eq!(
            timed_out.detail.as_deref(),
            Some("build timed out after 1800s")
        );

        let missing_exit = parse_build_response(
            br#"{
                "status": "failed",
                "image_tag": "acme/app:latest",
                "duration_ms": 1,
                "log_tail": "boom",
                "truncated": false
            }"#,
        )
        .unwrap();
        assert_eq!(missing_exit.exit_code, NO_EXIT_CODE);
    }

    #[test]
    fn build_response_rejects_malformed_payloads() {
        for body in [
            b"".as_slice(),
            b"not json".as_slice(),
            br#"{"status":"succeeded"}"#.as_slice(),
            br#"{"status":1,"duration_ms":0,"log_tail":"","truncated":false}"#.as_slice(),
            br#"{"status":"succeeded","exit_code":"0","duration_ms":0,"log_tail":"","truncated":false}"#.as_slice(),
        ] {
            assert!(
                parse_build_response(body).is_err(),
                "expected rejection for {}",
                String::from_utf8_lossy(body)
            );
        }
    }

    #[test]
    fn http_errors_are_compact_and_sanitized() {
        let message = describe_http_error(
            StatusCode::UNAUTHORIZED,
            br#"{"error":{"code":"unauthorized","message":"a valid bearer token is required"}}"#,
        );
        assert!(message.contains("401"));
        assert!(message.contains("unauthorized"));
        assert!(message.contains("a valid bearer token is required"));

        let message = describe_http_error(
            StatusCode::BAD_REQUEST,
            br#"{"error":{"code":"validation_error","message":"branch:\nbad\u0000value"}}"#,
        );
        assert!(!message.chars().any(char::is_control), "{message}");
        assert!(message.contains("branch:"));

        let message =
            describe_http_error(StatusCode::BAD_GATEWAY, b"<html>\nsecret-ish body\n</html>");
        assert!(message.contains("502"));
        assert!(!message.contains('\n'));
        assert!(message.contains("secret-ish body"));

        let message = describe_http_error(StatusCode::INTERNAL_SERVER_ERROR, b"");
        assert_eq!(
            message,
            "build runner returned HTTP 500 Internal Server Error"
        );
    }

    #[test]
    fn http_error_bodies_are_bounded() {
        let huge = format!(
            "{{\"error\":{{\"code\":\"x\",\"message\":\"{}\"}}}}",
            "a".repeat(10_000)
        );
        let message = describe_http_error(StatusCode::INTERNAL_SERVER_ERROR, huge.as_bytes());
        assert!(
            message.len() <= ERROR_EXCERPT_BYTES + 128,
            "{}",
            message.len()
        );

        let huge_plain = "b".repeat(10_000);
        let message = describe_http_error(StatusCode::BAD_GATEWAY, huge_plain.as_bytes());
        assert!(message.len() <= ERROR_EXCERPT_BYTES + 128);
    }

    #[test]
    fn json_serialization_does_not_leak_absent_tokens() {
        let request = sample_request();
        let serialized = serde_json::to_string(
            &BuildPayload::for_request(BuildKind::Dockerfile, &request).unwrap(),
        )
        .unwrap();
        let value: Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            value,
            json!({
                "git_url": "https://github.com/acme/app.git",
                "branch": "main",
                "image_tag": "acme/app:latest",
            })
        );
        assert!(!serialized.contains("token"));
    }

    #[test]
    fn compose_serialization_carries_context_and_rejects_dockerfile() {
        let mut request = sample_request();
        request.context_dir = Some("deploy".to_owned());
        let serialized = serde_json::to_string(
            &BuildPayload::for_request(BuildKind::Compose, &request).unwrap(),
        )
        .unwrap();
        let value: Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            value,
            json!({
                "git_url": "https://github.com/acme/app.git",
                "branch": "main",
                "image_tag": "acme/app:latest",
                "context_dir": "deploy",
            })
        );

        let mut request = sample_request();
        request.dockerfile_path = Some("Dockerfile".to_owned());
        assert!(BuildPayload::for_request(BuildKind::Compose, &request).is_err());

        let request = sample_request();
        let serialized = serde_json::to_string(
            &BuildPayload::for_request(BuildKind::Nixpacks, &request).unwrap(),
        )
        .unwrap();
        assert!(!serialized.contains("context_dir"));
        assert!(!serialized.contains("dockerfile_path"));
    }
}
