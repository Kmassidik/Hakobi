//! Host agent handlers: list, register, resources, test, toggle, delete.

use axum::{
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{
    html_response, internal_error, page_csrf, post_is_allowed, redirect_response, truncate,
    with_catalog, CsrfForm,
};
use crate::web::{agent_client::HostAgent, templates, AppState};
use hakobi::host_agent::{AgentEndpoint, AgentHost};
use hakobi::metrics;

/// History time-window query parameter.
#[derive(Debug, Deserialize)]
pub struct HistoryParams {
    window: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HostsCreateForm {
    name: String,
    agent_url: String,
    token: String,
    #[serde(default)]
    csrf_token: String,
}

pub(crate) async fn hosts_list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let hosts = match with_catalog(&state, |catalog| catalog.list_host_agents()).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let markup = templates::hosts(
        &state.config.base_url,
        &state.config.username,
        &page_csrf(&state, &headers).0,
        &hosts,
    );
    html_response(StatusCode::OK, markup, Vec::new())
}

pub(crate) async fn hosts_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<HostsCreateForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let name = form.name.trim();
    if name.is_empty() || name.len() > 64 {
        return (StatusCode::BAD_REQUEST, "name must be 1–64 characters").into_response();
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return (
            StatusCode::BAD_REQUEST,
            "name must contain only ASCII letters, digits, '-', '_', or '.'",
        )
            .into_response();
    }
    if form.agent_url.len() > 512 {
        return (StatusCode::BAD_REQUEST, "agent URL is too long").into_response();
    }
    if form.token.len() > 4096 {
        return (StatusCode::BAD_REQUEST, "token is too long").into_response();
    }
    let name = name.to_owned();
    let agent_url = form.agent_url.trim().to_owned();
    let token = form.token.trim().to_owned();
    if AgentEndpoint::new(agent_url.clone(), token.clone()).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            "agent URL and bearer token are invalid",
        )
            .into_response();
    }

    let result = with_catalog(&state, move |catalog| {
        catalog.create_host_agent_with_provider(&name, &agent_url, &token)
    })
    .await;
    match result {
        Ok(_host_id) => super::redirect_with_notice("/hosts", "Host registered."),
        Err(error) => internal_error(error),
    }
}

pub(crate) async fn resolve_host_agent(
    state: &AppState,
    id: &str,
) -> std::result::Result<(String, HostAgent), StatusCode> {
    if id == "local" {
        let agent = HostAgent::from_env().ok_or(StatusCode::NOT_FOUND)?;
        return Ok(("Local host".to_owned(), agent));
    }
    let hosts = with_catalog(state, |catalog| catalog.list_host_agents())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let host = hosts
        .into_iter()
        .find(|host| host.id == id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let host_id = host.id.clone();
    let token = with_catalog(state, move |catalog| catalog.host_agent_token(&host_id))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let agent_host = AgentHost::new(host.id.clone(), host.agent_url, token)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let agent = HostAgent::from_host(agent_host).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((host.name, agent))
}

async fn host_endpoint(state: &AppState, id: &str) -> Option<(String, HostAgent, String)> {
    let (label, agent) = resolve_host_agent(state, id).await.ok()?;
    let endpoint = if id == "local" {
        "agent://local".to_owned()
    } else {
        format!("agent://{id}")
    };
    Some((label, agent, endpoint))
}

pub(crate) async fn host_resources(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let Some((label, agent, endpoint)) = host_endpoint(&state, &id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let system = match agent.system_metrics().await {
        Ok(system) => system,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!(
                    "Host agent metrics failed: {}",
                    truncate(&error.to_string(), 300)
                ),
            )
                .into_response();
        }
    };
    let containers = state
        .backend
        .containers(&endpoint)
        .await
        .unwrap_or_default();
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let live_url = format!("/hosts/{id}/resources/partial");
    let history_host_id = if id == "local" { "local" } else { id.as_str() };
    let history_markup =
        load_history_markup(&state, history_host_id, templates::HistoryWindow::OneHour).await;
    let markup = templates::host_resources(
        &state.config.base_url,
        &state.config.username,
        &label,
        &csrf_token,
        &system,
        &containers,
        &live_url,
        history_markup,
    );
    html_response(StatusCode::OK, markup, cookies)
}

