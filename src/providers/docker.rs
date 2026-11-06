use std::net::{IpAddr, Ipv4Addr};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use bollard::query_parameters::{
    ListContainersOptions, ListImagesOptions, ListNetworksOptions, ListVolumesOptions,
};
use bollard::{Docker, API_DEFAULT_VERSION};
use url::Url;

use crate::stacks::docker_exec::{remove_container as remove_docker_container, run_image};

pub use crate::stacks::docker_exec::RunResult;

const CONNECT_TIMEOUT_SECS: u64 = 8;
const MAX_IMAGE_LEN: usize = 255;
const MAX_NAME_LEN: usize = 128;

#[derive(Debug, Clone)]
pub struct ContainerPort {
    pub host_ip: Option<String>,
    pub host_port: Option<u16>,
    pub container_port: u16,
    pub protocol: String,
}

#[derive(Debug, Clone)]
pub struct ContainerRow {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub health: Option<String>,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    pub ports: Vec<ContainerPort>,
}

#[derive(Debug, Clone)]
pub struct ContainerGroup {
    pub name: String,
    pub containers: Vec<ContainerRow>,
}

#[derive(Debug, Clone)]
pub struct ServiceGroup {
    pub name: String,
    pub containers: Vec<ContainerRow>,
}

#[derive(Debug, Clone)]
pub struct StackGroup {
    pub name: String,
    pub services: Vec<ServiceGroup>,
    pub container_count: usize,
    pub running_count: usize,
}

#[derive(Debug, Clone)]
pub struct ContainerInventory {
    pub groups: Vec<ContainerGroup>,
    pub stacks: Vec<StackGroup>,
    pub suggested_ports: Vec<u16>,
}

