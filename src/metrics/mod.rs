//! SQLite metrics time-series database (CP7).
//!
//! The metrics database is separate from the catalog: it stores live host
//! samples plus 1-minute, 5-minute, and 1-hour rollups, each with its own
//! retention window. See PRD section 3.2 for the schema contract.

mod history;
mod rollups;
mod sampler;
mod samples;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::Connection;

pub use history::{HistoryPoint, LatestLive};
pub use rollups::{combine_1m, combine_5m, rollup_from_live, Rollup1h, Rollup1m, Rollup5m};
pub use sampler::{
    advance_rollups, sample_host, spawn_retention_job, spawn_sampler, SamplingReport,
};
pub use samples::LiveSample;

/// Retention for live samples: 5 minutes, per the PRD ring-buffer contract.
pub(crate) const LIVE_RETENTION_MS: i64 = 5 * 60_000;
/// Retention for 1-minute rollups: 7 days.
pub(crate) const MINUTE_RETENTION_MS: i64 = 7 * 86_400_000;
/// Retention for 5-minute rollups: 30 days.
pub(crate) const FIVE_MINUTE_RETENTION_MS: i64 = 30 * 86_400_000;
/// Retention for 1-hour rollups: 90 days.
pub(crate) const HOUR_RETENTION_MS: i64 = 90 * 86_400_000;

/// Rollup cadence: a 5-minute rollup is written every 5 sampling ticks.
pub(crate) const ROLLUP_5M_EVERY: i64 = 5;
/// Rollup cadence: a 1-hour rollup is written every 60 sampling ticks.
pub(crate) const ROLLUP_1H_EVERY: i64 = 60;

const SCHEMA: &str = r#"
-- Live samples (ring buffer, 5 minutes max retention)
CREATE TABLE IF NOT EXISTS metrics_live (
    host_id            TEXT NOT NULL,
    timestamp          INTEGER NOT NULL,
    cpu_percent        REAL NOT NULL,
    memory_used_bytes  INTEGER NOT NULL,
    memory_total_bytes INTEGER NOT NULL,
    disk_used_bytes    INTEGER NOT NULL,
    disk_total_bytes   INTEGER NOT NULL,
    network_rx_bps     REAL NOT NULL,
    network_tx_bps     REAL NOT NULL,
    load_1             REAL NOT NULL
);

-- 1-minute rollups (7 days retention)
CREATE TABLE IF NOT EXISTS metrics_1m (
    host_id        TEXT NOT NULL,
    timestamp      INTEGER NOT NULL,
    cpu_avg        REAL NOT NULL,
    cpu_max        REAL NOT NULL,
    memory_avg     REAL NOT NULL,
    memory_max     REAL NOT NULL,
    disk_used      INTEGER NOT NULL,
    network_rx_avg REAL NOT NULL,
    network_tx_avg REAL NOT NULL
);

-- 5-minute rollups (30 days retention)
CREATE TABLE IF NOT EXISTS metrics_5m (
    host_id        TEXT NOT NULL,
    timestamp      INTEGER NOT NULL,
    cpu_avg        REAL NOT NULL,
    cpu_max        REAL NOT NULL,
    memory_avg     REAL NOT NULL,
    disk_used      INTEGER NOT NULL,
    network_rx_avg REAL NOT NULL,
    network_tx_avg REAL NOT NULL
);

