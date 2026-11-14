//! CP7 metrics tests: database open/close, insert/query round-trips, rollup
//! math, retention purge, and the sampling cadence.

use anyhow::Result;

static ENV_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

use crate::metrics::{
    combine_1m, combine_5m, metrics_database_path, now_ms, rollup_from_live, LiveSample, MetricsDb,
    Rollup1m, Rollup5m, ROLLUP_5M_EVERY,
};

fn open_temp_metrics() -> (tempfile::TempDir, MetricsDb) {
    let directory = tempfile::tempdir().unwrap();
    let metrics = MetricsDb::open(directory.path().join("metrics.sqlite3")).unwrap();
    (directory, metrics)
}

fn sample(cpu: f64, memory_used: u64, memory_total: u64) -> LiveSample {
    LiveSample {
        cpu_percent: cpu,
        memory_used_bytes: memory_used,
        memory_total_bytes: memory_total,
        disk_used_bytes: 40 * 1_048_576,
        disk_total_bytes: 400 * 1_048_576,
        network_rx_bps: 1_000.0,
        network_tx_bps: 500.0,
        load_1: 0.5,
    }
}

#[test]
fn database_opens_and_reopens() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("metrics.sqlite3");
    {
        let metrics = MetricsDb::open(&path).unwrap();
        metrics.purge_expired(now_ms()).unwrap();
    }
    // Reopening must succeed and keep the schema version.
    let metrics = MetricsDb::open(&path).unwrap();
    let version: i64 = metrics
        .connection()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
}

#[test]
fn metrics_database_path_sits_next_to_catalog() {
    let path = metrics_database_path(std::path::Path::new("/data/hakobi/catalog.sqlite3"));
    assert_eq!(
        path,
        std::path::PathBuf::from("/data/hakobi/metrics.sqlite3")
    );
}

