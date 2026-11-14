use anyhow::{bail, Result};

use crate::catalog::Catalog;
use crate::host_agent::{self, AgentEndpoint, LOCAL_PROVIDER_KIND, LOCAL_PROVIDER_NAME};

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

pub(super) fn ensure_local_endpoint(endpoint: &str) -> Result<()> {
    if host_agent::is_local_endpoint(endpoint) {
        return Ok(());
    }
    bail!("the host agent backend only serves agent:// provider endpoints")
}
