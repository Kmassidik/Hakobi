use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use reqwest::Response;
use serde::de::DeserializeOwned;

use super::super::container_detail::{
    ContainerMount, DockerContainerUsage, DockerInspect, DockerLogs,
};
use super::super::docker::{
    validate_container_name, validate_image_reference, ContainerBackend, ContainerPort,
    ContainerRow, DockerSnapshot, RunResult,
};
use super::local::ensure_local_endpoint;
use crate::host_agent::{
    AgentEndpoint, AgentErrorBody, AgentHost, DockerContainerInspect, DockerContainerLogs,
    DockerContainerStats, DockerContainersResponse, DockerSnapshotResponse, DockerVersion,
    RunContainerRequest, RunContainerResponse,
};

// Client budgets stay above the agent's own limits (20 s for queries and
// mutations, 120 s for runs) so a slow Docker daemon surfaces as the agent's
// typed error instead of a local timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const QUERY_TIMEOUT: Duration = Duration::from_secs(25);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(30);
const RUN_TIMEOUT: Duration = Duration::from_secs(150);
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

    async fn remove_container(&self, endpoint: &str, name: &str) -> Result<()> {
        ensure_local_endpoint(endpoint)?;
        validate_container_name(name)?;
        self.post_unit(
            &format!("/v1/docker/containers/{name}/delete"),
            MUTATION_TIMEOUT,
        )
        .await
    }

    async fn run_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        container_port: u16,
        env: &std::collections::HashMap<String, String>,
    ) -> Result<RunResult> {
        ensure_local_endpoint(endpoint)?;
        validate_image_reference(image)?;
        validate_container_name(name)?;
        if container_port == 0 {
            bail!("container port must be between 1 and 65535");
        }
        let request = RunContainerRequest {
            image: image.to_owned(),
            name: name.to_owned(),
            container_port,
            env: env.clone(),
        };
        let response: RunContainerResponse = self
            .post("/v1/docker/containers/run", &request, RUN_TIMEOUT)
            .await?;
        Ok(RunResult::from(response))
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
