//! Shared protocol between the dashboard and the native host agent.
//!
//! Both binaries link this module through the `hakobi` library crate, so the
//! Docker request and response types cannot drift apart. The types are plain
//! serde structs: unknown fields are ignored on purpose so a newer agent stays
//! compatible with an older dashboard.

use serde::{Deserialize, Serialize};

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

/// Connection settings for the host agent.
///
/// The token is never printed: [`std::fmt::Debug`] redacts it so request logs
/// and error reports stay free of credentials.
#[derive(Clone)]
pub struct AgentEndpoint {
    url: String,
    token: String,
}

impl AgentEndpoint {
    /// Builds validated settings, or `None` when the URL or token is unusable.
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Option<Self> {
        let url = url.into();
        let url = url.trim().trim_end_matches('/');
        let token = token.into();
        let token = token.trim();
        if token.len() < MIN_TOKEN_LEN || !is_agent_url(url) {
            return None;
        }
        Some(Self {
            url: url.to_owned(),
            token: token.to_owned(),
        })
    }

    /// Reads `HAKOBI_AGENT_URL` and `HAKOBI_AGENT_TOKEN` from the environment.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("HAKOBI_AGENT_URL").ok()?;
        let token = std::env::var("HAKOBI_AGENT_TOKEN").ok()?;
        Self::new(url, token)
    }

    /// Base URL of the agent, without a trailing slash.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Bearer token used for every agent request.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl std::fmt::Debug for AgentEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentEndpoint")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Accepts only plain loopback-style HTTP(S) base URLs.
fn is_agent_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return false;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return false;
    }
    matches!(parsed.path(), "" | "/")
}

/// Error body returned by the host agent for failed requests.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentErrorBody {
    #[serde(default)]
    pub error: String,
}

/// Aggregate resource usage reported by `GET /v1/docker/metrics`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DockerMetrics {
    #[serde(default)]
    pub containers: usize,
    #[serde(default)]
    pub cpu_percent: f64,
    #[serde(default)]
    pub memory_used_bytes: u64,
    #[serde(default)]
    pub memory_limit_bytes: u64,
}

/// State of an asynchronous host installer job.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallJob {
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub output: String,
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

/// Body of `POST /v1/docker/containers/run`.
///
/// Only an image that already exists locally, a container name, and a single
/// container port are accepted. There is no endpoint, command, or environment
/// field on purpose.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunContainerRequest {
    #[serde(default)]
    pub image: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub container_port: u16,
}

/// Result of `POST /v1/docker/containers/run`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunContainerResponse {
    #[serde(default)]
    pub container_id: String,
    #[serde(default)]
    pub host_port: u16,
    #[serde(default)]
    pub name: String,
}

