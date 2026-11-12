use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use bollard::query_parameters::{
    InspectContainerOptions, ListContainersOptions, ListImagesOptions, ListNetworksOptions,
    ListVolumesOptions, LogsOptionsBuilder, StatsOptionsBuilder,
};
use futures_util::StreamExt;

use super::container_detail::{DockerContainerUsage, DockerInspect, DockerLogs};
use super::validate::connect;
pub use super::validate::{validate_container_name, validate_endpoint, validate_image_reference};
use crate::providers::docker_exec::{remove_container as remove_docker_container, run_image};

pub use crate::providers::docker_exec::RunResult;

/// Upper bound of log lines the detail page and agent ever return.
pub(crate) const MAX_LOG_LINES: usize = 500;

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

    /// Returns the full inspect data of one container.
    ///
    /// Backends that cannot answer (the test stubs) fail loudly instead of
    /// inventing data.
    async fn inspect_container(&self, _endpoint: &str, _name: &str) -> Result<DockerInspect> {
        bail!("this backend does not support container inspect")
    }

    /// Returns the last `tail` log lines of one container.
    async fn container_logs(
        &self,
        _endpoint: &str,
        _name: &str,
        _tail: usize,
    ) -> Result<DockerLogs> {
        bail!("this backend does not support container logs")
    }

    /// Returns one non-streaming stats sample of one container.
    async fn container_stats(&self, _endpoint: &str, _name: &str) -> Result<DockerContainerUsage> {
        bail!("this backend does not support container stats")
    }
}

pub struct DockerBackend;

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

    /// Returns the full inspect data of one container.
    ///
    /// The container name is validated before it reaches the Docker API, which
    /// keeps path fragments out of the request URL.
    pub async fn inspect_container(&self, endpoint: &str, name: &str) -> Result<DockerInspect> {
        validate_container_name(name)?;
        let inspect = connect(endpoint)?
            .inspect_container(name, None::<InspectContainerOptions>)
            .await
            .with_context(|| format!("inspect container `{name}`"))?;
        Ok(DockerInspect::from_bollard(&inspect))
    }

    /// Returns the last `tail` log lines of one container.
    ///
    /// `tail` clamps to [`MAX_LOG_LINES`]; the returned flag reports whether
    /// the answer filled the whole window, so the UI can show truncation.
    pub async fn container_logs(
        &self,
        endpoint: &str,
        name: &str,
        tail: usize,
    ) -> Result<DockerLogs> {
        validate_container_name(name)?;
        let tail = tail.clamp(1, MAX_LOG_LINES);
        let options = LogsOptionsBuilder::new()
            .stdout(true)
            .stderr(true)
            .tail(&tail.to_string())
            .build();
        let stream = connect(endpoint)?.logs(name, Some(options));
        let collected = stream
            .take(tail)
            .map(|chunk| chunk.map(|chunk| chunk.to_string()))
            .collect::<Vec<_>>()
            .await;
        let mut lines = Vec::new();
        for chunk in collected {
            let chunk = chunk.with_context(|| format!("read logs of container `{name}`"))?;
            for line in chunk.lines() {
                lines.push(line.trim_end_matches(['\r']).to_owned());
            }
        }
        let truncated = lines.len() >= tail;
        lines.truncate(MAX_LOG_LINES);
        Ok(DockerLogs { lines, truncated })
    }

    /// Returns one non-streaming stats sample of one container.
    ///
    /// Uses the bollard stats API with `one_shot` so the daemon answers with a
    /// single JSON object instead of an endless stream.
    pub async fn container_stats(
        &self,
        endpoint: &str,
        name: &str,
    ) -> Result<DockerContainerUsage> {
        validate_container_name(name)?;
        let options = StatsOptionsBuilder::new()
            .stream(false)
            .one_shot(true)
            .build();
        let sample = connect(endpoint)?
            .stats(name, Some(options))
            .next()
            .await
            .with_context(|| format!("collect stats of container `{name}`"))??;
        Ok(DockerContainerUsage::from_bollard(&sample))
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
