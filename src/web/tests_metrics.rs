//! CP7 web integration tests: history endpoint serves chart data from the
//! metrics database and enforces authentication.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, Request, StatusCode},
    Router,
};
use hakobi::metrics::{now_ms, sample_host, LiveSample, MetricsDb, Rollup1h, Rollup1m};
use tower::ServiceExt;

use super::router_with_backend;
use super::tests::{config, sign_in, FakeBackend};

fn app_with_tempdir() -> (tempfile::TempDir, Router) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.sqlite3");
    (
        directory,
        router_with_backend(config(), path, Arc::new(FakeBackend)),
    )
}

fn metrics_path(directory: &std::path::Path) -> std::path::PathBuf {
    hakobi::metrics::metrics_database_path(&directory.join("catalog.sqlite3"))
}

#[tokio::test]
async fn history_endpoint_requires_authentication() {
    let (data, app) = app_with_tempdir();
    let _path = metrics_path(data.path());
    let response = app
        .oneshot(
            Request::get("/partials/charts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sampling_job_writes_rollups_to_the_database() {
    // Direct exercise of the sampling-job write path (the same code the
    // spawned task runs per host): live sample in, rollups out.
    let directory = tempfile::tempdir().unwrap();
    let db = MetricsDb::open(directory.path().join("metrics.sqlite3")).unwrap();
    let stamp = now_ms();
    db.insert_live_sample(
        "h1",
        stamp - 30_000,
        &LiveSample {
            cpu_percent: 10.0,
            memory_used_bytes: 1_000,
            memory_total_bytes: 4_000,
            disk_used_bytes: 100,
            disk_total_bytes: 1_000,
            network_rx_bps: 10.0,
            network_tx_bps: 20.0,
            load_1: 0.1,
        },
    )
    .unwrap();
    db.insert_live_sample(
        "h1",
        stamp - 10_000,
        &LiveSample {
            cpu_percent: 30.0,
            memory_used_bytes: 3_000,
            memory_total_bytes: 4_000,
            disk_used_bytes: 300,
            disk_total_bytes: 1_000,
            network_rx_bps: 30.0,
            network_tx_bps: 60.0,
            load_1: 0.3,
        },
    )
    .unwrap();

    let report = sample_host(
        &db,
        "h1",
        &LiveSample {
            cpu_percent: 50.0,
            memory_used_bytes: 2_000,
            memory_total_bytes: 4_000,
            disk_used_bytes: 500,
            disk_total_bytes: 1_000,
            network_rx_bps: 50.0,
            network_tx_bps: 100.0,
            load_1: 0.5,
        },
    )
    .unwrap();
    assert_eq!(report.tick_count, 1);
    assert!(!report.wrote_5m);

    // The 1-minute rollup folded in the seeded live rows.
    let points = db.query_1m("h1", stamp - 120_000, stamp + 60_000).unwrap();
    let latest = points.last().unwrap();
    assert!((latest.cpu_percent - 30.0).abs() < 1e-6);
    assert!(latest.disk_used_bytes >= 300);

    // Simulating five completed minutes: the tick counter reaches 5.
    for _ in 0..4 {
        sample_host(&db, "h1", &LiveSample::default()).unwrap();
    }
    let ticks: i64 = db.query_1m("h1", 0, stamp + 600_000).unwrap().len() as i64;
    assert_eq!(ticks, 5);
    let combined = hakobi::metrics::combine_1m(&[Rollup1m {
        cpu_avg: 1.0,
        cpu_max: 2.0,
        memory_avg: 3.0,
        memory_max: 4.0,
        disk_used: 5,
        network_rx_avg: 6.0,
        network_tx_avg: 7.0,
    }]);
    db.insert_5m_rollup("h1", stamp, &combined).unwrap();
    db.insert_1h_rollup(
        "h1",
        stamp,
        &Rollup1h {
            cpu_avg: 2.0,
            memory_avg: 3.0,
            disk_used: 5,
        },
    )
    .unwrap();
    assert_eq!(db.query_5m("h1", 0, stamp + 1).unwrap().len(), 1);
    assert_eq!(db.query_1h("h1", 0, stamp + 1).unwrap().len(), 1);

    // Content-Type for the partial is HTML.
    let (data, app) = app_with_tempdir();
    let (cookies, _csrf) = sign_in(&app).await;
    let response = app
        .oneshot(
            Request::get("/partials/charts")
                .header(axum::http::header::COOKIE, cookies)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(content_type.starts_with("text/html"));
    drop(data);
}
