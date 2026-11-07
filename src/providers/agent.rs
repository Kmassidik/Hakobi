//! Container backend backed by the native host agent.
//!
//! The dashboard container has no Docker socket. When the host agent is
//! configured, provider rows using the `agent://` scheme are served through the
//! agent's typed Docker endpoints instead. Everything else keeps talking to
//! Docker directly, which is what [`RoutingBackend`] decides per request.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use reqwest::Response;
use serde::de::DeserializeOwned;

use super::docker::{
    validate_container_name, validate_image_reference, ContainerBackend, ContainerPort,
    ContainerRow, DockerBackend, DockerSnapshot, HostInfo, ImageRow, NetworkRow, RunResult,
    VolumeRow,
};
use crate::catalog::Catalog;
use crate::host_agent::{
    self, AgentEndpoint, AgentErrorBody, AgentHost, DockerContainerPort, DockerContainerRow,
    DockerContainersResponse, DockerHostInfo, DockerImageRow, DockerNetworkRow,
    DockerSnapshotResponse, DockerVersion, DockerVolumeRow, RunContainerRequest,
    RunContainerResponse, LOCAL_PROVIDER_KIND, LOCAL_PROVIDER_NAME,
};

// Client budgets stay above the agent's own limits (20 s for queries and
// mutations, 120 s for runs) so a slow Docker daemon surfaces as the agent's
// typed error instead of a local timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const QUERY_TIMEOUT: Duration = Duration::from_secs(25);
const MUTATION_TIMEOUT: Duration = Duration::from_secs(30);
const RUN_TIMEOUT: Duration = Duration::from_secs(150);
const MAX_ERROR_CHARS: usize = 500;

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
        };
        let response: RunContainerResponse = self
            .post("/v1/docker/containers/run", &request, RUN_TIMEOUT)
            .await?;
        Ok(RunResult::from(response))
    }
}

/// Routes each provider endpoint to the backend that can serve it.
///
/// `agent://…` endpoints go to the host agent backend; every other endpoint
/// keeps using the direct Docker backend. When no agent is configured, an
/// `agent://` endpoint fails with a configuration error instead of silently
/// falling back to a socket the process does not have.
pub struct RoutingBackend {
    agent: Option<Arc<dyn ContainerBackend>>,
    docker: Arc<dyn ContainerBackend>,
    catalog: Option<Arc<Mutex<Catalog>>>,
}
impl RoutingBackend {
    /// Builds the production backend: Docker over sockets plus the host agent
    /// backend when `HAKOBI_AGENT_URL` and `HAKOBI_AGENT_TOKEN` are configured.
    pub fn from_env() -> Self {
        let agent =
            AgentBackend::from_env().map(|backend| Arc::new(backend) as Arc<dyn ContainerBackend>);
        Self::new(Arc::new(DockerBackend), agent, None)
    }

    /// Builds the production backend with catalog-backed per-host resolution.
    pub fn from_env_with_catalog(catalog: Arc<Mutex<Catalog>>) -> Self {
        let agent =
            AgentBackend::from_env().map(|backend| Arc::new(backend) as Arc<dyn ContainerBackend>);
        Self::new(Arc::new(DockerBackend), agent, Some(catalog))
    }

    /// Builds the production backend with an explicitly supplied agent client.
    pub fn with_agent(endpoint: AgentEndpoint) -> Result<Self> {
        let agent = AgentBackend::new(endpoint)?;
        Ok(Self::new(
            Arc::new(DockerBackend),
            Some(Arc::new(agent) as Arc<dyn ContainerBackend>),
            None,
        ))
    }

    /// Builds the production backend with a catalog for per-host resolution.
    pub fn with_agent_and_catalog(
        endpoint: AgentEndpoint,
        catalog: Arc<Mutex<Catalog>>,
    ) -> Result<Self> {
        let agent = AgentBackend::new(endpoint)?;
        Ok(Self::new(
            Arc::new(DockerBackend),
            Some(Arc::new(agent) as Arc<dyn ContainerBackend>),
            Some(catalog),
        ))
    }

