use super::super::docker::{
    ContainerPort, ContainerRow, DockerSnapshot, HostInfo, ImageRow, NetworkRow, RunResult,
    VolumeRow,
};
use crate::host_agent::{
    DockerContainerPort, DockerContainerRow, DockerHostInfo, DockerImageRow, DockerNetworkRow,
    DockerSnapshotResponse, DockerVolumeRow, RunContainerResponse,
};

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
