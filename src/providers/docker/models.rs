/// Optional hard limits applied when a container is created.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ResourceLimits {
    pub cpu: Option<f64>,
    pub memory_bytes: Option<u64>,
}

impl ResourceLimits {
    pub fn validate(self) -> anyhow::Result<()> {
        if self
            .cpu
            .is_some_and(|value| !value.is_finite() || !(0.01..=256.0).contains(&value))
        {
            anyhow::bail!("CPU limit must be between 0.01 and 256 cores");
        }
        const MIN_MEMORY: u64 = 16 * 1024 * 1024;
        const MAX_MEMORY: u64 = 1024 * 1024 * 1024 * 1024;
        if self
            .memory_bytes
            .is_some_and(|value| !(MIN_MEMORY..=MAX_MEMORY).contains(&value))
        {
            anyhow::bail!("memory limit must be between 16 MiB and 1 TiB");
        }
        Ok(())
    }
}

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