    pub fn new(
        docker: Arc<dyn ContainerBackend>,
        agent: Option<Arc<dyn ContainerBackend>>,
        catalog: Option<Arc<Mutex<Catalog>>>,
    ) -> Self {
        Self {
            agent,
            docker,
            catalog,
        }
    }

    async fn select(&self, endpoint: &str) -> Result<Arc<dyn ContainerBackend>> {
        let endpoint = endpoint.trim();
        if endpoint == host_agent::LOCAL_ENDPOINT {
            return self
                .agent
                .as_ref()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "the host agent is not configured; set HAKOBI_AGENT_URL and HAKOBI_AGENT_TOKEN"
                    )
                })
                .map(Arc::clone);
        }
        if let Some((_scheme, authority)) = endpoint.split_once("://") {
            if endpoint.starts_with("agent://") && !authority.is_empty() {
                if let Some(catalog) = &self.catalog {
                    let catalog = catalog.lock().unwrap();
                    let host_id = authority.to_string();
                    let token = catalog.host_agent_token(&host_id)?;
                    let agent_url = catalog
                        .list_host_agents()?
                        .into_iter()
                        .find(|ha| ha.id == host_id)
                        .map(|ha| ha.agent_url)
                        .ok_or_else(|| anyhow::anyhow!("host agent {host_id} not found"))?;
                    let host = AgentHost::new(host_id.clone(), agent_url, token)
                        .ok_or_else(|| anyhow::anyhow!("invalid host agent {host_id}"))?;
                    let backend = AgentBackend::from_host(host)?;
                    return Ok(Arc::new(backend));
                }
                // Compatibility for injected backends (and a catalog-open
                // failure): a configured agent serves the scheme.
                if let Some(agent) = &self.agent {
                    return Ok(Arc::clone(agent));
                }
                anyhow::bail!("the host agent catalog is not configured");
            }
        }
        Ok(Arc::clone(&self.docker))
    }
}

#[async_trait]
impl ContainerBackend for RoutingBackend {
    async fn test(&self, endpoint: &str) -> Result<String> {
        self.select(endpoint).await?.test(endpoint).await
    }

    async fn containers(&self, endpoint: &str) -> Result<Vec<ContainerRow>> {
        self.select(endpoint).await?.containers(endpoint).await
    }

    async fn snapshot(&self, endpoint: &str) -> Result<DockerSnapshot> {
        self.select(endpoint).await?.snapshot(endpoint).await
    }

    async fn start_container(&self, endpoint: &str, name: &str) -> Result<()> {
        self.select(endpoint)
            .await?
            .start_container(endpoint, name)
            .await
    }

    async fn stop_container(&self, endpoint: &str, name: &str) -> Result<()> {
        self.select(endpoint)
            .await?
            .stop_container(endpoint, name)
            .await
    }

    async fn restart_container(&self, endpoint: &str, name: &str) -> Result<()> {
        self.select(endpoint)
            .await?
            .restart_container(endpoint, name)
            .await
    }

    async fn remove_container(&self, endpoint: &str, name: &str) -> Result<()> {
        self.select(endpoint)
            .await?
            .remove_container(endpoint, name)
            .await
    }

    async fn run_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        container_port: u16,
    ) -> Result<RunResult> {
        self.select(endpoint)
            .await?
            .run_container(endpoint, image, name, container_port)
            .await
    }
}

/// Which local Docker access path this process can use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDockerAccess {
    /// Docker is reached through the native host agent.
    HostAgent,
    /// The process can open the Docker socket directly.
    DockerSocket,
}

impl LocalDockerAccess {
    /// Provider endpoint that represents this access path.
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::HostAgent => host_agent::LOCAL_ENDPOINT,
            Self::DockerSocket => host_agent::DOCKER_SOCKET_ENDPOINT,
        }
    }
}

