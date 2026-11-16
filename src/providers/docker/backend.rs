use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use bollard::query_parameters::{
    InspectContainerOptions, ListContainersOptions, ListImagesOptions, ListNetworksOptions,
    ListVolumesOptions, LogsOptionsBuilder, RemoveImageOptions, StatsOptionsBuilder,
};
use futures_util::StreamExt;

use super::super::container_detail::{DockerContainerUsage, DockerInspect, DockerLogs};
use super::super::validate::{connect, connect_with_timeout};
use super::{validate_container_name, validate_image_reference};
use super::{
    ContainerPort, ContainerRow, DockerSnapshot, HostInfo, ImageRow, NetworkRow, ResourceLimits,
    VolumeRow, APP_OWNER_LABEL,
};
use crate::providers::docker_exec::{remove_container as remove_docker_container, run_image};

pub use crate::providers::docker_exec::RunResult;

/// Upper bound of log lines the detail page and agent ever return.
pub(crate) const MAX_LOG_LINES: usize = 500;

fn owner_labels(app_id: Option<&str>) -> Option<HashMap<String, String>> {
    app_id.map(|app_id| HashMap::from([(APP_OWNER_LABEL.to_owned(), app_id.to_owned())]))
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
    /// Pauses a running container.
    async fn pause_container(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Unpauses a paused container.
    async fn unpause_container(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Removes a container. Attached volumes are preserved.
    async fn remove_container(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Removes an image without forcing deletion or pruning other images.
    async fn remove_image(&self, endpoint: &str, image: &str) -> Result<()>;
    /// Creates a user-defined Docker network with the default driver.
    async fn create_network(&self, endpoint: &str, name: &str, app_id: Option<&str>) -> Result<()>;
    /// Removes a user-defined Docker network. Built-in networks are protected.
    async fn remove_network(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Creates a Docker volume with the default driver.
    async fn create_volume(&self, endpoint: &str, name: &str, app_id: Option<&str>) -> Result<()>;
    /// Removes a Docker volume without forcing deletion.
    async fn remove_volume(&self, endpoint: &str, name: &str) -> Result<()>;
    /// Creates and starts a container from an image already present locally.
    async fn run_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        container_port: u16,
        env: &std::collections::HashMap<String, String>,
        resources: ResourceLimits,
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
        let mut rows: Vec<ContainerRow> = containers.into_iter().map(container_row).collect();
        rows.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(rows)
    }

    async fn snapshot(&self, endpoint: &str) -> Result<DockerSnapshot> {
        let docker = connect(endpoint)?;
        let info = docker.info().await?;
        let images = list_images(&docker).await?;
        let volumes = list_volumes(&docker).await?;
        let networks = list_networks(&docker).await?;
        Ok(DockerSnapshot {
            host: host_info(info),
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
        connect_with_timeout(endpoint, 15)?
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

    async fn pause_container(&self, endpoint: &str, name: &str) -> Result<()> {
        validate_container_name(name)?;
        connect(endpoint)?
            .pause_container(name)
            .await
            .with_context(|| format!("pause container `{name}`"))
    }

    async fn unpause_container(&self, endpoint: &str, name: &str) -> Result<()> {
        validate_container_name(name)?;
        connect(endpoint)?
            .unpause_container(name)
            .await
            .with_context(|| format!("unpause container `{name}`"))
    }

    async fn remove_container(&self, endpoint: &str, name: &str) -> Result<()> {
        remove_docker_container(endpoint, name).await
    }

    async fn remove_image(&self, endpoint: &str, image: &str) -> Result<()> {
        validate_image_reference(image)?;
        connect(endpoint)?
            .remove_image(image, None::<RemoveImageOptions>, None)
            .await
            .with_context(|| format!("remove image `{image}`"))?;
        Ok(())
    }

    async fn create_network(&self, endpoint: &str, name: &str, app_id: Option<&str>) -> Result<()> {
        validate_container_name(name)?;
        connect(endpoint)?
            .create_network(bollard::models::NetworkCreateRequest {
                name: name.to_owned(),
                labels: owner_labels(app_id),
                ..Default::default()
            })
            .await
            .with_context(|| format!("create network `{name}`"))?;
        Ok(())
    }

    async fn remove_network(&self, endpoint: &str, name: &str) -> Result<()> {
        validate_container_name(name)?;
        if matches!(name, "bridge" | "host" | "none") {
            bail!("built-in network `{name}` cannot be removed");
        }
        connect(endpoint)?
            .remove_network(name)
            .await
            .with_context(|| format!("remove network `{name}`"))
    }

    async fn create_volume(&self, endpoint: &str, name: &str, app_id: Option<&str>) -> Result<()> {
        validate_container_name(name)?;
        connect(endpoint)?
            .create_volume(bollard::models::VolumeCreateRequest {
                name: Some(name.to_owned()),
                labels: owner_labels(app_id),
                ..Default::default()
            })
            .await
            .with_context(|| format!("create volume `{name}`"))?;
        Ok(())
    }

    async fn remove_volume(&self, endpoint: &str, name: &str) -> Result<()> {
        validate_container_name(name)?;
        connect(endpoint)?
            .remove_volume(name, None::<bollard::query_parameters::RemoveVolumeOptions>)
            .await
            .with_context(|| format!("remove volume `{name}`"))
    }

    async fn run_container(
        &self,
        endpoint: &str,
        image: &str,
        name: &str,
        container_port: u16,
        env: &std::collections::HashMap<String, String>,
        resources: ResourceLimits,
    ) -> Result<RunResult> {
        run_image(endpoint, image, name, container_port, env, resources).await
    }
}

fn container_row(container: bollard::models::ContainerSummary) -> ContainerRow {
    let labels = container.labels.unwrap_or_default();
    let ports = container_ports(container.ports.unwrap_or_default());
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
}

fn container_ports(ports: Vec<bollard::models::PortSummary>) -> Vec<ContainerPort> {
    let mut ports = ports
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
    ports.retain(|port| seen.insert((port.host_port, port.container_port, port.protocol.clone())));
    ports
}

async fn list_images(docker: &bollard::Docker) -> Result<Vec<ImageRow>> {
    Ok(docker
        .list_images(None::<ListImagesOptions>)
        .await?
        .into_iter()
        .map(|image| ImageRow {
            id: image.id,
            tags: image.repo_tags,
            size_bytes: image.size,
        })
        .collect())
}

async fn list_volumes(docker: &bollard::Docker) -> Result<Vec<VolumeRow>> {
    Ok(docker
        .list_volumes(None::<ListVolumesOptions>)
        .await?
        .volumes
        .unwrap_or_default()
        .into_iter()
        .map(|volume| VolumeRow {
            name: volume.name,
            driver: volume.driver,
            mountpoint: volume.mountpoint,
            app_id: volume.labels.get(APP_OWNER_LABEL).cloned(),
        })
        .collect())
}

async fn list_networks(docker: &bollard::Docker) -> Result<Vec<NetworkRow>> {
    Ok(docker
        .list_networks(None::<ListNetworksOptions>)
        .await?
        .into_iter()
        .map(|network| NetworkRow {
            name: network.name.unwrap_or_else(|| "unnamed".into()),
            driver: network.driver.unwrap_or_else(|| "unknown".into()),
            scope: network.scope.unwrap_or_else(|| "unknown".into()),
            app_id: network
                .labels
                .as_ref()
                .and_then(|labels| labels.get(APP_OWNER_LABEL))
                .cloned(),
        })
        .collect())
}

fn host_info(info: bollard::models::SystemInfo) -> HostInfo {
    HostInfo {
        name: info.name.unwrap_or_else(|| "unknown".into()),
        server_version: info.server_version.unwrap_or_else(|| "unknown".into()),
        containers: info.containers.unwrap_or_default(),
        running: info.containers_running.unwrap_or_default(),
        images: info.images.unwrap_or_default(),
        operating_system: info.operating_system.unwrap_or_else(|| "unknown".into()),
        architecture: info.architecture.unwrap_or_else(|| "unknown".into()),
        cpus: info.ncpu.unwrap_or_default(),
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::{owner_labels, APP_OWNER_LABEL};

    #[test]
    fn labels_are_added_only_for_owned_resources() {
        assert_eq!(owner_labels(None), None);
        let labels = owner_labels(Some("123e4567-e89b-42d3-a456-426614174000")).unwrap();
        assert_eq!(
            labels.get(APP_OWNER_LABEL).map(String::as_str),
            Some("123e4567-e89b-42d3-a456-426614174000")
        );
        assert_eq!(labels.len(), 1);
    }
}
