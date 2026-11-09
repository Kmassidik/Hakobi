//! Rollup row types and their insertion statements.

use anyhow::Result;
use rusqlite::{params, Connection};

/// One-minute rollup row for `metrics_1m`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rollup1m {
    pub cpu_avg: f64,
    pub cpu_max: f64,
    pub memory_avg: f64,
    pub memory_max: f64,
    pub disk_used: u64,
    pub network_rx_avg: f64,
    pub network_tx_avg: f64,
}

/// Five-minute rollup row for `metrics_5m`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rollup5m {
    pub cpu_avg: f64,
    pub cpu_max: f64,
    pub memory_avg: f64,
    pub disk_used: u64,
    pub network_rx_avg: f64,
    pub network_tx_avg: f64,
}

/// One-hour rollup row for `metrics_1h`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rollup1h {
    pub cpu_avg: f64,
    pub memory_avg: f64,
    pub disk_used: u64,
}

/// Sums a column over raw rows and divides by the row count.
fn average(values: impl IntoIterator<Item = f64>) -> f64 {
    let (sum, count) = values
        .into_iter()
        .fold((0.0, 0_u32), |(sum, count), value| (sum + value, count + 1));
    if count == 0 {
        0.0
    } else {
        sum / f64::from(count)
    }
}

/// Builds a 1-minute rollup from the live samples of one host over one minute.
///
/// Memory and disk use percentages so the rollup stays meaningful when total
/// capacity changes between samples; `memory_avg`/`memory_max` are percentages
/// and `disk_used` is the latest absolute byte count.
pub fn rollup_from_live(samples: &[crate::metrics::samples::LiveSample]) -> Rollup1m {
    let memory_pct = |sample: &crate::metrics::samples::LiveSample| {
        if sample.memory_total_bytes == 0 {
            0.0
        } else {
            100.0 * sample.memory_used_bytes as f64 / sample.memory_total_bytes as f64
        }
    };
    Rollup1m {
        cpu_avg: average(samples.iter().map(|sample| sample.cpu_percent)),
        cpu_max: samples
            .iter()
            .map(|sample| sample.cpu_percent)
            .fold(0.0, f64::max),
        memory_avg: average(samples.iter().map(memory_pct)),
        memory_max: samples.iter().map(memory_pct).fold(0.0, f64::max),
        disk_used: samples
            .iter()
            .map(|sample| sample.disk_used_bytes)
            .max()
            .unwrap_or(0),
        network_rx_avg: average(samples.iter().map(|sample| sample.network_rx_bps)),
        network_tx_avg: average(samples.iter().map(|sample| sample.network_tx_bps)),
    }
}

/// Builds a 5-minute rollup from five 1-minute rollups.
pub fn combine_1m(rollups: &[Rollup1m]) -> Rollup5m {
    Rollup5m {
        cpu_avg: average(rollups.iter().map(|rollup| rollup.cpu_avg)),
        cpu_max: rollups
            .iter()
            .map(|rollup| rollup.cpu_max)
            .fold(0.0, f64::max),
        memory_avg: average(rollups.iter().map(|rollup| rollup.memory_avg)),
        disk_used: rollups
            .iter()
            .map(|rollup| rollup.disk_used)
            .max()
            .unwrap_or(0),
        network_rx_avg: average(rollups.iter().map(|rollup| rollup.network_rx_avg)),
        network_tx_avg: average(rollups.iter().map(|rollup| rollup.network_tx_avg)),
    }
}

/// Builds a 1-hour rollup from sixty 5-minute rollups.
pub fn combine_5m(rollups: &[Rollup5m]) -> Rollup1h {
    Rollup1h {
        cpu_avg: average(rollups.iter().map(|rollup| rollup.cpu_avg)),
        memory_avg: average(rollups.iter().map(|rollup| rollup.memory_avg)),
        disk_used: rollups
            .iter()
            .map(|rollup| rollup.disk_used)
            .max()
            .unwrap_or(0),
    }
}

pub(crate) fn insert_1m(
    connection: &Connection,
    host_id: &str,
    timestamp_ms: i64,
    rollup: &Rollup1m,
) -> Result<()> {
    connection.execute(
        "INSERT INTO metrics_1m \
         (host_id, timestamp, cpu_avg, cpu_max, memory_avg, memory_max, disk_used, \
          network_rx_avg, network_tx_avg) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            host_id,
            timestamp_ms,
            rollup.cpu_avg,
            rollup.cpu_max,
            rollup.memory_avg,
            rollup.memory_max,
            rollup.disk_used as i64,
            rollup.network_rx_avg,
            rollup.network_tx_avg,
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_5m(
    connection: &Connection,
    host_id: &str,
    timestamp_ms: i64,
    rollup: &Rollup5m,
) -> Result<()> {
    connection.execute(
        "INSERT INTO metrics_5m \
         (host_id, timestamp, cpu_avg, cpu_max, memory_avg, disk_used, \
          network_rx_avg, network_tx_avg) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            host_id,
            timestamp_ms,
            rollup.cpu_avg,
            rollup.cpu_max,
            rollup.memory_avg,
            rollup.disk_used as i64,
            rollup.network_rx_avg,
            rollup.network_tx_avg,
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_1h(
    connection: &Connection,
    host_id: &str,
    timestamp_ms: i64,
    rollup: &Rollup1h,
) -> Result<()> {
    connection.execute(
        "INSERT INTO metrics_1h \
         (host_id, timestamp, cpu_avg, memory_avg, disk_used) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            host_id,
            timestamp_ms,
            rollup.cpu_avg,
            rollup.memory_avg,
            rollup.disk_used as i64,
        ],
    )?;
    Ok(())
}
