//! Live sample rows: the raw input captured from a host agent.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use anyhow::Result;

/// One host resource snapshot destined for `metrics_live`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveSample {
    pub cpu_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub network_rx_bps: f64,
    pub network_tx_bps: f64,
    pub load_1: f64,
}

impl LiveSample {
    /// Adapts an agent `SystemMetrics` snapshot into a storable sample. Disk
    /// figures come from the root mount, falling back to the first mount.
    pub fn from_system(metrics: &crate::host_agent::SystemMetrics) -> Self {
        let root_disk = metrics
            .disks
            .iter()
            .find(|disk| disk.mount == "/")
            .or_else(|| metrics.disks.first());
        let network_rx: u64 = metrics
            .networks
            .iter()
            .filter(|network| network.name != "lo")
            .map(|network| network.received_bytes_per_second)
            .sum();
        let network_tx: u64 = metrics
            .networks
            .iter()
            .filter(|network| network.name != "lo")
            .map(|network| network.transmitted_bytes_per_second)
            .sum();
        Self {
            cpu_percent: f64::from(metrics.cpu_usage_percent),
            memory_used_bytes: metrics.memory_used_bytes,
            memory_total_bytes: metrics.memory_total_bytes,
            disk_used_bytes: root_disk
                .map(|disk| disk.total_bytes.saturating_sub(disk.available_bytes))
                .unwrap_or(0),
            disk_total_bytes: root_disk.map(|disk| disk.total_bytes).unwrap_or(0),
            network_rx_bps: network_rx as f64,
            network_tx_bps: network_tx as f64,
            load_1: metrics.load_1,
        }
    }
}

pub(crate) const INSERT_LIVE: &str = "INSERT INTO metrics_live \
    (host_id, timestamp, cpu_percent, memory_used_bytes, memory_total_bytes, \
     disk_used_bytes, disk_total_bytes, network_rx_bps, network_tx_bps, load_1) \
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";

pub(crate) fn insert_live(
    connection: &Connection,
    host_id: &str,
    timestamp_ms: i64,
    sample: &LiveSample,
) -> Result<()> {
    connection.execute(
        INSERT_LIVE,
        params![
            host_id,
            timestamp_ms,
            sample.cpu_percent,
            sample.memory_used_bytes as i64,
            sample.memory_total_bytes as i64,
            sample.disk_used_bytes as i64,
            sample.disk_total_bytes as i64,
            sample.network_rx_bps,
            sample.network_tx_bps,
            sample.load_1,
        ],
    )?;
    Ok(())
}