#[test]
fn live_samples_round_trip() -> Result<()> {
    let (_data, metrics) = open_temp_metrics();
    metrics.insert_live_sample("h1", 1_000, &sample(12.5, 2_000, 4_000))?;
    metrics.insert_live_sample("h1", 2_000, &sample(50.0, 3_000, 4_000))?;
    metrics.insert_live_sample("h2", 2_000, &sample(1.0, 100, 200))?;

    let rows = metrics
        .connection()
        .prepare("SELECT cpu_percent, memory_used_bytes FROM metrics_live WHERE host_id = ?1 ORDER BY timestamp")?
        .query_map(["h1"], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    assert_eq!(rows, vec![(12.5, 2_000), (50.0, 3_000)]);
    Ok(())
}

#[test]
fn rollups_round_trip_for_all_tables() -> Result<()> {
    let (_data, metrics) = open_temp_metrics();
    let rollup_1m = Rollup1m {
        cpu_avg: 10.0,
        cpu_max: 20.0,
        memory_avg: 30.0,
        memory_max: 40.0,
        disk_used: 1_000,
        network_rx_avg: 5.0,
        network_tx_avg: 6.0,
    };
    metrics.insert_1m_rollup("h1", 1_000, &rollup_1m)?;
    let rollup_5m = combine_1m(&[rollup_1m]);
    metrics.insert_5m_rollup("h1", 5_000, &rollup_5m)?;
    let rollup_1h = combine_5m(&[rollup_5m]);
    metrics.insert_1h_rollup("h1", 3_600_000, &rollup_1h)?;

    let points_1m = metrics.query_1m("h1", 0, 2_000)?;
    assert_eq!(points_1m.len(), 1);
    assert_eq!(points_1m[0].cpu_percent, 10.0);
    assert_eq!(points_1m[0].memory_percent, 30.0);
    assert_eq!(points_1m[0].disk_used_bytes, 1_000);

    let points_5m = metrics.query_5m("h1", 0, 6_000)?;
    assert_eq!(points_5m.len(), 1);
    assert_eq!(points_5m[0].cpu_percent, 10.0);
    assert_eq!(points_5m[0].network_rx_bps, 5.0);

    let points_1h = metrics.query_1h("h1", 0, 3_700_000)?;
    assert_eq!(points_1h.len(), 1);
    assert_eq!(points_1h[0].cpu_percent, 10.0);
    assert_eq!(points_1h[0].disk_used_bytes, 1_000);

    // Time bounds exclude out-of-window rows and other hosts stay isolated.
    assert!(metrics.query_1m("h1", 2_001, 9_999)?.is_empty());
    assert!(metrics.query_1m("h2", 0, 9_999)?.is_empty());
    Ok(())
}

#[test]
fn rollup_calculation_is_correct() {
    assert_live_rollup_calculation();
    assert_five_minute_rollup_calculation();
    assert_hourly_rollup_calculation();

    // Empty windows stay zero-valued instead of dividing by zero.
    assert_eq!(rollup_from_live(&[]), Rollup1m::default());
}

fn assert_live_rollup_calculation() {
    let samples = vec![
        sample(10.0, 1_000, 2_000),
        sample(30.0, 3_000, 4_000),
        sample(20.0, 2_000, 2_000),
    ];
    let rollup = rollup_from_live(&samples);
    assert!((rollup.cpu_avg - 20.0).abs() < 1e-9);
    assert!((rollup.cpu_max - 30.0).abs() < 1e-9);
    // Memory percentages: 50%, 75%, 100% => average 75%, max 100%.
    assert!((rollup.memory_avg - 75.0).abs() < 1e-9);
    assert!((rollup.memory_max - 100.0).abs() < 1e-9);
    assert_eq!(rollup.disk_used, 40 * 1_048_576);
    assert!((rollup.network_rx_avg - 1_000.0).abs() < 1e-9);
    assert!((rollup.network_tx_avg - 500.0).abs() < 1e-9);
}

fn assert_five_minute_rollup_calculation() {
    let rollup_5m = combine_1m(&[
        Rollup1m {
            cpu_avg: 0.0,
            cpu_max: 10.0,
            memory_avg: 0.0,
            memory_max: 10.0,
            disk_used: 100,
            network_rx_avg: 0.0,
            network_tx_avg: 0.0,
        },
        Rollup1m {
            cpu_avg: 20.0,
            cpu_max: 30.0,
            memory_avg: 20.0,
            memory_max: 30.0,
            disk_used: 300,
            network_rx_avg: 10.0,
            network_tx_avg: 20.0,
        },
    ]);
    assert!((rollup_5m.cpu_avg - 10.0).abs() < 1e-9);
    assert!((rollup_5m.cpu_max - 30.0).abs() < 1e-9);
    assert!((rollup_5m.memory_avg - 10.0).abs() < 1e-9);
    assert_eq!(rollup_5m.disk_used, 300);
}

fn assert_hourly_rollup_calculation() {
    let rollup_1h = combine_5m(&[
        Rollup5m {
            cpu_avg: 0.0,
            cpu_max: 0.0,
            memory_avg: 40.0,
            disk_used: 100,
            network_rx_avg: 0.0,
            network_tx_avg: 0.0,
        },
        Rollup5m {
            cpu_avg: 10.0,
            cpu_max: 10.0,
            memory_avg: 60.0,
            disk_used: 900,
            network_rx_avg: 0.0,
            network_tx_avg: 0.0,
        },
    ]);
    assert!((rollup_1h.cpu_avg - 5.0).abs() < 1e-9);
    assert!((rollup_1h.memory_avg - 50.0).abs() < 1e-9);
    assert_eq!(rollup_1h.disk_used, 900);
}

#[test]
fn retention_purge_removes_expired_rows_only() -> Result<()> {
    let (_data, metrics) = open_temp_metrics();
    let now = 100 * 86_400_000; // 100 days in ms

    metrics.insert_1m_rollup("h1", now - 8 * 86_400_000, &Rollup1m::default())?;
    metrics.insert_1m_rollup("h1", now - 6 * 86_400_000, &Rollup1m::default())?;
    metrics.insert_5m_rollup("h1", now - 31 * 86_400_000, &Rollup5m::default())?;
    metrics.insert_5m_rollup("h1", now - 29 * 86_400_000, &Rollup5m::default())?;
    metrics.insert_live_sample("h1", now - 400_000, &LiveSample::default())?;
    metrics.insert_live_sample("h1", now - 100_000, &LiveSample::default())?;

    let report = metrics.purge_expired(now)?;
    assert_eq!(report.one_minute, 1);
    assert_eq!(report.five_minutes, 1);
    assert_eq!(report.one_hour, 0);
    assert_eq!(report.live, 1);
    assert_eq!(report.total(), 3);

    assert_eq!(metrics.query_1m("h1", 0, now)?.len(), 1);
    assert_eq!(metrics.query_5m("h1", 0, now)?.len(), 1);
    let live_left: i64 =
        metrics
            .connection()
            .query_row("SELECT COUNT(*) FROM metrics_live", [], |row| row.get(0))?;
    assert_eq!(live_left, 1);
    Ok(())
}

#[test]
fn rollup_cadence_matches_prd() -> Result<()> {
    use crate::metrics::advance_rollups;

    let (_data, metrics) = open_temp_metrics();
    let connection = metrics.connection();

    // Ticks 1-4: only the counter advances.
    for tick in 1..5 {
        let report = advance_rollups(connection, "h1", 5)?;
        assert_eq!(report.tick_count, tick);
        assert_eq!(report.wrote_5m, tick % ROLLUP_5M_EVERY == 0);
        assert!(!report.wrote_1h);
    }
    // Tick 5 writes a 5-minute rollup.
    let report = advance_rollups(connection, "h1", 5)?;
    assert_eq!(report.tick_count, 5);
    assert!(report.wrote_5m);
    assert!(!report.wrote_1h);

    // Tick 60 writes both.
    for _ in 6..60 {
        advance_rollups(connection, "h1", 5)?;
    }
    let report = advance_rollups(connection, "h1", 5)?;
    assert_eq!(report.tick_count, 60);
    assert!(report.wrote_5m);
    assert!(report.wrote_1h);

    // A failed sample (count 0) neither advances the counter nor triggers rollups.
    let report = advance_rollups(connection, "h1", 0)?;
    assert_eq!(report.tick_count, 60);
    assert!(!report.wrote_5m);
    Ok(())
}

#[test]
fn sample_host_writes_1m_rollup_from_live_window() -> Result<()> {
    use crate::metrics::sample_host;

    let (_data, metrics) = open_temp_metrics();
    // Seed two live samples a few seconds apart, then sample at a fixed instant.
    let now = now_ms();
    metrics.insert_live_sample("h1", now - 30_000, &sample(10.0, 1_000, 4_000))?;
    metrics.insert_live_sample("h1", now - 10_000, &sample(30.0, 3_000, 4_000))?;

    // sample_host stamps the rollup with the current time, so query a wide window.
    let report = sample_host(&metrics, "h1", &sample(20.0, 2_000, 4_000))?;
    assert_eq!(report.tick_count, 1);
    assert!(!report.wrote_5m);

    let points = metrics.query_1m("h1", now - 60_000, now + 60_000)?;
    assert!(!points.is_empty());
    let latest = points.last().unwrap();
    // The seeded samples average in: cpu (10+30+20)/3 = 20.
    assert!((latest.cpu_percent - 20.0).abs() < 1e-6);
    Ok(())
}

#[tokio::test]
async fn spawn_retention_job_is_created_and_cancelled_cleanly() {
    let directory = tempfile::tempdir().unwrap();
    let handle = crate::metrics::spawn_retention_job(directory.path().join("catalog.sqlite3"));
    handle.abort();
    let outcome = handle.await;
    assert!(outcome.is_err()); // aborted
}
#[tokio::test]
async fn local_agent_is_included_in_sampling_targets() {
    let _guard = ENV_LOCK.lock().await;
    // The app's own agent (env-configured) is sampled as host `local` even
    // with no registered agents, so the machine Hakobi runs on reports
    // dashboard metrics.
    let directory = tempfile::tempdir().unwrap();
    std::env::set_var("HAKOBI_AGENT_URL", "http://127.0.0.1:1");
    std::env::set_var("HAKOBI_AGENT_TOKEN", "0123456789abcdef0123456789abcdef");
    let targets =
        crate::metrics::sampler::list_enabled_hosts(&directory.path().join("catalog.sqlite3"))
            .await
            .unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].0, "local");
    std::env::remove_var("HAKOBI_AGENT_URL");
    std::env::remove_var("HAKOBI_AGENT_TOKEN");
}