/// Picks the local Docker access path available to this process.
///
/// The host agent wins when it is configured; the native socket is the
/// fallback. `None` means neither path exists, in which case no provider row is
/// registered at all.
pub fn detect_local_access() -> Option<LocalDockerAccess> {
    if AgentEndpoint::from_env().is_some() {
        return Some(LocalDockerAccess::HostAgent);
    }
    docker_socket_present().then_some(LocalDockerAccess::DockerSocket)
}

/// Reports whether the native Docker socket is accessible from this process.
pub fn docker_socket_present() -> bool {
    std::fs::metadata(host_agent::DOCKER_SOCKET_PATH)
        .map(|metadata| !metadata.is_dir())
        .unwrap_or(false)
}

/// Ensures the automatically managed local Docker provider exists.
///
/// The row is keyed by endpoint, so repeated startups reuse the same provider
/// instead of stacking duplicates. Existing rows are never modified: a provider
/// that was disabled or edited by the operator stays exactly as it is. When a
/// row already uses the managed name with a different endpoint, nothing is
/// created, which also keeps the unique name constraint satisfied.
pub fn ensure_local_provider(
    catalog: &mut Catalog,
    access: Option<LocalDockerAccess>,
) -> Result<Option<String>> {
    let Some(access) = access else {
        return Ok(None);
    };
    let endpoint = access.endpoint();
    let existing = catalog.list_provider_connections()?;
    if let Some(provider) = existing
        .iter()
        .find(|provider| provider.endpoint.trim() == endpoint)
    {
        return Ok(Some(provider.id.clone()));
    }
    if existing
        .iter()
        .any(|provider| provider.name == LOCAL_PROVIDER_NAME)
    {
        return Ok(None);
    }
    let id = catalog.create_provider_connection(
        LOCAL_PROVIDER_NAME,
        LOCAL_PROVIDER_KIND,
        endpoint,
        "",
    )?;
    Ok(Some(id))
}

fn ensure_local_endpoint(endpoint: &str) -> Result<()> {
    if host_agent::is_local_endpoint(endpoint) {
        return Ok(());
    }
    bail!("the host agent backend only serves agent:// provider endpoints")
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

fn truncate_chars(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_owned();
    }
    let mut text: String = value.chars().take(limit).collect();
    text.push('…');
    text
}

impl From<DockerContainerPort> for ContainerPort {
    fn from(port: DockerContainerPort) -> Self {
        Self {
            host_ip: port.host_ip,
            host_port: port.host_port,
            container_port: port.container_port,
            protocol: port.protocol,
        }
    }
}

impl From<ContainerPort> for DockerContainerPort {
    fn from(port: ContainerPort) -> Self {
        Self {
            host_ip: port.host_ip,
            host_port: port.host_port,
            container_port: port.container_port,
            protocol: port.protocol,
        }
    }
}

impl From<DockerContainerRow> for ContainerRow {
    fn from(row: DockerContainerRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            image: row.image,
            state: row.state,
            status: row.status,
            health: row.health,
            compose_project: row.compose_project,
            compose_service: row.compose_service,
            ports: row.ports.into_iter().map(ContainerPort::from).collect(),
        }
    }
}

impl From<ContainerRow> for DockerContainerRow {
    fn from(row: ContainerRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            image: row.image,
            state: row.state,
            status: row.status,
            health: row.health,
            compose_project: row.compose_project,
            compose_service: row.compose_service,
            ports: row
                .ports
                .into_iter()
                .map(DockerContainerPort::from)
                .collect(),
        }
    }
}

impl From<DockerHostInfo> for HostInfo {
    fn from(host: DockerHostInfo) -> Self {
        Self {
            name: host.name,
            server_version: host.server_version,
            containers: host.containers,
            running: host.running,
            images: host.images,
            operating_system: host.operating_system,
            architecture: host.architecture,
            cpus: host.cpus,
        }
    }
}

impl From<HostInfo> for DockerHostInfo {
    fn from(host: HostInfo) -> Self {
        Self {
            name: host.name,
            server_version: host.server_version,
            containers: host.containers,
            running: host.running,
            images: host.images,
            operating_system: host.operating_system,
            architecture: host.architecture,
            cpus: host.cpus,
        }
    }
}

