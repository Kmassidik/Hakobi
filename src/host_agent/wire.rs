use serde::{Deserialize, Serialize};

pub use crate::providers::ContainerVolume;

/// Error body returned by the host agent for failed requests.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentErrorBody {
    #[serde(default)]
    pub error: String,
}

/// Docker server version reported by the host agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerVersion {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub api_version: String,
}

/// Published port of a container.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerContainerPort {
    #[serde(default)]
    pub host_ip: Option<String>,
    #[serde(default)]
    pub host_port: Option<u16>,
    #[serde(default)]
    pub container_port: u16,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

/// One container reported by the host Docker engine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerContainerRow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub health: Option<String>,
    #[serde(default)]
    pub compose_project: Option<String>,
    #[serde(default)]
    pub compose_service: Option<String>,
    #[serde(default)]
    pub ports: Vec<DockerContainerPort>,
}

/// Response of `GET /v1/docker/containers`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerContainersResponse {
    #[serde(default)]
    pub containers: Vec<DockerContainerRow>,
}

/// Docker engine information reported by the host snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerHostInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub server_version: String,
    #[serde(default)]
    pub containers: i64,
    #[serde(default)]
    pub running: i64,
    #[serde(default)]
    pub images: i64,
    #[serde(default)]
    pub operating_system: String,
    #[serde(default)]
    pub architecture: String,
    #[serde(default)]
    pub cpus: i64,
}

/// One image reported by the host snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerImageRow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub size_bytes: i64,
}

/// One volume reported by the host snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerVolumeRow {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub mountpoint: String,
    #[serde(default)]
    pub app_id: Option<String>,
}

/// One network reported by the host snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerNetworkRow {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub app_id: Option<String>,
}

/// Response of `GET /v1/docker/snapshot`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerSnapshotResponse {
    #[serde(default)]
    pub host: DockerHostInfo,
    #[serde(default)]
    pub images: Vec<DockerImageRow>,
    #[serde(default)]
    pub volumes: Vec<DockerVolumeRow>,
    #[serde(default)]
    pub networks: Vec<DockerNetworkRow>,
}

/// Body of Docker network and volume create/delete endpoints.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerResourceRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
}

/// Body of `POST /v1/docker/images/remove`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoveImageRequest {
    #[serde(default)]
    pub image: String,
}

/// Body of `POST /v1/docker/containers/run`.
///
/// Accepts only local images and fixed Docker options. `container_port: null`
/// starts a private worker; a present port is published on loopback only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunContainerRequest {
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub container_port: Option<u16>,
    /// Requested loopback host port; omitted selects an automatic port.
    #[serde(default)]
    pub host_port: Option<u16>,
    /// Extra environment variables set inside the container.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// Maximum CPU cores; omitted means unlimited.
    #[serde(default)]
    pub cpu_limit: Option<f64>,
    /// Maximum memory in bytes; omitted means unlimited.
    #[serde(default)]
    pub memory_limit_bytes: Option<u64>,
    /// Optional command executed through `/bin/sh -c`, preserving ENTRYPOINT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<ContainerVolume>,
}

/// Result of `POST /v1/docker/containers/run`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunContainerResponse {
    #[serde(default)]
    pub container_id: String,
    /// Absent for private workers with no published port.
    #[serde(default)]
    pub host_port: Option<u16>,
    #[serde(default)]
    pub name: String,
}

/// Body of `POST /v1/docker/containers/release`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseContainerRequest {
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<ContainerVolume>,
    #[serde(default)]
    pub command: String,
}

/// Completed release command, including nonzero exits, with bounded logs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseContainerResponse {
    #[serde(default)]
    pub exit_code: i64,
    #[serde(default)]
    pub logs: String,
}

/// One mounted path of a container, reported by inspect.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerContainerMount {
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub read_only: bool,
}

/// Full inspect data of one container, reported by
/// `GET /v1/docker/containers/{name}/inspect`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DockerContainerInspect {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub image_id: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub health: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub restart_count: i64,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub ports: Vec<DockerContainerPort>,
    #[serde(default)]
    pub mounts: Vec<DockerContainerMount>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub labels: Vec<(String, String)>,
}

/// Per-container resource usage, reported by
/// `GET /v1/docker/containers/{name}/stats`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DockerContainerStats {
    #[serde(default)]
    pub cpu_percent: f64,
    #[serde(default)]
    pub memory_used_bytes: u64,
    #[serde(default)]
    pub memory_limit_bytes: u64,
    #[serde(default)]
    pub network_rx_bytes: u64,
    #[serde(default)]
    pub network_tx_bytes: u64,
}

/// Log lines of one container, reported by
/// `GET /v1/docker/containers/{name}/logs`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerContainerLogs {
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub truncated: bool,
}

fn default_protocol() -> String {
    "tcp".to_owned()
}