#[tokio::test]
async fn local_agent_environment_supplies_missing_catalog_token() {
    let _guard = ENV_LOCK.lock().await;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.sqlite3");
    let mut catalog = crate::catalog::Catalog::open(&path).unwrap();
    let id = catalog
        .create_host_agent("local", "http://127.0.0.1:8787", "temporary")
        .unwrap();
    drop(catalog);
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE host_agents SET encrypted_token = NULL WHERE id = ?1",
            rusqlite::params![id],
        )
        .unwrap();
    std::env::set_var("HAKOBI_AGENT_URL", "http://127.0.0.1:8787");
    std::env::set_var("HAKOBI_AGENT_TOKEN", "0123456789abcdef0123456789abcdef");
    let targets = crate::metrics::sampler::list_enabled_hosts(&path)
        .await
        .unwrap();
    std::env::remove_var("HAKOBI_AGENT_URL");
    std::env::remove_var("HAKOBI_AGENT_TOKEN");
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].0, id);
    assert_eq!(targets[0].2, "0123456789abcdef0123456789abcdef");
}

#[test]
fn disk_and_network_map_to_correct_columns() -> Result<()> {
    // Regression: map_rollup_row used positional indexes while the 1m table
    // interleaves cpu_max/memory_max, so disk and network silently read the
    // wrong columns and always rendered zero on the dashboard.
    let (_data, metrics) = open_temp_metrics();
    let rollup = Rollup1m {
        cpu_avg: 10.0,
        cpu_max: 20.0,
        memory_avg: 30.0,
        memory_max: 40.0,
        disk_used: 2_050_000_000_000,
        network_rx_avg: 900_000.0,
        network_tx_avg: 450_000.0,
    };
    metrics.insert_1m_rollup("local", 1_000, &rollup)?;
    let point = &metrics.query_1m("local", 0, 2_000)?[0];
    assert_eq!(point.disk_used_bytes, 2_050_000_000_000);
    assert!((point.network_rx_bps - 900_000.0).abs() < 1e-9);
    assert!((point.network_tx_bps - 450_000.0).abs() < 1e-9);
    assert!((point.cpu_percent - 10.0).abs() < 1e-9);
    assert!((point.memory_percent - 30.0).abs() < 1e-9);
    Ok(())
}
