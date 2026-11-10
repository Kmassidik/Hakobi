//! HTMX partial handlers: metrics, readiness, and status badge.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use axum::extract::Form;
use serde::Deserialize;

use super::dashboard::{collect_dashboard_metrics, load_readiness};
use super::{
    authenticated, html_response, page_csrf, post_is_allowed, redirect_response, truncate,
};
use crate::web::{agent_client::HostAgent, templates, AppState};

#[derive(Debug, Deserialize)]
pub(crate) struct InstallToolForm {
    csrf_token: String,
}

pub(crate) async fn install_tool(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(tool): axum::extract::Path<String>,
    Form(form): Form<InstallToolForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !matches!(
        tool.as_str(),
        "docker" | "compose" | "kubectl" | "nix" | "nixpacks"
    ) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(agent) = HostAgent::from_env() else {
        return dashboard_notice("Host agent is not configured.");
    };
    if let Err(error) = agent.install(&tool).await {
        return dashboard_notice(&format!("{tool} install failed to start: {error}"));
    }
    let mut message = format!("{tool} installation is running on the host.");
    for _ in 0..15 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let Ok(jobs) = agent.install_status().await else {
            continue;
        };
        let Some(job) = jobs.get(&tool) else {
            continue;
        };
        if job.running {
            continue;
        }
        message = if job.success {
            format!("{tool} installed on the host.")
        } else {
            format!("{tool} install failed: {}", truncate(&job.output, 400))
        };
        break;
    }
    dashboard_notice(&message)
}

fn dashboard_notice(message: &str) -> Response {
    let encoded: String = url::form_urlencoded::byte_serialize(message.as_bytes()).collect();
    redirect_response(&format!("/?notice={encoded}"), Vec::new())
}

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
