use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use reqwest::Response;
use serde::de::DeserializeOwned;

use super::super::container_detail::{
    ContainerMount, DockerContainerUsage, DockerInspect, DockerLogs,
};
use super::super::docker::{
    validate_container_name, validate_image_reference, validate_shell_command, validate_volumes,
    ContainerBackend, ContainerPort, ContainerRow, ContainerVolume, DockerSnapshot, ReleaseResult,
    ResourceLimits, RunResult,
};
use super::local::ensure_local_endpoint;
use crate::buildrunner::{validate_branch, validate_public_git_url};
use crate::host_agent::{
    AgentEndpoint, AgentErrorBody, AgentHost, DockerContainerInspect, DockerContainerLogs,
    DockerContainerStats, DockerContainersResponse, DockerResourceRequest, DockerSnapshotResponse,
    DockerVersion, PublicSourceScanRequest, PublicSourceScanResponse, ReleaseContainerRequest,
    ReleaseContainerResponse, RemoveImageRequest, RunContainerRequest, RunContainerResponse,
};

// Client budgets stay above the agent's own limits (20 s for queries and
// mutations, 120 s for runs) so a slow Docker daemon surfaces as the agent's
// typed error instead of a local timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const QUERY_TIMEOUT: Duration = Duration::from_secs(25);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(30);
const RUN_TIMEOUT: Duration = Duration::from_secs(150);
const PUBLIC_SOURCE_TIMEOUT: Duration = Duration::from_secs(65);
pub(super) const MAX_ERROR_CHARS: usize = 500;
/// Mirrors the log window cap applied by the direct Docker backend.
const MAX_LOG_LINES: usize = 500;

/// [`ContainerBackend`] that forwards to the host agent's Docker endpoints.
pub struct AgentBackend {
    endpoint: AgentEndpoint,
    client: reqwest::Client,
}