impl ContainerInventory {
    pub fn from_rows(rows: Vec<ContainerRow>) -> Self {
        let mut groups = std::collections::BTreeMap::<String, Vec<ContainerRow>>::new();
        let mut stack_map = std::collections::BTreeMap::<
            String,
            std::collections::BTreeMap<String, Vec<ContainerRow>>,
        >::new();
        for row in &rows {
            let group = row
                .compose_project
                .clone()
                .unwrap_or_else(|| "Standalone".into());
            groups.entry(group.clone()).or_default().push(row.clone());
            let service = row
                .compose_service
                .clone()
                .unwrap_or_else(|| row.name.clone());
            stack_map
                .entry(group)
                .or_default()
                .entry(service)
                .or_default()
                .push(row.clone());
        }
        let mut groups = groups
            .into_iter()
            .map(|(name, containers)| ContainerGroup { name, containers })
            .collect::<Vec<_>>();
        groups.sort_by(|left, right| {
            let left_standalone = left.name == "Standalone";
            let right_standalone = right.name == "Standalone";
            left_standalone
                .cmp(&right_standalone)
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut stacks = stack_map
            .into_iter()
            .map(|(name, services)| {
                let mut services = services
                    .into_iter()
                    .map(|(name, containers)| ServiceGroup { name, containers })
                    .collect::<Vec<_>>();
                services.sort_by(|left, right| left.name.cmp(&right.name));
                let container_count = services
                    .iter()
                    .map(|service| service.containers.len())
                    .sum();
                let running_count = services
                    .iter()
                    .flat_map(|service| &service.containers)
                    .filter(|container| container.state == "running")
                    .count();
                StackGroup {
                    name,
                    services,
                    container_count,
                    running_count,
                }
            })
            .collect::<Vec<_>>();
        stacks.sort_by(|left, right| {
            let left_unmanaged = left.name == "Standalone" || left.name == "Unmanaged";
            let right_unmanaged = right.name == "Standalone" || right.name == "Unmanaged";
            left_unmanaged
                .cmp(&right_unmanaged)
                .then_with(|| left.name.cmp(&right.name))
        });
        let used: std::collections::BTreeSet<u16> = rows
            .iter()
            .flat_map(|row| row.ports.iter().filter_map(|port| port.host_port))
            .collect();
        let suggested_ports = (18000..18200)
            .filter(|port| !used.contains(port))
            .take(5)
            .collect();
        Self {
            groups,
            stacks,
            suggested_ports,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HostInfo {
    pub name: String,
    pub server_version: String,
    pub containers: i64,
    pub running: i64,
    pub images: i64,
    pub operating_system: String,
    pub architecture: String,
    pub cpus: i64,
}

#[derive(Debug, Clone)]
pub struct ImageRow {
    pub id: String,
    pub tags: Vec<String>,
    pub size_bytes: i64,
}

#[derive(Debug, Clone)]
pub struct VolumeRow {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
}

#[derive(Debug, Clone)]
pub struct NetworkRow {
    pub name: String,
    pub driver: String,
    pub scope: String,
}

#[derive(Debug, Clone)]
pub struct DockerSnapshot {
    pub host: HostInfo,
    pub images: Vec<ImageRow>,
    pub volumes: Vec<VolumeRow>,
    pub networks: Vec<NetworkRow>,
}

#[async_trait]
pub trait ContainerBackend: Send + Sync {
    async fn test(&self, endpoint: &str) -> Result<String>;
    async fn containers(&self, endpoint: &str) -> Result<Vec<ContainerRow>>;
    async fn snapshot(&self, endpoint: &str) -> Result<DockerSnapshot>;
    /// Starts an existing container. Never creates or pulls anything.
    async fn start_container(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Stops a running container.
    async fn stop_container(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Restarts a container.
    async fn restart_container(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Removes a container. Attached volumes are preserved.
    async fn remove_container(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Creates and starts a container from an image already present locally.
    async fn run_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        container_port: u16,
    ) -> Result<RunResult>;
}

pub struct DockerBackend;

pub fn validate_endpoint(endpoint: &str) -> Result<()> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() || endpoint.len() > 512 {
        bail!("provider endpoint must be 1–512 characters");
    }
    if endpoint.chars().any(char::is_control) {
        bail!("provider endpoint must not contain control characters");
    }
    if let Some(path) = endpoint.strip_prefix("unix://") {
        if !path.starts_with('/') || path.contains("..") || path == "/" {
            bail!("Unix provider endpoint must be an absolute socket path");
        }
        return Ok(());
    }
    if let Some(address) = endpoint.strip_prefix("tcp://") {
        let url = Url::parse(&format!("http://{address}/"))
            .context("Docker TCP endpoint must be host:port")?;
        let host = url.host_str().context("Docker TCP endpoint needs a host")?;
        if url.port().is_none() {
            bail!("Docker TCP endpoint must include a port");
        }
        if matches!(url.path(), "" | "/") && url.query().is_none() && url.username().is_empty() {
            if let Ok(address) = host.parse::<IpAddr>() {
                if address == IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)) {
                    bail!("metadata addresses are not allowed");
                }
            }
            return Ok(());
        }
    }
    bail!("use a unix:///path or tcp://host:port Docker endpoint")
}

fn connect(endpoint: &str) -> Result<Docker> {
    validate_endpoint(endpoint)?;
    if let Some(path) = endpoint.strip_prefix("unix://") {
        return Docker::connect_with_unix(path, CONNECT_TIMEOUT_SECS, API_DEFAULT_VERSION)
            .context("connect to Docker Unix socket");
    }
    let host = endpoint
        .strip_prefix("tcp://")
        .context("invalid Docker endpoint")?;
    Docker::connect_with_http(
        &format!("http://{host}"),
        CONNECT_TIMEOUT_SECS,
        API_DEFAULT_VERSION,
    )
    .context("connect to Docker TCP endpoint")
}

/// Validates a Docker container name before it reaches the Docker API.
///
/// Mirrors the rules applied by the execution layer: alphanumeric first
/// character, then letters, digits, `.`, `_` and `-` only.
pub fn validate_container_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        bail!("container name must be 1–{MAX_NAME_LEN} characters");
    }
    match name.chars().next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => bail!("container name must start with a letter or digit"),
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        bail!("container name may only contain letters, digits, '.', '_' and '-'");
    }
    Ok(())
}

/// Validates an image reference that must already exist locally.
///
/// The reference is never pulled, so this only rejects shapes Docker would
/// refuse or that could smuggle arguments into a registry URL.
pub fn validate_image_reference(image: &str) -> Result<()> {
    if image.is_empty() || image.len() > MAX_IMAGE_LEN {
        bail!("image reference must be 1–{MAX_IMAGE_LEN} characters");
    }
    if image
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        bail!("image reference must not contain whitespace or control characters");
    }
    match image.chars().next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => bail!("image reference must start with a letter or digit"),
    }
    if !image.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/' | ':' | '@')
    }) {
        bail!("image reference contains unsupported characters");
    }
    if image.contains("..") || image.contains("//") || image.contains("@@") {
        bail!("image reference contains an invalid separator sequence");
    }
    Ok(())
}