impl From<DockerImageRow> for ImageRow {
    fn from(image: DockerImageRow) -> Self {
        Self {
            id: image.id,
            tags: image.tags,
            size_bytes: image.size_bytes,
        }
    }
}

impl From<ImageRow> for DockerImageRow {
    fn from(image: ImageRow) -> Self {
        Self {
            id: image.id,
            tags: image.tags,
            size_bytes: image.size_bytes,
        }
    }
}

impl From<DockerVolumeRow> for VolumeRow {
    fn from(volume: DockerVolumeRow) -> Self {
        Self {
            name: volume.name,
            driver: volume.driver,
            mountpoint: volume.mountpoint,
        }
    }
}

impl From<VolumeRow> for DockerVolumeRow {
    fn from(volume: VolumeRow) -> Self {
        Self {
            name: volume.name,
            driver: volume.driver,
            mountpoint: volume.mountpoint,
        }
    }
}

impl From<DockerNetworkRow> for NetworkRow {
    fn from(network: DockerNetworkRow) -> Self {
        Self {
            name: network.name,
            driver: network.driver,
            scope: network.scope,
        }
    }
}

impl From<NetworkRow> for DockerNetworkRow {
    fn from(network: NetworkRow) -> Self {
        Self {
            name: network.name,
            driver: network.driver,
            scope: network.scope,
        }
    }
}

impl From<DockerSnapshotResponse> for DockerSnapshot {
    fn from(snapshot: DockerSnapshotResponse) -> Self {
        Self {
            host: HostInfo::from(snapshot.host),
            images: snapshot.images.into_iter().map(ImageRow::from).collect(),
            volumes: snapshot.volumes.into_iter().map(VolumeRow::from).collect(),
            networks: snapshot
                .networks
                .into_iter()
                .map(NetworkRow::from)
                .collect(),
        }
    }
}

impl From<DockerSnapshot> for DockerSnapshotResponse {
    fn from(snapshot: DockerSnapshot) -> Self {
        Self {
            host: DockerHostInfo::from(snapshot.host),
            images: snapshot
                .images
                .into_iter()
                .map(DockerImageRow::from)
                .collect(),
            volumes: snapshot
                .volumes
                .into_iter()
                .map(DockerVolumeRow::from)
                .collect(),
            networks: snapshot
                .networks
                .into_iter()
                .map(DockerNetworkRow::from)
                .collect(),
        }
    }
}

impl From<RunContainerResponse> for RunResult {
    fn from(result: RunContainerResponse) -> Self {
        Self {
            container_id: result.container_id,
            host_port: result.host_port,
            name: result.name,
        }
    }
}

