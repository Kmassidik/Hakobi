//! Overview dashboard handler and metrics collection.

use std::collections::HashSet;

use anyhow::Context as _;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::{authenticated, html_response, load_providers, page_csrf, ProviderNotice};
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
    let (readiness, agent_online) = match load_readiness(agent.as_ref()).await {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(error = %error, "readiness detection failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let metrics = collect_dashboard_metrics(&state).await;
    let install_running = if agent_online && notice.notice.is_some() {
        match &agent {
            Some(agent) => agent
                .install_status()
                .await
                .map(|jobs| jobs.values().any(|job| job.running))
                .unwrap_or(false),
            None => false,
        }
    } else {
        false
    };
    let markup = templates::dashboard(templates::DashboardView {
        base_url: &state.config.base_url,
        csrf_token: &csrf_token,
        tunnel_enabled: state.config.tunnel_enabled,
        username: &state.config.username,
        readiness: &readiness,
        metrics: &metrics,
        notice: notice.notice.as_deref(),
        agent_configured,
        agent_online,
        install_running,
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