impl DockerBackend {
    /// Returns the Docker server version and the API version it exposes.
    ///
    /// Kept separate from [`ContainerBackend::test`] so the host agent can
    /// report the two values as typed data instead of an already formatted
    /// string.
    pub async fn version(&self, endpoint: &str) -> Result<(String, String)> {
        let version = connect(endpoint)?.version().await?;
        Ok((
            version.version.unwrap_or_else(|| "unknown".into()),
            version.api_version.unwrap_or_else(|| "unknown".into()),
        ))
    }
}

#[async_trait]
impl ContainerBackend for DockerBackend {
    async fn test(&self, endpoint: &str) -> Result<String> {
        let (server, api) = self.version(endpoint).await?;
        Ok(format!("Docker {server} · API {api}"))
    }

    async fn containers(&self, endpoint: &str) -> Result<Vec<ContainerRow>> {
        let options = ListContainersOptions {
            all: true,
            ..Default::default()
        };
        let containers = connect(endpoint)?.list_containers(Some(options)).await?;
        let mut rows: Vec<ContainerRow> = containers
            .into_iter()
            .map(|container| {
                let labels = container.labels.unwrap_or_default();
                let mut ports = container
                    .ports
                    .unwrap_or_default()
                    .into_iter()
                    .map(|port| ContainerPort {
                        host_ip: port.ip,
                        host_port: port.public_port,
                        container_port: port.private_port,
                        protocol: port
                            .typ
                            .map(|kind| format!("{kind:?}").to_lowercase())
                            .unwrap_or_else(|| "tcp".into()),
                    })
                    .collect::<Vec<_>>();
                ports.sort_by_key(|port| (port.host_port.unwrap_or(u16::MAX), port.container_port));
                let mut seen = std::collections::BTreeSet::new();
                ports.retain(|port| {
                    seen.insert((port.host_port, port.container_port, port.protocol.clone()))
                });
                ContainerRow {
                    id: container.id.unwrap_or_default(),
                    name: container
                        .names
                        .and_then(|names| names.into_iter().next())
                        .map(|name| name.trim_start_matches('/').to_owned())
                        .unwrap_or_else(|| "unnamed".into()),
                    image: container.image.unwrap_or_else(|| "unknown".into()),
                    state: container
                        .state
                        .map(|state| format!("{state:?}").to_lowercase())
                        .unwrap_or_else(|| "unknown".into()),
                    status: container.status.unwrap_or_default(),
                    health: container
                        .health
                        .and_then(|health| health.status)
                        .map(|status| format!("{status:?}").to_lowercase()),
                    compose_project: labels.get("com.docker.compose.project").cloned(),
                    compose_service: labels.get("com.docker.compose.service").cloned(),
                    ports,
                }
            })
            .collect();
        rows.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(rows)
    }

