//! Shared protocol between the dashboard and the native host agent.
//!
//! Both binaries link this module through the `hakobi` library crate, so the
//! Docker request and response types cannot drift apart. The types are plain
//! serde structs: unknown fields are ignored on purpose so a newer agent stays
//! compatible with an older dashboard.

mod endpoint;
mod metrics;
mod wire;

#[cfg(test)]
mod tests;

pub use endpoint::{AgentEndpoint, AgentHost};
pub use metrics::{
    DockerMetrics, InstallJob, SystemDisk, SystemMetrics, SystemNetwork, SystemProcess,
};
pub use wire::{
    AgentErrorBody, DockerContainerInspect, DockerContainerLogs, DockerContainerMount,
    DockerContainerPort, DockerContainerRow, DockerContainerStats, DockerContainersResponse,
    DockerHostInfo, DockerImageRow, DockerNetworkRow, DockerSnapshotResponse, DockerVersion,
    DockerVolumeRow, RunContainerRequest, RunContainerResponse,
};

/// Provider endpoint that is served by the host agent instead of a socket.
pub const LOCAL_ENDPOINT: &str = "agent://local";

/// Path of the native Docker socket. The host agent never accepts another one.
pub const DOCKER_SOCKET_PATH: &str = "/var/run/docker.sock";

/// Provider endpoint of the native Docker socket on the host.
pub const DOCKER_SOCKET_ENDPOINT: &str = "unix:///var/run/docker.sock";

/// Name of the provider row that is registered automatically at startup.
pub const LOCAL_PROVIDER_NAME: &str = "Local host";

/// Kind of the provider row that is registered automatically at startup.
pub const LOCAL_PROVIDER_KIND: &str = "docker";

/// Minimum bearer token length, mirrored by the host agent binary.
pub const MIN_TOKEN_LEN: usize = 24;

/// Reports whether a provider endpoint is served by the host agent.
///
/// Only the `agent://` scheme is routed to the agent backend; every other
/// endpoint keeps using the direct Docker backend.
pub fn is_local_endpoint(endpoint: &str) -> bool {
    let Some((scheme, authority)) = endpoint.trim().split_once("://") else {
        return false;
    };
    scheme.eq_ignore_ascii_case("agent") && !authority.is_empty()
}
