//! Background sampling job: polls every enabled host agent once a minute,
//! writes live samples and rollups, and drives the daily retention purge.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use rusqlite::params;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{info, warn};

use crate::metrics::{
    combine_1m, combine_5m, metrics_database_path, now_ms, rollup_from_live, LiveSample, MetricsDb,
    ROLLUP_1H_EVERY, ROLLUP_5M_EVERY,
};

/// Sampling cadence for the background job: 60 seconds.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(60);
/// Retention purge cadence: once a day.
const PURGE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Per-host sampler bookkeeping stored in `sampler_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplingReport {
    pub tick_count: i64,
    pub wrote_5m: bool,
    pub wrote_1h: bool,
}

/// Loads or initializes the per-host tick counter.
fn load_tick(connection: &rusqlite::Connection, host_id: &str) -> Result<i64> {
    let tick = connection
        .query_row(
            "SELECT tick_count FROM sampler_state WHERE host_id = ?1",
            params![host_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    Ok(tick)
}

/// Stores the per-host tick counter.
fn save_tick(connection: &rusqlite::Connection, host_id: &str, tick: i64) -> Result<()> {
    connection.execute(
        "INSERT INTO sampler_state (host_id, tick_count, updated_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(host_id) DO UPDATE SET tick_count = ?2, updated_at = ?3",
        params![host_id, tick, now_ms()],
    )?;
    Ok(())
}

/// Advances the per-host tick counter and derives which rollups to write.
///
/// The tick counter increments every minute. A 5-minute rollup is written when
/// the counter hits a multiple of [`ROLLUP_5M_EVERY`]; a 1-hour rollup when it
/// hits a multiple of [`ROLLUP_1H_EVERY`].
pub fn advance_rollups(
    connection: &rusqlite::Connection,
    host_id: &str,
    sample_count: usize,
) -> Result<SamplingReport> {
    let previous = load_tick(connection, host_id)?;
    let tick = if sample_count > 0 {
        previous + 1
    } else {
        previous
    };
    save_tick(connection, host_id, tick)?;
    Ok(SamplingReport {
        tick_count: tick,
        wrote_5m: sample_count > 0 && tick % ROLLUP_5M_EVERY == 0,
        wrote_1h: sample_count > 0 && tick % ROLLUP_1H_EVERY == 0,
    })
}

/// Runs one sampling tick against one host: stores the live sample, computes
/// the 1-minute rollup from the last minute of live rows, and writes the
/// cadenced 5-minute and 1-hour rollups.
pub fn sample_host(
    metrics: &MetricsDb,
    host_id: &str,
    sample: &LiveSample,
) -> Result<SamplingReport> {
    let timestamp = now_ms();
    metrics.insert_live_sample(host_id, timestamp, sample)?;

    let connection = metrics.connection();
    let minute_ago = timestamp - crate::metrics::LIVE_RETENTION_MS + 60_000;
    let samples = load_live_window(connection, host_id, minute_ago, timestamp)?;

    let rollup_1m = rollup_from_live(&samples);
    metrics.insert_1m_rollup(host_id, timestamp, &rollup_1m)?;

    let report = advance_rollups(connection, host_id, samples.len())?;
    if report.wrote_5m {
        let window = load_1m_window(connection, host_id, timestamp)?;
        metrics.insert_5m_rollup(host_id, timestamp, &combine_1m(&window))?;
    }
    if report.wrote_1h {
        let window = load_5m_window(connection, host_id, timestamp)?;
        metrics.insert_1h_rollup(host_id, timestamp, &combine_5m(&window))?;
    }
    Ok(report)
}

/// Loads live samples for one host within `[from, to]`.
fn load_live_window(
    connection: &rusqlite::Connection,
    host_id: &str,
    from: i64,
    to: i64,
) -> Result<Vec<LiveSample>> {
    let rows = connection
        .prepare(
            "SELECT cpu_percent, memory_used_bytes, memory_total_bytes, disk_used_bytes, \
                    disk_total_bytes, network_rx_bps, network_tx_bps, load_1 \
             FROM metrics_live WHERE host_id = ?1 AND timestamp BETWEEN ?2 AND ?3",
        )?
        .query_map(params![host_id, from, to], |row| {
            Ok(LiveSample {
                cpu_percent: row.get(0)?,
                memory_used_bytes: row.get::<_, i64>(1)?.max(0) as u64,
                memory_total_bytes: row.get::<_, i64>(2)?.max(0) as u64,
                disk_used_bytes: row.get::<_, i64>(3)?.max(0) as u64,
                disk_total_bytes: row.get::<_, i64>(4)?.max(0) as u64,
                network_rx_bps: row.get(5)?,
                network_tx_bps: row.get(6)?,
                load_1: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Loads 1-minute rollups within the last five minutes, for the 5-minute rollup.
fn load_1m_window(
    connection: &rusqlite::Connection,
    host_id: &str,
    timestamp: i64,
) -> Result<Vec<crate::metrics::Rollup1m>> {
    let rows = connection
        .prepare(
            "SELECT cpu_avg, cpu_max, memory_avg, memory_max, disk_used, network_rx_avg, network_tx_avg \
             FROM metrics_1m WHERE host_id = ?1 AND timestamp BETWEEN ?2 AND ?3",
        )?
        .query_map(
            params![host_id, timestamp - 5 * 60_000, timestamp],
            |row| {
                Ok(crate::metrics::Rollup1m {
                    cpu_avg: row.get(0)?,
                    cpu_max: row.get(1)?,
                    memory_avg: row.get(2)?,
                    memory_max: row.get(3)?,
                    disk_used: row.get::<_, i64>(4)?.max(0) as u64,
                    network_rx_avg: row.get(5)?,
                    network_tx_avg: row.get(6)?,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Loads 5-minute rollups within the last hour, for the 1-hour rollup.
fn load_5m_window(
    connection: &rusqlite::Connection,
    host_id: &str,
    timestamp: i64,
) -> Result<Vec<crate::metrics::Rollup5m>> {
    let rows = connection
        .prepare(
            "SELECT cpu_avg, cpu_max, memory_avg, disk_used, network_rx_avg, network_tx_avg \
             FROM metrics_5m WHERE host_id = ?1 AND timestamp BETWEEN ?2 AND ?3",
        )?
        .query_map(
            params![host_id, timestamp - 60 * 60_000, timestamp],
            |row| {
                Ok(crate::metrics::Rollup5m {
                    cpu_avg: row.get(0)?,
                    cpu_max: row.get(1)?,
                    memory_avg: row.get(2)?,
                    disk_used: row.get::<_, i64>(3)?.max(0) as u64,
                    network_rx_avg: row.get(4)?,
                    network_tx_avg: row.get(5)?,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Internal access for the sampler and tests.
impl MetricsDb {
    pub(crate) fn connection(&self) -> &rusqlite::Connection {
        &self.connection
    }
}

/// Spawns the background sampler: every 60 seconds it polls each enabled host
/// agent's `/v1/system/metrics` and records the results. Offline agents are
/// skipped with a warning.
pub fn spawn_sampler(catalog_path: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(SAMPLE_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await; // skip the immediate first tick
        loop {
            ticker.tick().await;
            if let Err(error) = sampling_tick(&catalog_path).await {
                warn!(error = %error, "metrics sampling tick failed");
            }
        }
    })
}

/// One full sampling pass over every enabled host in the catalog.
async fn sampling_tick(catalog_path: &std::path::Path) -> Result<()> {
    let hosts = list_enabled_hosts(catalog_path).await?;
    if hosts.is_empty() {
        return Ok(());
    }
    let metrics_path = metrics_database_path(catalog_path);
    for host in hosts {
        let sample = match fetch_sample(&host).await {
            Ok(sample) => sample,
            Err(error) => {
                warn!(
                    host = %host.0,
                    error = %error,
                    "host agent offline; skipping metrics sample"
                );
                continue;
            }
        };
        match tokio::task::spawn_blocking({
            let metrics_path = metrics_path.clone();
            let host_id = host.0.clone();
            move || {
                let metrics = MetricsDb::open(&metrics_path)?;
                sample_host(&metrics, &host_id, &sample)
            }
        })
        .await
        {
            Ok(Ok(report)) => {
                info!(host = %host.0, tick = report.tick_count, "sampled host metrics");
            }
            Ok(Err(error)) => warn!(host = %host.0, error = %error, "metrics sample failed"),
            Err(error) => warn!(host = %host.0, error = %error, "metrics sample task panicked"),
        }
    }
    Ok(())
}

/// Enabled host agents from the catalog as `(id, url, token)` triples.
///
/// The app's own host agent (configured via `HAKOBI_AGENT_URL` /
/// `HAKOBI_AGENT_TOKEN`) is always included as host id `local` so the
/// machine Hakobi runs on is sampled even with no registered agents.
pub(super) async fn list_enabled_hosts(
    catalog_path: &std::path::Path,
) -> Result<Vec<(String, String, String)>> {
    let path = catalog_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let catalog = crate::catalog::Catalog::open(&path)?;
        let hosts = catalog.list_host_agents()?;
        let mut resolved = Vec::new();
        for host in hosts.into_iter().filter(|host| host.enabled) {
            let token = catalog.host_agent_token(&host.id)?;
            resolved.push((host.id, host.agent_url, token));
        }
        if let Some(endpoint) = crate::host_agent::AgentEndpoint::from_env() {
            let already = resolved.iter().any(|(_, url, _)| url == endpoint.url());
            if !already {
                resolved.push((
                    "local".to_owned(),
                    endpoint.url().to_owned(),
                    endpoint.token().to_owned(),
                ));
            }
        }
        Ok(resolved)
    })
    .await?
}

/// Fetches one live sample from a host agent.
async fn fetch_sample(host: &(String, String, String)) -> Result<super::LiveSample> {
    let endpoint = crate::host_agent::AgentEndpoint::new(host.1.clone(), host.2.clone())
        .ok_or_else(|| anyhow::anyhow!("invalid agent endpoint"))?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()?;
    let response = client
        .get(format!("{}/v1/system/metrics", endpoint.url()))
        .bearer_auth(endpoint.token())
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("host agent returned {status}");
    }
    let system: crate::host_agent::SystemMetrics = response.json().await?;
    Ok(super::LiveSample::from_system(&system))
}

/// Spawns the daily retention purge job.
pub fn spawn_retention_job(catalog_path: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let metrics_path = metrics_database_path(&catalog_path);
        let mut ticker = interval(PURGE_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let path = metrics_path.clone();
            match tokio::task::spawn_blocking(move || {
                let metrics = MetricsDb::open(&path)?;
                metrics.purge_expired(now_ms())
            })
            .await
            {
                Ok(Ok(report)) if report.total() > 0 => {
                    info!(removed = report.total(), "metrics retention purge");
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => warn!(error = %error, "metrics retention purge failed"),
                Err(error) => warn!(error = %error, "metrics purge task panicked"),
            }
        }
    })
}