fn default_protocol() -> String {
    "tcp".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_endpoints_use_the_agent_scheme() {
        assert!(is_local_endpoint(LOCAL_ENDPOINT));
        assert!(is_local_endpoint("  agent://local  "));
        assert!(is_local_endpoint("AGENT://local"));
        assert!(!is_local_endpoint("agent://"));
        assert!(!is_local_endpoint(DOCKER_SOCKET_ENDPOINT));
        assert!(!is_local_endpoint("tcp://127.0.0.1:2375"));
        assert!(!is_local_endpoint(""));
    }

    #[test]
    fn socket_endpoint_matches_the_socket_path() {
        assert_eq!(
            DOCKER_SOCKET_ENDPOINT,
            format!("unix://{DOCKER_SOCKET_PATH}")
        );
    }

    #[test]
    fn agent_endpoints_require_a_loopback_style_url_and_token() {
        let token = "a".repeat(MIN_TOKEN_LEN);
        let endpoint = AgentEndpoint::new("http://127.0.0.1:8787/", token.as_str()).unwrap();
        assert_eq!(endpoint.url(), "http://127.0.0.1:8787");
        assert_eq!(endpoint.token(), token.as_str());

        assert!(AgentEndpoint::new("http://127.0.0.1:8787", "short").is_none());
        assert!(AgentEndpoint::new("ftp://127.0.0.1:8787", token.as_str()).is_none());
        assert!(AgentEndpoint::new("http:///v1/docker", token.as_str()).is_none());
        assert!(AgentEndpoint::new("http://127.0.0.1:8787/prefix", token.as_str()).is_none());
        assert!(AgentEndpoint::new("http://user@127.0.0.1:8787", token.as_str()).is_none());
        assert!(AgentEndpoint::new("http://127.0.0.1:8787?token=1", token.as_str()).is_none());
    }

    #[test]
    fn agent_endpoint_debug_redacts_the_token() {
        let token = "b".repeat(MIN_TOKEN_LEN);
        let endpoint = AgentEndpoint::new("http://127.0.0.1:8787", token.as_str()).unwrap();
        let rendered = format!("{endpoint:?}");
        assert!(rendered.contains("http://127.0.0.1:8787"));
        assert!(!rendered.contains(&token));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn container_rows_round_trip_through_json() {
        let row = DockerContainerRow {
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
        };
        let encoded = serde_json::to_string(&row).unwrap();
        let decoded: DockerContainerRow = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, row);

        let wrapped = serde_json::to_string(&DockerContainersResponse {
            containers: vec![row.clone()],
        })
        .unwrap();
        let decoded: DockerContainersResponse = serde_json::from_str(&wrapped).unwrap();
        assert_eq!(decoded.containers, vec![row]);
    }

    #[test]
    fn snapshots_round_trip_through_json() {
        let snapshot = DockerSnapshotResponse {
            host: DockerHostInfo {
                name: "fedora-test".into(),
                server_version: "28.0.1".into(),
                containers: 2,
                running: 1,
                images: 3,
                operating_system: "Fedora Linux 44".into(),
                architecture: "x86_64".into(),
                cpus: 16,
            },
            images: vec![DockerImageRow {
                id: "sha256:abcdef".into(),
                tags: vec!["alpine:3.20".into()],
                size_bytes: 8 * 1_048_576,
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
        };
        let encoded = serde_json::to_string(&snapshot).unwrap();
        let decoded: DockerSnapshotResponse = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, snapshot);
    }

    #[test]
    fn run_requests_and_results_round_trip_through_json() {
        let request = RunContainerRequest {
            image: "alpine:3.20".into(),
            name: "demo-api".into(),
            container_port: 8080,
        };
        let encoded = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<RunContainerRequest>(&encoded).unwrap(),
            request
        );

        let result = RunContainerResponse {
            container_id: "feedface".into(),
            host_port: 18000,
            name: "demo-api".into(),
        };
        let encoded = serde_json::to_string(&result).unwrap();
        assert_eq!(
            serde_json::from_str::<RunContainerResponse>(&encoded).unwrap(),
            result
        );
    }

    #[test]
    fn metrics_and_install_jobs_round_trip_through_json() {
        let metrics = DockerMetrics {
            containers: 2,
            cpu_percent: 12.5,
            memory_used_bytes: 1_048_576,
            memory_limit_bytes: 8_388_608,
        };
        let encoded = serde_json::to_string(&metrics).unwrap();
        assert_eq!(
            serde_json::from_str::<DockerMetrics>(&encoded).unwrap(),
            metrics
        );

        let job = InstallJob {
            running: true,
            success: false,
            output: "installing".into(),
        };
        let encoded = serde_json::to_string(&job).unwrap();
        assert_eq!(serde_json::from_str::<InstallJob>(&encoded).unwrap(), job);

        // Missing fields fall back to defaults for forward compatibility.
        let decoded: InstallJob = serde_json::from_str(r#"{"running":true}"#).unwrap();
        assert!(decoded.running);
        assert!(!decoded.success);
        assert!(decoded.output.is_empty());
    }

    #[test]
    fn unknown_fields_and_missing_optional_fields_stay_compatible() {
        let decoded: DockerContainerRow = serde_json::from_str(
            r#"{"name":"demo-api","state":"running","future_field":{"nested":true}}"#,
        )
        .unwrap();
        assert_eq!(decoded.name, "demo-api");
        assert_eq!(decoded.state, "running");
        assert!(decoded.ports.is_empty());
        assert!(decoded.health.is_none());

        let decoded: DockerContainerPort =
            serde_json::from_str(r#"{"container_port":8080}"#).unwrap();
        assert_eq!(decoded.container_port, 8080);
        assert_eq!(decoded.protocol, "tcp");

        let decoded: DockerSnapshotResponse =
            serde_json::from_str(r#"{"host":{"name":"host"},"extra":[]}"#).unwrap();
        assert_eq!(decoded.host.name, "host");
        assert!(decoded.images.is_empty());

        let decoded: AgentErrorBody = serde_json::from_str(r#"{"error":"boom","code":7}"#).unwrap();
        assert_eq!(decoded.error, "boom");

        assert_eq!(
            serde_json::to_string(&DockerVersion::default()).unwrap(),
            r#"{"version":"","api_version":""}"#
        );
    }
}