-- 1-hour rollups (90 days retention)
CREATE TABLE IF NOT EXISTS metrics_1h (
    host_id    TEXT NOT NULL,
    timestamp  INTEGER NOT NULL,
    cpu_avg    REAL NOT NULL,
    memory_avg REAL NOT NULL,
    disk_used  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_metrics_live_host_ts ON metrics_live (host_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_metrics_1m_host_ts ON metrics_1m (host_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_metrics_5m_host_ts ON metrics_5m (host_id, timestamp);
CREATE INDEX IF NOT EXISTS idx_metrics_1h_host_ts ON metrics_1h (host_id, timestamp);

-- Internal sampler bookkeeping: per-host tick counter driving rollup cadence.
CREATE TABLE IF NOT EXISTS sampler_state (
    host_id    TEXT PRIMARY KEY NOT NULL,
    tick_count INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
"#;

/// Resolves the metrics database path that lives next to a catalog database.
pub fn metrics_database_path(catalog_path: &Path) -> PathBuf {
    catalog_path.with_file_name("metrics.sqlite3")
}

/// Current wall-clock time as milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Rows removed by one retention purge, per table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurgeReport {
    pub live: usize,
    pub one_minute: usize,
    pub five_minutes: usize,
    pub one_hour: usize,
}

impl PurgeReport {
    /// Total rows removed across all tables.
    pub fn total(&self) -> usize {
        self.live + self.one_minute + self.five_minutes + self.one_hour
    }
}

/// Handle to the metrics database.
pub struct MetricsDb {
    connection: Connection,
}

impl MetricsDb {
    /// Opens (or creates) the metrics database at `path` and applies pending
    /// schema migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let connection =
            Connection::open(path).with_context(|| format!("open metrics {}", path.display()))?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .context("configure metrics busy timeout")?;

        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .context("read metrics schema version")?;
        if version < 1 {
            connection
                .execute_batch(SCHEMA)
                .context("apply metrics schema")?;
            connection
                .pragma_update(None, "user_version", 1)
                .context("record metrics schema version")?;
        }

        Ok(Self { connection })
    }

    /// Stores one live sample for `host_id` at `timestamp_ms`.
    pub fn insert_live_sample(
        &self,
        host_id: &str,
        timestamp_ms: i64,
        sample: &LiveSample,
    ) -> Result<()> {
        samples::insert_live(&self.connection, host_id, timestamp_ms, sample)
    }

    /// Stores one 1-minute rollup for `host_id` at `timestamp_ms`.
    pub fn insert_1m_rollup(
        &self,
        host_id: &str,
        timestamp_ms: i64,
        rollup: &Rollup1m,
    ) -> Result<()> {
        rollups::insert_1m(&self.connection, host_id, timestamp_ms, rollup)
    }

    /// Stores one 5-minute rollup for `host_id` at `timestamp_ms`.
    pub fn insert_5m_rollup(
        &self,
        host_id: &str,
        timestamp_ms: i64,
        rollup: &Rollup5m,
    ) -> Result<()> {
        rollups::insert_5m(&self.connection, host_id, timestamp_ms, rollup)
    }

    /// Stores one 1-hour rollup for `host_id` at `timestamp_ms`.
    pub fn insert_1h_rollup(
        &self,
        host_id: &str,
        timestamp_ms: i64,
        rollup: &Rollup1h,
    ) -> Result<()> {
        rollups::insert_1h(&self.connection, host_id, timestamp_ms, rollup)
    }

    /// Deletes rows older than each table's retention window. `now_ms` is the
    /// current time in milliseconds since the Unix epoch.
    pub fn purge_expired(&self, now_ms: i64) -> Result<PurgeReport> {
        Ok(PurgeReport {
            one_minute: self.purge_table("metrics_1m", now_ms - MINUTE_RETENTION_MS)?,
            five_minutes: self.purge_table("metrics_5m", now_ms - FIVE_MINUTE_RETENTION_MS)?,
            one_hour: self.purge_table("metrics_1h", now_ms - HOUR_RETENTION_MS)?,
            live: self.purge_table("metrics_live", now_ms - LIVE_RETENTION_MS)?,
        })
    }

    /// Deletes rows of one table older than the cutoff. The table name comes
    /// from a call-site constant, never from user input.
    fn purge_table(&self, table: &str, cutoff_ms: i64) -> Result<usize> {
        let deleted = self
            .connection
            .execute(
                &format!("DELETE FROM {table} WHERE timestamp < ?1"),
                [cutoff_ms],
            )
            .with_context(|| format!("purge {table}"))?;
        Ok(deleted)
    }
}