impl From<RunResult> for RunContainerResponse {
    fn from(result: RunResult) -> Self {
        Self {
            container_id: result.container_id,
            host_port: result.host_port,
            name: result.name,
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use axum::{
        extract::State,
        http::{header::AUTHORIZATION, HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        routing::{get, post},
        Json, Router,
    };
    use tempfile::TempDir;

    use super::*;
    use crate::catalog::Catalog;

    const TOKEN: &str = "test-token-0123456789abcdef";

    /// Backend that records which endpoint it was asked to serve.
    struct StubBackend(&'static str);

    #[async_trait]
    impl ContainerBackend for StubBackend {
        async fn test(&self, _endpoint: &str) -> Result<String> {
            Ok(format!("{} test", self.0))
        }

        async fn containers(&self, _endpoint: &str) -> Result<Vec<ContainerRow>> {
            Ok(Vec::new())
        }

        async fn snapshot(&self, _endpoint: &str) -> Result<DockerSnapshot> {
            bail!("{} snapshot", self.0)
        }

        async fn start_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
            Ok(())
        }

        async fn stop_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
            Ok(())
        }

        async fn restart_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
            Ok(())
        }

        async fn remove_container(&self, _endpoint: &str, _name: &str) -> Result<()> {
            Ok(())
        }

        async fn run_container(
            &self,
            _endpoint: &str,
            _image: &str,
            _name: &str,
            _container_port: u16,
        ) -> Result<RunResult> {
            bail!("{} run", self.0)
        }
    }

    fn routing(agent: Option<&'static str>) -> RoutingBackend {
        RoutingBackend::new(
            Arc::new(StubBackend("docker")),
            agent.map(|label| Arc::new(StubBackend(label)) as Arc<dyn ContainerBackend>),
            None,
        )
    }

    #[tokio::test]
    async fn routing_dispatches_by_endpoint_scheme() {
        let backend = routing(Some("agent"));
        assert_eq!(backend.test("agent://local").await.unwrap(), "agent test");
        assert_eq!(
            backend.test("unix:///var/run/docker.sock").await.unwrap(),
            "docker test"
        );
        assert_eq!(
            backend.test("tcp://127.0.0.1:2375").await.unwrap(),
            "docker test"
        );
        assert!(backend.snapshot("agent://local").await.is_err());
        assert!(backend
            .run_container("agent://local", "alpine:3.20", "demo", 80)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn routing_without_an_agent_reports_a_configuration_error() {
        let backend = routing(None);
        let error = backend.test("agent://local").await.unwrap_err().to_string();
        assert!(error.contains("HAKOBI_AGENT_URL"), "{error}");
        assert_eq!(
            backend.test("unix:///var/run/docker.sock").await.unwrap(),
            "docker test"
        );
    }

    fn catalog() -> (TempDir, Catalog) {
        let directory = tempfile::tempdir().unwrap();
        let catalog = Catalog::open(directory.path().join("catalog.sqlite3")).unwrap();
        (directory, catalog)
    }

    #[test]
    fn local_provider_registration_is_idempotent() {
        let (_directory, mut catalog) = catalog();
        let first = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent))
            .unwrap()
            .expect("provider registered");
        let second = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent))
            .unwrap()
            .expect("provider reused");
        assert_eq!(first, second);

