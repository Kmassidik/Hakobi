//! Container detail data extraction from bollard responses.
//!
//! The host agent serves `inspect`, `logs`, and `stats` from these types; the
//! dashboard renders them directly. Everything degrades to empty strings or
//! zero instead of failing when the daemon omits a field.

use std::collections::HashMap;

pub use bollard::models::{
    ContainerCpuStats, ContainerInspectResponse, ContainerStatsResponse, NetworkSettings,
};

use crate::host_agent::DockerContainerInspect as InspectDto;
use crate::host_agent::DockerContainerLogs as LogsDto;
use crate::host_agent::DockerContainerMount as InspectMount;
use crate::host_agent::DockerContainerStats as StatsDto;
use crate::providers::docker::ContainerPort;

/// Snapshot of one container's inspect data.
#[derive(Debug, Clone, Default)]
pub struct DockerInspect {
    pub id: String,
    pub name: String,
    pub image: String,
    pub image_id: String,
    pub command: String,
    pub state: String,
    pub status: String,
    pub health: Option<String>,
    pub created_at: String,
    pub started_at: String,
    pub restart_count: i64,
    pub platform: String,
    pub ports: Vec<ContainerPort>,
    pub mounts: Vec<ContainerMount>,
    pub env: Vec<String>,
    pub labels: Vec<(String, String)>,
}

/// One mounted path of a container.
#[derive(Debug, Clone, Default)]
pub struct ContainerMount {
    pub source: String,
    pub destination: String,
    pub mode: String,
    pub read_only: bool,
}

/// Last log lines of one container.
#[derive(Debug, Clone, Default)]
pub struct DockerLogs {
    pub lines: Vec<String>,
    pub truncated: bool,
}

/// One non-streaming resource usage sample of one container.
#[derive(Debug, Clone, Default)]
pub struct DockerContainerUsage {
    pub cpu_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_limit_bytes: u64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
}

impl DockerInspect {
    /// Flattens the nested bollard inspect response into the fields the UI
    /// shows. Unknown shapes degrade to empty strings instead of failing.
    pub(crate) fn from_bollard(inspect: &ContainerInspectResponse) -> Self {
        let config = inspect.config.as_ref();
        let state = inspect.state.as_ref();
        let mut command = inspect.path.clone().unwrap_or_default();
        if let Some(args) = inspect.args.as_ref() {
            if !args.is_empty() {
                command.push(' ');
                command.push_str(&args.join(" "));
            }
        }
        let image = config
            .and_then(|config| config.image.clone())
            .filter(|image| !image.is_empty())
            .unwrap_or_else(|| "unknown".into());
        let ports = network_ports(inspect.network_settings.as_ref());
        let mounts = inspect
            .mounts
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|mount| ContainerMount {
                source: mount.source.clone().unwrap_or_default(),
                destination: mount.destination.clone().unwrap_or_default(),
                mode: mount.mode.clone().unwrap_or_default(),
                read_only: !mount.rw.unwrap_or(true),
            })
            .collect();
        let mut labels = config
            .and_then(|config| config.labels.clone())
            .unwrap_or_default()
            .into_iter()
            .collect::<Vec<_>>();
        labels.sort();
        Self {
            id: inspect.id.clone().unwrap_or_default(),
            name: inspect
                .name
                .as_deref()
                .map(|name| name.trim_start_matches('/').to_owned())
                .unwrap_or_default(),
            image,
            image_id: inspect.image.clone().unwrap_or_default(),
            command,
            state: state
                .and_then(|state| state.status)
                .map(|status| format!("{status:?}").to_lowercase())
                .unwrap_or_else(|| "unknown".into()),
            status: String::new(),
            health: state
                .and_then(|state| state.health.as_ref())
                .and_then(|health| health.status)
                .map(|status| format!("{status:?}").to_lowercase()),
            created_at: inspect.created.clone().unwrap_or_default(),
            started_at: state
                .and_then(|state| state.started_at.clone())
                .unwrap_or_default(),
            restart_count: inspect.restart_count.unwrap_or_default(),
            platform: inspect.platform.clone().unwrap_or_default(),
            ports,
            mounts,
            env: config
                .and_then(|config| config.env.clone())
                .unwrap_or_default(),
            labels,
        }
    }
}

