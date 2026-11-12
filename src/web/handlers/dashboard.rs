//! Overview dashboard handler and metrics collection.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context as _;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::{
    authenticated, html_response, load_providers, page_csrf, with_catalog, ProviderNotice,
};
use crate::web::{agent_client::HostAgent, templates, AppState};
use hakobi::providers::ContainerRow;

pub(crate) async fn dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(notice): Query<ProviderNotice>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }

    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let agent = HostAgent::from_env();
    let agent_configured = agent.is_some();
    let (_, agent_online) = match load_readiness(agent.as_ref()).await {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(error = %error, "readiness detection failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let metrics = collect_dashboard_metrics(&state).await;
    let (history_points, live, activity) = load_dashboard_extras(&state).await;
    let activity_views: Vec<templates::ActivityRow<'_>> = activity
        .iter()
        .map(|item| templates::ActivityRow {
            deployment: &item.deployment,
            app_name: &item.app_name,
        })
        .collect();
    let markup = templates::dashboard(templates::DashboardView {
        username: &state.config.username,
        metrics: &metrics,
        notice: notice.notice.as_deref(),
        agent_configured,
        agent_online,
        history_points: &history_points,
        live: &live,
        activity: &activity_views,
        csrf_token: &csrf_token,
    });
    html_response(StatusCode::OK, markup, cookies)
}

pub(crate) async fn load_readiness(
    agent: Option<&HostAgent>,
) -> anyhow::Result<(hakobi::install::Readiness, bool)> {
    if let Some(agent) = agent {
        match agent.readiness().await {
            Ok(readiness) => return Ok((readiness, true)),
            Err(error) => tracing::warn!(error = %error, "host agent readiness failed"),
        }
    }
    let readiness = tokio::task::spawn_blocking(hakobi::install::detect)
        .await
        .context("readiness detection task failed")?;
    Ok((readiness, false))
}

pub(crate) async fn collect_dashboard_metrics(state: &AppState) -> templates::DashboardMetrics {
    let mut metrics = templates::DashboardMetrics::default();
    let Ok(providers) = load_providers(state).await else {
        metrics.catalog_failed = true;
        tracing::warn!("dashboard provider list failed");
        return metrics;
    };
    let mut services = HashSet::new();
    let mut running_services = HashSet::new();
    let mut stacks = HashSet::new();
    for provider in providers.into_iter().filter(|provider| provider.enabled) {
        metrics.providers_configured += 1;
        let rows = match state.backend.containers(&provider.endpoint).await {
            Ok(rows) => {
                metrics.providers_reachable += 1;
                rows
            }
            Err(error) => {
                metrics.providers_failed += 1;
                tracing::warn!(
                    provider = %provider.name,
                    error = %error,
                    "dashboard provider inventory failed"
                );
                continue;
            }
        };
        account_rows(
            &provider.id,
            rows,
            &mut metrics,
            &mut services,
            &mut running_services,
            &mut stacks,
        );
    }
    metrics.services_total = services.len();
    metrics.services_running = running_services.len();
    metrics.stacks = stacks.len();
    metrics
}

fn account_rows(
    provider_id: &str,
    rows: Vec<ContainerRow>,
    metrics: &mut templates::DashboardMetrics,
    services: &mut HashSet<String>,
    running_services: &mut HashSet<String>,
    stacks: &mut HashSet<String>,
) {
    for row in rows {
        let running = row.state == "running";
        metrics.containers_total += 1;
        if running {
            metrics.containers_running += 1;
            if row.health.as_deref() != Some("unhealthy") {
                metrics.containers_healthy += 1;
            }
        }
        if row.state == "restarting" {
            metrics.containers_restarting += 1;
        }
        if let Some(project) = row
            .compose_project
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            stacks.insert(format!("{provider_id}/{project}"));
        }
        let service = match (
            row.compose_project.as_deref(),
            row.compose_service.as_deref(),
        ) {
            (Some(project), Some(service)) => format!("{provider_id}/{project}/{service}"),
            _ => format!("{provider_id}/{}", row.name),
        };
        services.insert(service.clone());
        if running {
            running_services.insert(service);
        }
    }
}

