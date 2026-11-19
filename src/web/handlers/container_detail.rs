//! Container detail page and live stats/log partial handlers.
//!
//! The detail page renders the full inspect payload once and lets HTMX poll
//! the stats and logs partials every two seconds afterwards.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::{
    app_container_names, html_response, internal_error, load_providers, page_csrf, truncate,
};
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
    render_container_detail(&state, &headers, &provider, &name).await
}

async fn render_container_detail(
    state: &AppState,
    headers: &HeaderMap,
    provider: &ProviderConnection,
    name: &str,
) -> Response {
    let app_names = match app_container_names(state).await {
        Ok(names) => names,
        Err(error) => return internal_error(error),
    };
    let display_name = app_names.get(name).map_or(name, String::as_str);
    let (csrf_token, cookies) = page_csrf(state, headers);
    let inspect = state
        .backend
        .inspect_container(&provider.endpoint, name)
        .await;
    let detail = match inspect {
        Ok(inspect) => match redact_detail(state, inspect).await {
            Ok(inspect) => Ok(inspect),
            Err(error) => return internal_error(error),
        },
        // A missing container is a user-facing miss; everything else is a
        // provider failure rendered with its bounded message.
        Err(error) if error_chain_contains_not_found(&error) => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) => Err(truncate(&error.to_string(), 500)),
    };
    let (stats, logs) = load_live_sections(state, provider, name, detail.is_ok()).await;
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
            name,
            display_name,
            data.as_ref(),
            stats.as_ref(),
            logs.as_ref(),
        ),
        cookies,
    )
}

async fn redact_detail(
    state: &AppState,
    mut inspect: hakobi::providers::DockerInspect,
) -> anyhow::Result<hakobi::providers::DockerInspect> {
    redact_managed_secrets(state, &mut inspect).await?;
    Ok(inspect)
}

fn managed_app_id(inspect: &hakobi::providers::DockerInspect) -> Option<String> {
    let name = inspect
        .name
        .trim_start_matches('/')
        .strip_prefix("hakobi-")?;
    let app_id = name.get(..36)?;
    uuid::Uuid::parse_str(app_id).ok()?;
    Some(app_id.to_owned())
}

async fn redact_managed_secrets(
    state: &AppState,
    inspect: &mut hakobi::providers::DockerInspect,
) -> anyhow::Result<()> {
    let Some(app_id) = managed_app_id(inspect) else {
        return Ok(());
    };
    let names =
        super::with_catalog(state, move |catalog| catalog.list_app_secret_names(&app_id)).await?;
    redact_environment(inspect, &names);
    Ok(())
}

fn redact_environment(inspect: &mut hakobi::providers::DockerInspect, names: &[String]) {
    for variable in &mut inspect.env {
        if let Some((name, _)) = variable.split_once('=') {
            if names.iter().any(|secret| secret == name) {
                *variable = format!("{name}=[redacted]");
            }
        }
    }
}

async fn load_live_sections(
    state: &AppState,
    provider: &ProviderConnection,
    name: &str,
    available: bool,
) -> (Option<DockerContainerUsage>, Option<DockerLogs>) {
    if !available {
        return (None, None);
    }
    (
        container_detail_stats(state, &provider.endpoint, name)
            .await
            .ok(),
        container_detail_logs(state, &provider.endpoint, name, DETAIL_TAIL_LINES)
            .await
            .ok(),
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

#[cfg(test)]
mod tests {
    #[test]
    fn managed_secret_values_are_masked() {
        let mut inspect = hakobi::providers::DockerInspect {
            name: "hakobi-2b85234e-b896-4d72-b166-06694b27c6da-worker-worker".into(),
            env: vec![
                "DATABASE_URL=postgres://private".into(),
                "RUST_LOG=info".into(),
            ],
            ..Default::default()
        };
        assert_eq!(
            super::managed_app_id(&inspect).as_deref(),
            Some("2b85234e-b896-4d72-b166-06694b27c6da")
        );
        super::redact_environment(&mut inspect, &["DATABASE_URL".into()]);
        assert_eq!(inspect.env[0], "DATABASE_URL=[redacted]");
        assert_eq!(inspect.env[1], "RUST_LOG=info");
    }
}
