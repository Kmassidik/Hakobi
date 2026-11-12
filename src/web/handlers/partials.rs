//! HTMX partial handlers: metrics, readiness, and status badge.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::dashboard::{collect_dashboard_metrics, load_readiness};
use super::{authenticated, html_response, page_csrf};
use crate::web::{agent_client::HostAgent, templates, AppState};

pub(crate) fn partial_unauthorized() -> Response {
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    response.headers_mut().insert(
        "hx-redirect",
        axum::http::HeaderValue::from_static("/login"),
    );
    response
}

pub(crate) async fn metrics_partial(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers).await {
        return partial_unauthorized();
    }
    let metrics = collect_dashboard_metrics(&state).await;
    let history_path = (*state.catalog_path).clone();
    let live_cache = std::sync::Arc::clone(&state.live_cache);
    let live = tokio::task::spawn_blocking(move || {
        live_cache
            .lock()
            .map(|mut cache| cache.get_or_load(&history_path))
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    html_response(
        StatusCode::OK,
        templates::metrics_section(&live, &metrics),
        Vec::new(),
    )
}

/// Renders the four time-series chart panels with fresh rollup data.
pub(crate) async fn charts_partial(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers).await {
        return partial_unauthorized();
    }
    let metrics_path = hakobi::metrics::metrics_database_path(&state.catalog_path);
    let points = tokio::task::spawn_blocking(move || -> Vec<hakobi::metrics::HistoryPoint> {
        let now = hakobi::metrics::now_ms();
        let from = now - 24 * 60 * 60_000;
        hakobi::metrics::MetricsDb::open(metrics_path)
            .map(|db| super::dashboard::aggregate_all_hosts(&db, from, now).unwrap_or_default())
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    html_response(
        StatusCode::OK,
        templates::chart_section(&points),
        Vec::new(),
    )
}

pub(crate) async fn readiness_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !authenticated(&state, &headers).await {
        return partial_unauthorized();
    }
    let agent = HostAgent::from_env();
    let agent_configured = agent.is_some();
    let (readiness, agent_online) = match load_readiness(agent.as_ref()).await {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(error = %error, "readiness detection failed");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::readiness_section(&readiness, &csrf_token, agent_online, agent_configured),
        cookies,
    )
}

pub(crate) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let session = crate::web::auth::cookie_value(&headers, state.config.session_cookie_name());
    if !state.sessions.is_valid(session).await {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response.headers_mut().insert(
            "hx-redirect",
            axum::http::HeaderValue::from_static("/login"),
        );
        return response;
    }
    html_response(
        StatusCode::OK,
        templates::status_badge("Control plane online", true),
        Vec::new(),
    )
}