/// Loads the default (1h) history panel markup; an empty or unavailable
/// metrics database renders the panel with "No data yet" charts.
async fn load_history_markup(
    state: &AppState,
    host_id: &str,
    window: templates::HistoryWindow,
) -> maud::Markup {
    let metrics_path = metrics::metrics_database_path(&state.catalog_path);
    let host_id = host_id.to_owned();
    let host_for_panel = host_id.clone();
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<maud::Markup> {
        let stamp = hakobi::metrics::now_ms();
        let db = metrics::MetricsDb::open(&metrics_path)?;
        let (from, to) = templates::history_window_bounds(window, stamp);
        let points = match window {
            templates::HistoryWindow::FiveMinutes => db.query_1m(&host_id, from, to)?,
            templates::HistoryWindow::OneHour => db.query_5m(&host_id, from, to)?,
            templates::HistoryWindow::TwentyFourHours => db.query_1h(&host_id, from, to)?,
        };
        Ok(templates::history_panel(&host_id, window, &points, stamp))
    })
    .await;
    match result {
        Ok(Ok(markup)) => markup,
        Ok(Err(error)) => {
            tracing::warn!(error = %error, "history panel unavailable");
            templates::history_panel(&host_for_panel, window, &[], hakobi::metrics::now_ms())
        }
        Err(error) => {
            tracing::warn!(error = %error, "history panel task failed");
            templates::history_panel(&host_for_panel, window, &[], hakobi::metrics::now_ms())
        }
    }
}

pub(crate) async fn host_resources_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    let Some((_label, agent, endpoint)) = host_endpoint(&state, &id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let system = match agent.system_metrics().await {
        Ok(system) => system,
        Err(error) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!(
                    "Host agent metrics failed: {}",
                    truncate(&error.to_string(), 300)
                ),
            )
                .into_response();
        }
    };
    let containers = state
        .backend
        .containers(&endpoint)
        .await
        .unwrap_or_default();
    let live_url = format!("/hosts/{id}/resources/partial");
    html_response(
        StatusCode::OK,
        templates::host_resources_live(&system, &containers, &live_url),
        Vec::new(),
    )
}

/// Serves the host history panel: SVG charts for the selected time window,
/// read from the metrics database. The live section keeps polling separately.
pub(crate) async fn host_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    if host_endpoint(&state, &id).await.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let window = templates::HistoryWindow::from_query(params.window.as_deref());
    let host_id = id.clone();
    let markup = load_history_markup(&state, &host_id, window).await;
    html_response(StatusCode::OK, markup, Vec::new())
}

pub(crate) async fn hosts_test(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let hosts = match with_catalog(&state, |catalog| catalog.list_host_agents()).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let Some(host) = hosts.iter().find(|h| h.id == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let test_endpoint = format!("agent://{}", host.id);
    let result = state.backend.test(&test_endpoint).await;
    let error = result.as_ref().err().map(|e| truncate(&e.to_string(), 500));
    let id = host.id.clone();
    let record = with_catalog(&state, move |catalog| {
        catalog.record_host_agent_health(&id, error.as_deref())
    })
    .await;
    if let Err(error) = record {
        return internal_error(error);
    }
    let notice = if result.is_ok() {
        "Connection+OK"
    } else {
        "Connection+failed"
    };
    redirect_response(&format!("/hosts?notice={notice}"), Vec::new())
}

pub(crate) async fn hosts_toggle(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let hosts = match with_catalog(&state, |catalog| catalog.list_host_agents()).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let Some(host) = hosts.iter().find(|h| h.id == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let enabled = !host.enabled;
    let id = host.id.clone();
    let result = with_catalog(&state, move |catalog| {
        catalog.set_host_agent_enabled(&id, enabled)
    })
    .await;
    match result {
        Ok(()) => redirect_response("/hosts?notice=Host+updated", Vec::new()),
        Err(error) => internal_error(error),
    }
}

pub(crate) async fn hosts_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let result = with_catalog(&state, move |catalog| catalog.delete_host_agent(&id)).await;
    match result {
        Ok(()) => redirect_response("/hosts?notice=Host+deleted", Vec::new()),
        Err(error) if error.to_string().contains("not found") => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => internal_error(error),
    }
}