/// Owned activity row so the view can borrow without lifetime knots.
struct ActivityItem {
    app_name: String,
    deployment: hakobi::catalog::Deployment,
}

/// Loads 24h aggregate history (averaged across hosts) and recent deployments.
async fn load_dashboard_extras(
    state: &AppState,
) -> (
    Vec<hakobi::metrics::HistoryPoint>,
    Vec<hakobi::metrics::LatestLive>,
    Vec<ActivityItem>,
) {
    let history_path = (*state.catalog_path).clone();
    let live_cache = Arc::clone(&state.live_cache);
    let (history, live) = tokio::task::spawn_blocking(move || -> (Vec<hakobi::metrics::HistoryPoint>, Vec<hakobi::metrics::LatestLive>) {
        let now = hakobi::metrics::now_ms();
        let from = now - 24 * 60 * 60_000;
        let live = live_cache.lock().map(|mut cache| cache.get_or_load(&history_path)).unwrap_or_default();
        let history = hakobi::metrics::MetricsDb::open(hakobi::metrics::metrics_database_path(&history_path))
            .map(|db| aggregate_all_hosts(&db, from, now).unwrap_or_default())
            .unwrap_or_default();
        (history, live)
    })
    .await
    .unwrap_or_default();

    let activity = with_catalog(state, |catalog| {
        let deployments = catalog.list_recent_deployments(10)?;
        let apps = catalog.list_apps()?;
        let names: std::collections::HashMap<&str, &str> = apps
            .iter()
            .map(|app| (app.id.as_str(), app.name.as_str()))
            .collect();
        Ok(deployments
            .into_iter()
            .map(|deployment| ActivityItem {
                app_name: names
                    .get(deployment.app_id.as_str())
                    .map(|name| (*name).to_owned())
                    .unwrap_or_else(|| "deleted".to_owned()),
                deployment,
            })
            .collect())
    })
    .await
    .unwrap_or_default();
    (history, live, activity)
}

/// Averages per-host rollups into one series across all hosts.
/// Per-timestamp accumulator for cross-host aggregation.
#[derive(Default)]
struct AggregateEntry {
    cpu: f64,
    memory: f64,
    count: usize,
    disk_used: u64,
    network_rx: f64,
    network_tx: f64,
}

pub(crate) fn aggregate_all_hosts(
    db: &hakobi::metrics::MetricsDb,
    from: i64,
    to: i64,
) -> anyhow::Result<Vec<hakobi::metrics::HistoryPoint>> {
    let hosts = db.host_ids_with_data(from, to)?;
    if hosts.is_empty() {
        return Ok(Vec::new());
    }
    let per_host = hosts
        .iter()
        .map(|host| db.query_1m(host, from, to))
        .collect::<Result<Vec<_>, _>>()?;
    // Group by timestamp: CPU/memory average across hosts, disk is the max
    // (usage snapshot per host), network sums (traffic aggregates).
    let mut buckets: std::collections::BTreeMap<i64, AggregateEntry> = Default::default();
    for points in &per_host {
        for point in points {
            let entry = buckets.entry(point.timestamp).or_default();
            entry.cpu += point.cpu_percent;
            entry.memory += point.memory_percent;
            entry.count += 1;
            entry.disk_used = entry.disk_used.max(point.disk_used_bytes);
            entry.network_rx += point.network_rx_bps;
            entry.network_tx += point.network_tx_bps;
        }
    }
    Ok(buckets
        .into_iter()
        .map(|(timestamp, entry)| {
            let count = entry.count as f64;
            hakobi::metrics::HistoryPoint {
                timestamp,
                cpu_percent: entry.cpu / count,
                memory_percent: entry.memory / count,
                disk_used_bytes: entry.disk_used,
                disk_total_bytes: 0,
                network_rx_bps: entry.network_rx,
                network_tx_bps: entry.network_tx,
            }
        })
        .collect())
}

/// Redirects the legacy `/` bookmark to `/dashboard`.
pub(crate) async fn dashboard_redirect() -> Response {
    axum::response::Redirect::to("/dashboard").into_response()
}