    async fn snapshot(&self, endpoint: &str) -> Result<DockerSnapshot> {
        let docker = connect(endpoint)?;
        let info = docker.info().await?;
        let images = docker
            .list_images(None::<ListImagesOptions>)
            .await?
            .into_iter()
            .map(|image| ImageRow {
                id: image.id,
                tags: image.repo_tags,
                size_bytes: image.size,
            })
            .collect::<Vec<_>>();
        let volumes = docker
            .list_volumes(None::<ListVolumesOptions>)
            .await?
            .volumes
            .unwrap_or_default()
            .into_iter()
            .map(|volume| VolumeRow {
                name: volume.name,
                driver: volume.driver,
                mountpoint: volume.mountpoint,
            })
            .collect::<Vec<_>>();
        let networks = docker
            .list_networks(None::<ListNetworksOptions>)
            .await?
            .into_iter()
            .map(|network| NetworkRow {
                name: network.name.unwrap_or_else(|| "unnamed".into()),
                driver: network.driver.unwrap_or_else(|| "unknown".into()),
                scope: network.scope.unwrap_or_else(|| "unknown".into()),
            })
            .collect::<Vec<_>>();

        Ok(DockerSnapshot {
            host: HostInfo {
                name: info.name.unwrap_or_else(|| "unknown".into()),
                server_version: info.server_version.unwrap_or_else(|| "unknown".into()),
                containers: info.containers.unwrap_or_default(),
                running: info.containers_running.unwrap_or_default(),
                images: info.images.unwrap_or_default(),
                operating_system: info.operating_system.unwrap_or_else(|| "unknown".into()),
                architecture: info.architecture.unwrap_or_else(|| "unknown".into()),
                cpus: info.ncpu.unwrap_or_default(),
            },
            images,
            volumes,
            networks,
        })
    }

    async fn start_container(&self, endpoint: &str, name: &str) -> Result<()> {
        validate_container_name(name)?;
        connect(endpoint)?
            .start_container(name, None)
            .await
            .with_context(|| format!("start container `{name}`"))
    }

    async fn stop_container(&self, endpoint: &str, name: &str) -> Result<()> {
        validate_container_name(name)?;
        connect(endpoint)?
            .stop_container(name, None)
            .await
            .with_context(|| format!("stop container `{name}`"))
    }

    async fn restart_container(&self, endpoint: &str, name: &str) -> Result<()> {
        validate_container_name(name)?;
        connect(endpoint)?
            .restart_container(name, None)
            .await
            .with_context(|| format!("restart container `{name}`"))
    }

    async fn remove_container(&self, endpoint: &str, name: &str) -> Result<()> {
        remove_docker_container(endpoint, name).await
    }

    async fn run_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        container_port: u16,
    ) -> Result<RunResult> {
        run_image(endpoint, image, name, container_port).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_endpoints_are_tightly_scoped() {
        assert!(validate_endpoint("unix:///var/run/docker.sock").is_ok());
        assert!(validate_endpoint("tcp://127.0.0.1:2375").is_ok());
        assert!(validate_endpoint("unix://relative.sock").is_err());
        assert!(validate_endpoint("unix:///var/../etc/passwd").is_err());
        assert!(validate_endpoint("tcp://127.0.0.1").is_err());
        assert!(validate_endpoint("tcp://169.254.169.254:80").is_err());
        assert!(validate_endpoint("http://127.0.0.1:2375").is_err());
    }

    #[test]
    fn container_names_are_restricted_to_docker_safe_characters() {
        assert!(validate_container_name("demo-api").is_ok());
        assert!(validate_container_name("demo.api_2").is_ok());
        assert!(validate_container_name("9front").is_ok());
        assert!(validate_container_name("").is_err());
        assert!(validate_container_name("-leading-dash").is_err());
        assert!(validate_container_name("has space").is_err());
        assert!(validate_container_name("slash/name").is_err());
        assert!(validate_container_name("shell;rm").is_err());
        assert!(validate_container_name("flag--option").is_ok());
        assert!(validate_container_name(&"a".repeat(129)).is_err());
    }

    #[test]
    fn image_references_reject_whitespace_and_shell_metacharacters() {
        assert!(validate_image_reference("alpine:3.20").is_ok());
        assert!(validate_image_reference("ghcr.io/hakobi/demo:latest").is_ok());
        assert!(
            validate_image_reference("registry.example.com:5000/team/app@sha256:abcdef").is_ok()
        );
        assert!(validate_image_reference("").is_err());
        assert!(validate_image_reference("-alpine").is_err());
        assert!(validate_image_reference("alpine latest").is_err());
        assert!(validate_image_reference("alpine;rm -rf /").is_err());
        assert!(validate_image_reference("alpine..latest").is_err());
        assert!(validate_image_reference("registry//app").is_err());
        assert!(validate_image_reference(&"a".repeat(256)).is_err());
    }
}
