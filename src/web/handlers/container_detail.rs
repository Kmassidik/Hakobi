//! Container detail page and live stats/log partial handlers.
//!
//! The detail page renders the full inspect payload once and lets HTMX poll
//! the stats and logs partials every two seconds afterwards.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::{html_response, internal_error, load_providers, page_csrf, truncate};
use crate::web::{templates, AppState};
use hakobi::{
    catalog::ProviderConnection,
    providers::{docker::validate_container_name, DockerContainerUsage, DockerLogs},
};

/// Log lines polled by the detail page, matching the agent's default window.
const DETAIL_TAIL_LINES: usize = 100;

/// Loads the enabled provider row for the detail pages without filtering on
/// kind, so the page works for any enabled provider id in the path.
async fn detail_provider(
    state: &AppState,
    provider_id: &str,
) -> anyhow::Result<Option<ProviderConnection>> {
    let providers = load_providers(state).await?;
    Ok(providers
        .into_iter()
        .find(|provider| provider.id == provider_id && provider.enabled))
}

pub(crate) async fn container_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if validate_container_name(&name).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let provider = match detail_provider(&state, &provider).await {
        Ok(Some(provider)) => provider,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let inspect = state
        .backend
        .inspect_container(&provider.endpoint, &name)
        .await;
    let detail = match inspect {
        Ok(inspect) => Ok(inspect),
        // A missing container is a user-facing miss; everything else is a
        // provider failure rendered with its bounded message.
        Err(error) if error_chain_contains_not_found(&error) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) => Err(truncate(&error.to_string(), 500)),
    };
    let (stats, logs) = match &detail {
        Ok(_) => (
            container_detail_stats(&state, &provider.endpoint, &name)
                .await
                .ok(),
            container_detail_logs(&state, &provider.endpoint, &name, DETAIL_TAIL_LINES)
                .await
                .ok(),
        ),
        Err(_) => (None, None),
    };
    let data = detail.ok().map(|inspect| templates::ContainerDetailData {
        provider_id: provider.id.clone(),
        provider_name: provider.name.clone(),
        detail: inspect,
    });
    html_response(
        StatusCode::OK,
        templates::container_detail(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &name,
            data.as_ref(),
            stats.as_ref(),
            logs.as_ref(),
        ),
        cookies,
    )
}

pub(crate) async fn container_detail_stats_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    if validate_container_name(&name).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(provider) = detail_provider(&state, &provider).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match container_detail_stats(&state, &provider.endpoint, &name).await {
        Ok(stats) => html_response(
            StatusCode::OK,
            templates::container_stats_section(&name, &stats),
            Vec::new(),
        ),
        Err(error) => html_response(
            StatusCode::BAD_GATEWAY,
            templates::container_stats_error(&truncate(&error.to_string(), 500)),
            Vec::new(),
        ),
    }
}

pub(crate) async fn container_detail_logs_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((provider, name)): Path<(String, String)>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    if validate_container_name(&name).is_err() {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(provider) = detail_provider(&state, &provider).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match container_detail_logs(&state, &provider.endpoint, &name, DETAIL_TAIL_LINES).await {
        Ok(logs) => html_response(
            StatusCode::OK,
            templates::container_logs_section(&name, &logs),
            Vec::new(),
        ),
        Err(error) => html_response(
            StatusCode::BAD_GATEWAY,
            templates::container_logs_error(&truncate(&error.to_string(), 500)),
            Vec::new(),
        ),
    }
}

async fn container_detail_stats(
    state: &AppState,
    endpoint: &str,
    name: &str,
) -> anyhow::Result<DockerContainerUsage> {
    state.backend.container_stats(endpoint, name).await
}

async fn container_detail_logs(
    state: &AppState,
    endpoint: &str,
    name: &str,
    tail: usize,
) -> anyhow::Result<DockerLogs> {
    state.backend.container_logs(endpoint, name, tail).await
}

/// Reports whether the error chain carries Docker's "no such container" 404.
///
/// Bollard surfaces the daemon's reply as a status-code error; the string
/// check keeps the detail page honest for both backends without matching on
/// fragile error formatting.
fn error_chain_contains_not_found(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("No such container"))
        || error
            .chain()
            .any(|cause| cause.to_string().contains("no such container"))
}
