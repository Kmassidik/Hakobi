use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;

use super::super::container_detail::{DockerContainerUsage, DockerInspect, DockerLogs};
use super::super::docker::{
    ContainerBackend, ContainerRow, DockerBackend, DockerSnapshot, RunResult,
};
use super::backend::AgentBackend;
use crate::catalog::Catalog;
use crate::host_agent::{self, AgentEndpoint, AgentHost};

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
        env: &std::collections::HashMap<String, String>,
    ) -> Result<RunResult> {
        self.select(endpoint)
            .await?
            .run_container(endpoint, image, name, container_port, env)
            .await
    }

    async fn inspect_container(&self, endpoint: &str, name: &str) -> Result<DockerInspect> {
        self.select(endpoint)
            .await?
            .inspect_container(endpoint, name)
            .await
    }

    async fn container_logs(&self, endpoint: &str, name: &str, tail: usize) -> Result<DockerLogs> {
        self.select(endpoint)
            .await?
            .container_logs(endpoint, name, tail)
            .await
    }

    async fn container_stats(&self, endpoint: &str, name: &str) -> Result<DockerContainerUsage> {
        self.select(endpoint)
            .await?
            .container_stats(endpoint, name)
            .await
    }
}