impl AgentBackend {
    /// Builds a backend for an already validated agent endpoint.
    pub fn new(endpoint: AgentEndpoint) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(QUERY_TIMEOUT)
            .build()
            .context("build the host agent HTTP client")?;
        Ok(Self { endpoint, client })
    }
    /// Builds a backend from an `AgentHost` resolved from the catalog.
    pub fn from_host(host: AgentHost) -> Result<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(QUERY_TIMEOUT)
            .build()
            .context("build the host agent HTTP client")?;
        Ok(Self {
            endpoint: AgentEndpoint::from_host(host),
            client,
        })
    }

    /// Builds a backend from `HAKOBI_AGENT_URL` and `HAKOBI_AGENT_TOKEN`.
    pub fn from_env() -> Option<Self> {
        Self::new(AgentEndpoint::from_env()?).ok()
    }

    /// Connection settings this backend uses.
    pub fn endpoint(&self) -> &AgentEndpoint {
        &self.endpoint
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, limit: Duration) -> Result<T> {
        let response = self
            .client
            .get(format!("{}{path}", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
            .timeout(limit)
            .send()
            .await
            .with_context(|| format!("call host agent {path}"))?;
        decode(response).await
    }

    async fn post<B, T>(&self, path: &str, body: &B, limit: Duration) -> Result<T>
    where
        B: serde::Serialize + Sync,
        T: DeserializeOwned,
    {
        let response = self
            .client
            .post(format!("{}{path}", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
            .timeout(limit)
            .json(body)
            .send()
            .await
            .with_context(|| format!("call host agent {path}"))?;
        decode(response).await
    }

    async fn post_json_unit<B>(&self, path: &str, body: &B, limit: Duration) -> Result<()>
    where
        B: serde::Serialize + Sync,
    {
        let response = self
            .client
            .post(format!("{}{path}", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
            .timeout(limit)
            .json(body)
            .send()
            .await
            .with_context(|| format!("call host agent {path}"))?;
        let status = response.status();
        if !status.is_success() {
            bail!("{}", agent_error_message(response).await);
        }
        Ok(())
    }

    async fn post_unit(&self, path: &str, limit: Duration) -> Result<()> {
        let response = self
            .client
            .post(format!("{}{path}", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
            .timeout(limit)
            .send()
            .await
            .with_context(|| format!("call host agent {path}"))?;
        let status = response.status();
        if !status.is_success() {
            bail!("{}", agent_error_message(response).await);
        }
        Ok(())
    }
}

#[async_trait]
impl ContainerBackend for AgentBackend {
    async fn test(&self, endpoint: &str) -> Result<String> {
        ensure_local_endpoint(endpoint)?;
        let version: DockerVersion = self.get("/v1/docker/version", QUERY_TIMEOUT).await?;
        Ok(format!(
            "Host agent · Docker {} · API {}",
            version.version, version.api_version
        ))
    }

    async fn containers(&self, endpoint: &str) -> Result<Vec<ContainerRow>> {
        ensure_local_endpoint(endpoint)?;
        let response: DockerContainersResponse =
            self.get("/v1/docker/containers", QUERY_TIMEOUT).await?;
        Ok(response
            .containers
            .into_iter()
            .map(ContainerRow::from)
            .collect())
    }

    async fn snapshot(&self, endpoint: &str) -> Result<DockerSnapshot> {
        ensure_local_endpoint(endpoint)?;
        let response: DockerSnapshotResponse =
            self.get("/v1/docker/snapshot", QUERY_TIMEOUT).await?;
        Ok(DockerSnapshot::from(response))
    }

    async fn scan_public_source(
        &self,
        endpoint: &str,
        request: &PublicSourceScanRequest,
    ) -> Result<PublicSourceScanResponse> {
        ensure_local_endpoint(endpoint)?;
        validate_public_git_url(&request.git_url)?;
        if let Some(branch) = request.branch.as_deref() {
            validate_branch(branch)?;
        }
        self.post("/v1/public-source/scan", request, PUBLIC_SOURCE_TIMEOUT)
            .await
    }

    async fn start_container(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_unit(
            &format!("/v1/docker/containers/{name}/start"),
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn stop_container(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_unit(
            &format!("/v1/docker/containers/{name}/stop"),
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn restart_container(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_unit(
            &format!("/v1/docker/containers/{name}/restart"),
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn pause_container(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_unit(
            &format!("/v1/docker/containers/{name}/pause"),
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn unpause_container(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_unit(
            &format!("/v1/docker/containers/{name}/unpause"),
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn remove_container(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_unit(
            &format!("/v1/docker/containers/{name}/delete"),
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn remove_image(&self, endpoint: &str, image: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_image_reference(image)?;
        self.post_json_unit(
            "/v1/docker/images/remove",
            &RemoveImageRequest {
                image: image.to_owned(),
            },
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn create_network(&self, endpoint: &str, name: &str, app_id: Option<&str>) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_json_unit(
            "/v1/docker/networks/create",
            &DockerResourceRequest {
                name: name.to_owned(),
                app_id: app_id.map(str::to_owned),
            },
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn remove_network(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        if matches!(name, "bridge" | "host" | "none") {
            bail!("built-in network `{name}` cannot be removed");
        }
        self.post_json_unit(
            "/v1/docker/networks/delete",
            &DockerResourceRequest {
                name: name.to_owned(),
                app_id: None,
            },
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn create_volume(&self, endpoint: &str, name: &str, app_id: Option<&str>) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_json_unit(
            "/v1/docker/volumes/create",
            &DockerResourceRequest {
                name: name.to_owned(),
                app_id: app_id.map(str::to_owned),
            },
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn remove_volume(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_json_unit(
            "/v1/docker/volumes/delete",
            &DockerResourceRequest {
                name: name.to_owned(),
                app_id: None,
            },
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn run_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        container_port: Option<u16>,
        requested_host_port: Option<u16>,
        env: &std::collections::HashMap<String, String>,
        resources: ResourceLimits,
        start_command: Option<&str>,
        volumes: &[ContainerVolume],
    ) -> Result<RunResult> {
        ensure_local_endpoint(endpoint)?;
        validate_image_reference(image)?;
        validate_container_name(name)?;
        if container_port == Some(0) || requested_host_port == Some(0) {
            bail!("container and host ports must be between 1 and 65535");
        }
        if container_port.is_none() && requested_host_port.is_some() {
            bail!("a host port requires a container port");
        }
        if let Some(command) = start_command {
            validate_shell_command(command)?;
        }
        validate_volumes(volumes)?;
        let request = RunContainerRequest {
            image: image.to_owned(),
            name: name.to_owned(),
            container_port,
            host_port: requested_host_port,
            env: env.clone(),
            cpu_limit: resources.cpu,
            memory_limit_bytes: resources.memory_bytes,
            start_command: start_command.map(str::to_owned),
            volumes: volumes.to_vec(),
        };
        let response: RunContainerResponse = self
            .post("/v1/docker/containers/run", &request, RUN_TIMEOUT)
            .await?;
        Ok(RunResult::from(response))
    }

    async fn release_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        env: &std::collections::HashMap<String, String>,
        volumes: &[ContainerVolume],
        command: &str,
    ) -> Result<ReleaseResult> {
        ensure_local_endpoint(endpoint)?;
        validate_image_reference(image)?;
        validate_container_name(name)?;
        validate_volumes(volumes)?;
        validate_shell_command(command)?;
        if command.is_empty() {
            bail!("release command must not be empty");
        }
        let request = ReleaseContainerRequest {
            image: image.to_owned(),
            name: name.to_owned(),
            env: env.clone(),
            volumes: volumes.to_vec(),
            command: command.to_owned(),
        };
        let response: ReleaseContainerResponse = self
            .post("/v1/docker/containers/release", &request, RUN_TIMEOUT)
            .await?;
        Ok(ReleaseResult {
            exit_code: response.exit_code,
            logs: response.logs,
        })
    }

    async fn inspect_container(&self, endpoint: &str, name: &str) -> Result<DockerInspect> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        let inspect: DockerContainerInspect = self
            .get(
                &format!("/v1/docker/containers/{name}/inspect"),
                QUERY_TIMEOUT,
            )
            .await?;
        Ok(DockerInspect {
            id: inspect.id,
            name: inspect.name,
            image: inspect.image,
            image_id: inspect.image_id,
            command: inspect.command,
            state: inspect.state,
            status: inspect.status,
            health: inspect.health,
            created_at: inspect.created_at,
            started_at: inspect.started_at,
            restart_count: inspect.restart_count,
            platform: inspect.platform,
            ports: inspect.ports.into_iter().map(ContainerPort::from).collect(),
            mounts: inspect
                .mounts
                .into_iter()
                .map(|mount| ContainerMount {
                    source: mount.source,
                    destination: mount.destination,
                    mode: mount.mode,
                    read_only: mount.read_only,
                })
                .collect(),
            env: inspect.env,
            labels: inspect.labels,
        })
    }

    async fn container_logs(&self, endpoint: &str, name: &str, tail: usize) -> Result<DockerLogs> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        let tail = tail.clamp(1, MAX_LOG_LINES);
        let logs: DockerContainerLogs = self
            .get(
                &format!("/v1/docker/containers/{name}/logs?tail={tail}"),
                QUERY_TIMEOUT,
            )
            .await?;
        Ok(DockerLogs {
            lines: logs.lines,
            truncated: logs.truncated,
        })
    }

    async fn container_stats(&self, endpoint: &str, name: &str) -> Result<DockerContainerUsage> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        let stats: DockerContainerStats = self
            .get(
                &format!("/v1/docker/containers/{name}/stats"),
                QUERY_TIMEOUT,
            )
            .await?;
        Ok(DockerContainerUsage {
            cpu_percent: stats.cpu_percent,
            memory_used_bytes: stats.memory_used_bytes,
            memory_limit_bytes: stats.memory_limit_bytes,
            network_rx_bytes: stats.network_rx_bytes,
            network_tx_bytes: stats.network_tx_bytes,
        })
    }
}

async fn decode<T: DeserializeOwned>(response: Response) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        bail!("{}", agent_error_message(response).await);
    }
    response
        .json::<T>()
        .await
        .context("decode the host agent response")
}

/// Turns an agent error response into a bounded, secret-free message.
///
/// The agent already returns short JSON errors; the length bound here keeps a
/// misbehaving proxy from filling the dashboard UI.
async fn agent_error_message(response: Response) -> String {
    let status = response.status();
    match response.json::<AgentErrorBody>().await {
        Ok(body) if !body.error.trim().is_empty() => {
            format!(
                "host agent: {}",
                truncate_chars(body.error.trim(), MAX_ERROR_CHARS)
            )
        }
        _ => format!("host agent request failed with {status}"),
    }
}

pub(super) fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_owned();
    }
    let mut text: String = value.chars().take(limit).collect();
    text.push('…');
    text
}