impl DockerContainerUsage {
    /// Derives CPU percentage and memory from a one-shot stats sample, using
    /// the same formula as `docker stats` (current minus previous window).
    pub(crate) fn from_bollard(sample: &ContainerStatsResponse) -> Self {
        let cpu = sample.cpu_stats.as_ref();
        let previous = sample.precpu_stats.as_ref();
        let cpu_percent = cpu_percent(cpu, previous);
        let memory = sample.memory_stats.as_ref();
        let usage = memory.and_then(|memory| memory.usage).unwrap_or_default();
        let limit = memory.and_then(|memory| memory.limit).unwrap_or_default();
        let mut rx = 0u64;
        let mut tx = 0u64;
        for network in sample
            .networks
            .as_ref()
            .map(HashMap::values)
            .into_iter()
            .flatten()
        {
            rx = rx.saturating_add(network.rx_bytes.unwrap_or_default());
            tx = tx.saturating_add(network.tx_bytes.unwrap_or_default());
        }
        Self {
            cpu_percent,
            memory_used_bytes: usage,
            memory_limit_bytes: limit,
            network_rx_bytes: rx,
            network_tx_bytes: tx,
        }
    }
}

impl From<DockerInspect> for InspectDto {
    fn from(inspect: DockerInspect) -> Self {
        Self {
            id: inspect.id,
            name: inspect.name,
            image: inspect.image,
            image_id: inspect.image_id,
            command: inspect.command,
            state: inspect.state,
            status: inspect.status,
            health: inspect.health,
            created_at: inspect.created_at,
            started_at: inspect.started_at,
            restart_count: inspect.restart_count,
            platform: inspect.platform,
            ports: inspect
                .ports
                .into_iter()
                .map(crate::host_agent::DockerContainerPort::from)
                .collect(),
            mounts: inspect
                .mounts
                .into_iter()
                .map(|mount| InspectMount {
                    source: mount.source,
                    destination: mount.destination,
                    mode: mount.mode,
                    read_only: mount.read_only,
                })
                .collect(),
            env: inspect.env,
            labels: inspect.labels,
        }
    }
}

impl From<DockerLogs> for LogsDto {
    fn from(logs: DockerLogs) -> Self {
        Self {
            lines: logs.lines,
            truncated: logs.truncated,
        }
    }
}

impl From<DockerContainerUsage> for StatsDto {
    fn from(usage: DockerContainerUsage) -> Self {
        Self {
            cpu_percent: usage.cpu_percent,
            memory_used_bytes: usage.memory_used_bytes,
            memory_limit_bytes: usage.memory_limit_bytes,
            network_rx_bytes: usage.network_rx_bytes,
            network_tx_bytes: usage.network_tx_bytes,
        }
    }
}

/// Container CPU percentage across the last stats window.
///
/// Returns `None` when the daemon did not report a usable window (for example
/// for a container that never ran), which callers render as a dash.
fn cpu_percent(current: Option<&ContainerCpuStats>, previous: Option<&ContainerCpuStats>) -> f64 {
    let Some(current) = current else {
        return 0.0;
    };
    let usage = current
        .cpu_usage
        .as_ref()
        .and_then(|usage| usage.total_usage);
    let system = current.system_cpu_usage;
    let Some((Some(usage), Some(system))) = Some((usage, system)) else {
        return 0.0;
    };
    let previous_usage = previous
        .and_then(|previous| previous.cpu_usage.as_ref())
        .and_then(|usage| usage.total_usage)
        .unwrap_or_default();
    let previous_system = previous
        .and_then(|previous| previous.system_cpu_usage)
        .unwrap_or_default();
    let system_delta = system.saturating_sub(previous_system);
    let cpu_delta = usage.saturating_sub(previous_usage);
    if system_delta == 0 {
        return 0.0;
    }
    let cores = current
        .online_cpus
        .map(|cores| cores.max(1) as f64)
        .unwrap_or_else(|| {
            current
                .cpu_usage
                .as_ref()
                .and_then(|usage| usage.percpu_usage.as_deref())
                .map(|cores| cores.len().max(1) as f64)
                .unwrap_or(1.0)
        });
    (cpu_delta as f64 / system_delta as f64) * cores * 100.0
}

/// Flattens the inspect port map into the rows shown by the UI.
fn network_ports(settings: Option<&NetworkSettings>) -> Vec<ContainerPort> {
    let mut ports = Vec::new();
    let Some(map) = settings.and_then(|settings| settings.ports.as_ref()) else {
        return ports;
    };
    for (key, bindings) in map {
        let Some((container_part, protocol)) = key.rsplit_once('/') else {
            continue;
        };
        let Ok(container_port) = container_part.parse::<u16>() else {
            continue;
        };
        for binding in bindings.as_deref().unwrap_or_default() {
            ports.push(ContainerPort {
                host_ip: binding.host_ip.clone().filter(|ip| !ip.is_empty()),
                host_port: binding
                    .host_port
                    .as_deref()
                    .and_then(|port| port.parse::<u16>().ok()),
                container_port,
                protocol: protocol.to_ascii_lowercase(),
            });
        }
    }
    ports.sort_by_key(|port| (port.host_port.unwrap_or(u16::MAX), port.container_port));
    ports
}
