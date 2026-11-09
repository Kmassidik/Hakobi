//! History queries powering the host resources charts.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

/// One point of host history, shaped for the chart templates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HistoryPoint {
    /// Milliseconds since the Unix epoch.
    pub timestamp: i64,
    pub cpu_percent: f64,
    /// Memory usage as a percentage of total (live) or of average total (1m/5m/1h).
    pub memory_percent: f64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub network_rx_bps: f64,
    pub network_tx_bps: f64,
}

/// Loads 1-minute rollups for one host within `[from, to]` (milliseconds).
pub fn query_1m(
    connection: &Connection,
    host_id: &str,
    from: i64,
    to: i64,
) -> Result<Vec<HistoryPoint>> {
    load_1m(connection, host_id, from, to)
}

/// Loads 5-minute rollups for one host within `[from, to]` (milliseconds).
pub fn query_5m(
    connection: &Connection,
    host_id: &str,
    from: i64,
    to: i64,
) -> Result<Vec<HistoryPoint>> {
    load_5m(connection, host_id, from, to)
}

/// Loads 1-hour rollups for one host within `[from, to]` (milliseconds).
pub fn query_1h(
    connection: &Connection,
    host_id: &str,
    from: i64,
    to: i64,
) -> Result<Vec<HistoryPoint>> {
    load_1h(connection, host_id, from, to)
}

fn load_1m(
    connection: &Connection,
    host_id: &str,
    from: i64,
    to: i64,
) -> Result<Vec<HistoryPoint>> {
    let mut statement = connection
        .prepare(
            "SELECT timestamp, cpu_avg, memory_avg, disk_used, network_rx_avg, network_tx_avg \
             FROM metrics_1m WHERE host_id = ?1 AND timestamp BETWEEN ?2 AND ?3 ORDER BY timestamp",
        )
        .context("prepare metrics_1m query")?;
    let rows = statement
        .query_map(params![host_id, from, to], map_rollup_row)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("read metrics_1m rows")?;
    Ok(rows)
}

fn load_5m(
    connection: &Connection,
    host_id: &str,
    from: i64,
    to: i64,
) -> Result<Vec<HistoryPoint>> {
    let mut statement = connection
        .prepare(
            "SELECT timestamp, cpu_avg, memory_avg, disk_used, network_rx_avg, network_tx_avg \
             FROM metrics_5m WHERE host_id = ?1 AND timestamp BETWEEN ?2 AND ?3 ORDER BY timestamp",
        )
        .context("prepare metrics_5m query")?;
    let rows = statement
        .query_map(params![host_id, from, to], map_rollup_row)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("read metrics_5m rows")?;
    Ok(rows)
}

fn load_1h(
    connection: &Connection,
    host_id: &str,
    from: i64,
    to: i64,
) -> Result<Vec<HistoryPoint>> {
    let mut statement = connection
        .prepare(
            "SELECT timestamp, cpu_avg, memory_avg, disk_used, 0.0, 0.0 \
             FROM metrics_1h WHERE host_id = ?1 AND timestamp BETWEEN ?2 AND ?3 ORDER BY timestamp",
        )
        .context("prepare metrics_1h query")?;
    let rows = statement
        .query_map(params![host_id, from, to], map_rollup_row)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("read metrics_1h rows")?;
    Ok(rows)
}

fn map_rollup_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryPoint> {
    Ok(HistoryPoint {
        timestamp: row.get(0)?,
        cpu_percent: row.get(1)?,
        memory_percent: row.get(2)?,
        disk_used_bytes: row.get::<_, i64>(3)?.max(0) as u64,
        disk_total_bytes: 0,
        network_rx_bps: row.get(4)?,
        network_tx_bps: row.get(5)?,
    })
}

/// Public re-export used by the MetricsDb CRUD surface.
impl super::MetricsDb {
    /// Reads 1-minute rollups for one host within `[from, to]` (milliseconds).
    pub fn query_1m(&self, host_id: &str, from: i64, to: i64) -> Result<Vec<HistoryPoint>> {
        query_1m(&self.connection, host_id, from, to)
    }

    /// Reads 5-minute rollups for one host within `[from, to]` (milliseconds).
    pub fn query_5m(&self, host_id: &str, from: i64, to: i64) -> Result<Vec<HistoryPoint>> {
        query_5m(&self.connection, host_id, from, to)
    }

    /// Reads 1-hour rollups for one host within `[from, to]` (milliseconds).
    pub fn query_1h(&self, host_id: &str, from: i64, to: i64) -> Result<Vec<HistoryPoint>> {
        query_1h(&self.connection, host_id, from, to)
    }
}