        let providers = catalog.list_provider_connections().unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, LOCAL_PROVIDER_NAME);
        assert_eq!(providers[0].kind, LOCAL_PROVIDER_KIND);
        assert_eq!(providers[0].endpoint, host_agent::LOCAL_ENDPOINT);
        assert!(providers[0].enabled);
    }

    #[test]
    fn local_provider_registration_uses_the_socket_without_an_agent() {
        let (_directory, mut catalog) = catalog();
        ensure_local_provider(&mut catalog, Some(LocalDockerAccess::DockerSocket)).unwrap();
        let providers = catalog.list_provider_connections().unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].endpoint, host_agent::DOCKER_SOCKET_ENDPOINT);
        assert_eq!(providers[0].name, LOCAL_PROVIDER_NAME);
    }

    #[test]
    fn existing_provider_rows_are_preserved() {
        let (_directory, mut catalog) = catalog();
        let existing = catalog
            .create_provider_connection(
                "Primary docker",
                "docker",
                host_agent::DOCKER_SOCKET_ENDPOINT,
                "unused-secret",
            )
            .unwrap();

        // The socket path is already covered, so no second row appears and the
        // operator's row keeps its name, endpoint, and credential.
        let registered =
            ensure_local_provider(&mut catalog, Some(LocalDockerAccess::DockerSocket)).unwrap();
        assert_eq!(registered.as_deref(), Some(existing.as_str()));

        let providers = catalog.list_provider_connections().unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].name, "Primary docker");
        assert_eq!(providers[0].endpoint, host_agent::DOCKER_SOCKET_ENDPOINT);
        assert_eq!(catalog.provider_secret(&existing).unwrap(), "unused-secret");
    }

    #[test]
    fn switching_to_the_host_agent_adds_one_provider_without_touching_others() {
        let (_directory, mut catalog) = catalog();
        catalog
            .create_provider_connection(
                "Primary docker",
                "docker",
                host_agent::DOCKER_SOCKET_ENDPOINT,
                "unused-secret",
            )
            .unwrap();

        let id = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent))
            .unwrap()
            .expect("agent provider registered");
        let providers = catalog.list_provider_connections().unwrap();
        assert_eq!(providers.len(), 2);
        assert!(providers
            .iter()
            .any(|provider| provider.name == "Primary docker"
                && provider.endpoint == host_agent::DOCKER_SOCKET_ENDPOINT));
        let agent = providers
            .iter()
            .find(|provider| provider.id == id)
            .expect("agent provider present");
        assert_eq!(agent.name, LOCAL_PROVIDER_NAME);
        assert_eq!(agent.endpoint, host_agent::LOCAL_ENDPOINT);

        // The next start reuses the row instead of adding another one.
        let again =
            ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent)).unwrap();
        assert_eq!(again.as_deref(), Some(id.as_str()));
        assert_eq!(catalog.list_provider_connections().unwrap().len(), 2);
    }

    #[test]
    fn an_existing_managed_name_is_never_duplicated() {
        let (_directory, mut catalog) = catalog();
        catalog
            .create_provider_connection(
                LOCAL_PROVIDER_NAME,
                "docker",
                "tcp://127.0.0.1:2375",
                "unused-secret",
            )
            .unwrap();

        assert!(
            ensure_local_provider(&mut catalog, Some(LocalDockerAccess::HostAgent))
                .unwrap()
                .is_none()
        );
        let providers = catalog.list_provider_connections().unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].endpoint, "tcp://127.0.0.1:2375");
    }

    #[test]
    fn a_disabled_local_provider_is_not_reenabled() {
        let (_directory, mut catalog) = catalog();
        let id = ensure_local_provider(&mut catalog, Some(LocalDockerAccess::DockerSocket))
            .unwrap()
            .expect("provider registered");
        catalog.set_provider_enabled(&id, false).unwrap();

        ensure_local_provider(&mut catalog, Some(LocalDockerAccess::DockerSocket)).unwrap();
        let providers = catalog.list_provider_connections().unwrap();
        assert_eq!(providers.len(), 1);
        assert!(!providers[0].enabled);
    }

    #[test]
    fn no_provider_is_registered_without_an_agent_or_socket() {
        let (_directory, mut catalog) = catalog();
        assert!(ensure_local_provider(&mut catalog, None).unwrap().is_none());
        assert!(catalog.list_provider_connections().unwrap().is_empty());
    }

    #[test]
    fn local_access_endpoints_are_distinct() {
        assert_eq!(
            LocalDockerAccess::HostAgent.endpoint(),
            host_agent::LOCAL_ENDPOINT
        );
        assert_eq!(
            LocalDockerAccess::DockerSocket.endpoint(),
            host_agent::DOCKER_SOCKET_ENDPOINT
        );
    }

    /// Minimal in-process stand-in for the host agent Docker API.
    #[derive(Clone)]
    struct FakeAgent {
        token: String,
    }

    impl FakeAgent {
        fn authorized(&self, headers: &HeaderMap) -> bool {
            let expected = format!("Bearer {}", self.token);
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value == expected)
        }

        fn unauthorized_response() -> Response {
            (
                StatusCode::UNAUTHORIZED,
                Json(AgentErrorBody {
                    error: "unauthorized".into(),
                }),
            )
                .into_response()
        }
    }

    async fn fake_version(State(agent): State<FakeAgent>, headers: HeaderMap) -> Response {
        if !agent.authorized(&headers) {
            return FakeAgent::unauthorized_response();
        }
        Json(DockerVersion {
            version: "28.0.1".into(),
            api_version: "1.47".into(),
        })
        .into_response()
    }

    async fn fake_containers(State(agent): State<FakeAgent>, headers: HeaderMap) -> Response {
        if !agent.authorized(&headers) {
            return FakeAgent::unauthorized_response();
        }
        Json(DockerContainersResponse {
            containers: vec![DockerContainerRow {
                id: "0123456789abcdef".into(),
                name: "demo-api".into(),
                image: "hakobi/demo:latest".into(),
                state: "running".into(),
                status: "Up 2 hours".into(),
                health: Some("healthy".into()),
                compose_project: Some("hakobi-stack".into()),
                compose_service: Some("api".into()),
                ports: vec![DockerContainerPort {
                    host_ip: Some("0.0.0.0".into()),
                    host_port: Some(18080),
                    container_port: 8080,
                    protocol: "tcp".into(),
                }],
            }],
        })
        .into_response()
    }

    async fn fake_snapshot(State(agent): State<FakeAgent>, headers: HeaderMap) -> Response {
        if !agent.authorized(&headers) {
            return FakeAgent::unauthorized_response();
        }
        Json(DockerSnapshotResponse {
            host: DockerHostInfo {
                name: "fedora-test".into(),
                server_version: "28.0.1".into(),
                containers: 2,
                running: 1,
                images: 1,
                operating_system: "Fedora Linux 44".into(),
                architecture: "x86_64".into(),
                cpus: 16,
            },
            images: vec![DockerImageRow {
                id: "sha256:abcdef".into(),
                tags: vec!["alpine:3.20".into()],
                size_bytes: 1_048_576,
            }],
            volumes: vec![DockerVolumeRow {
                name: "hakobi-data".into(),
                driver: "local".into(),
                mountpoint: "/var/lib/docker/volumes/hakobi-data/_data".into(),
            }],
            networks: vec![DockerNetworkRow {
                name: "bridge".into(),
                driver: "bridge".into(),
                scope: "local".into(),
            }],
        })
        .into_response()
    }

    async fn fake_action(
        State(agent): State<FakeAgent>,
        headers: HeaderMap,
        axum::extract::Path(name): axum::extract::Path<String>,
    ) -> Response {
        if !agent.authorized(&headers) {
            return FakeAgent::unauthorized_response();
        }
        if name != "demo-api" {
            return (
                StatusCode::NOT_FOUND,
                Json(AgentErrorBody {
                    error: "no such container".into(),
                }),
            )
                .into_response();
        }
        StatusCode::NO_CONTENT.into_response()
    }

    async fn fake_run(
        State(agent): State<FakeAgent>,
        headers: HeaderMap,
        Json(request): Json<RunContainerRequest>,
    ) -> Response {
        if !agent.authorized(&headers) {
            return FakeAgent::unauthorized_response();
        }
        (
            StatusCode::CREATED,
            Json(RunContainerResponse {
                container_id: format!("id-{}", request.name),
                host_port: 18000,
                name: request.name,
            }),
        )
            .into_response()
    }

    async fn spawn_fake_agent(token: &str) -> String {
        let app = Router::new()
            .route("/v1/docker/version", get(fake_version))
            .route("/v1/docker/containers", get(fake_containers))
            .route("/v1/docker/snapshot", get(fake_snapshot))
            .route("/v1/docker/containers/run", post(fake_run))
            .route("/v1/docker/containers/{name}/start", post(fake_action))
            .route("/v1/docker/containers/{name}/stop", post(fake_action))
            .route("/v1/docker/containers/{name}/restart", post(fake_action))
            .route("/v1/docker/containers/{name}/delete", post(fake_action))
            .with_state(FakeAgent {
                token: token.to_owned(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn agent_backend_reads_the_typed_agent_api() {
        let url = spawn_fake_agent(TOKEN).await;
        let backend = AgentBackend::new(AgentEndpoint::new(url, TOKEN).unwrap()).unwrap();

        assert_eq!(
            backend.test(host_agent::LOCAL_ENDPOINT).await.unwrap(),
            "Host agent · Docker 28.0.1 · API 1.47"
        );

        let rows = backend
            .containers(host_agent::LOCAL_ENDPOINT)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "demo-api");
        assert_eq!(rows[0].compose_project.as_deref(), Some("hakobi-stack"));
        assert_eq!(rows[0].health.as_deref(), Some("healthy"));
        assert_eq!(rows[0].ports.len(), 1);
        assert_eq!(rows[0].ports[0].host_port, Some(18080));

        let snapshot = backend.snapshot(host_agent::LOCAL_ENDPOINT).await.unwrap();
        assert_eq!(snapshot.host.name, "fedora-test");
        assert_eq!(snapshot.host.cpus, 16);
        assert_eq!(snapshot.images.len(), 1);
        assert_eq!(snapshot.volumes.len(), 1);
        assert_eq!(snapshot.networks.len(), 1);
    }

    #[tokio::test]
    async fn agent_backend_sends_authenticated_mutations() {
        let url = spawn_fake_agent(TOKEN).await;
        let backend = AgentBackend::new(AgentEndpoint::new(url, TOKEN).unwrap()).unwrap();

        backend
            .start_container(host_agent::LOCAL_ENDPOINT, "demo-api")
            .await
            .unwrap();
        backend
            .stop_container(host_agent::LOCAL_ENDPOINT, "demo-api")
            .await
            .unwrap();
        backend
            .restart_container(host_agent::LOCAL_ENDPOINT, "demo-api")
            .await
            .unwrap();
        backend
            .remove_container(host_agent::LOCAL_ENDPOINT, "demo-api")
            .await
            .unwrap();

        let result = backend
            .run_container(host_agent::LOCAL_ENDPOINT, "alpine:3.20", "demo-api", 8080)
            .await
            .unwrap();
        assert_eq!(result.name, "demo-api");
        assert_eq!(result.host_port, 18000);
        assert_eq!(result.container_id, "id-demo-api");

        let missing = backend
            .start_container(host_agent::LOCAL_ENDPOINT, "other-api")
            .await
            .unwrap_err()
            .to_string();
        assert!(missing.contains("no such container"), "{missing}");
    }

    #[tokio::test]
    async fn agent_backend_rejects_bad_input_before_calling_the_agent() {
        let url = spawn_fake_agent(TOKEN).await;
        let backend = AgentBackend::new(AgentEndpoint::new(url, TOKEN).unwrap()).unwrap();

        assert!(backend
            .start_container(host_agent::LOCAL_ENDPOINT, "bad name")
            .await
            .is_err());
        assert!(backend
            .run_container(
                host_agent::LOCAL_ENDPOINT,
                "alpine latest",
                "demo-api",
                8080
            )
            .await
            .is_err());
        assert!(backend
            .run_container(host_agent::LOCAL_ENDPOINT, "alpine:3.20", "demo-api", 0)
            .await
            .is_err());
        assert!(backend.test("unix:///var/run/docker.sock").await.is_err());
    }

    #[tokio::test]
    async fn agent_backend_reports_authentication_failures() {
        let url = spawn_fake_agent(TOKEN).await;
        let backend =
            AgentBackend::new(AgentEndpoint::new(url, "another-token-0123456789abcdef").unwrap())
                .unwrap();

        let error = backend
            .containers(host_agent::LOCAL_ENDPOINT)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("unauthorized"), "{error}");
    }

    #[test]
    fn error_messages_are_bounded() {
        let long = "x".repeat(MAX_ERROR_CHARS + 10);
        let bounded = truncate_chars(&long, MAX_ERROR_CHARS);
        assert_eq!(bounded.chars().count(), MAX_ERROR_CHARS + 1);
        assert!(bounded.ends_with('…'));
    }

    #[test]
    fn conversion_keeps_container_identity() {
        let row = ContainerRow {
            id: "id".into(),
            name: "demo".into(),
            image: "alpine:3.20".into(),
            state: "running".into(),
            status: "Up".into(),
            health: None,
            compose_project: None,
            compose_service: None,
            ports: vec![ContainerPort {
                host_ip: None,
                host_port: Some(18000),
                container_port: 80,
                protocol: "tcp".into(),
            }],
        };
        let round_trip = ContainerRow::from(DockerContainerRow::from(row.clone()));
        assert_eq!(round_trip.id, row.id);
        assert_eq!(round_trip.name, row.name);
        assert_eq!(round_trip.ports.len(), 1);
        assert_eq!(round_trip.ports[0].container_port, 80);
    }
}
