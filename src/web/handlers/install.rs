//! Control-plane proxy for native host-agent tool installation.

use axum::{
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{
    authenticated, html_response, post_is_allowed, redirect_with_error, redirect_with_notice,
    AppState,
};
use crate::web::{agent_client::HostAgent, templates};

#[derive(Debug, Deserialize)]
pub(crate) struct InstallForm {
    csrf_token: String,
}

/// Starts a real installer on the configured native host agent.
pub(crate) async fn host_install(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tool): Path<String>,
    Form(form): Form<InstallForm>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !supported_tool(&tool) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(agent) = HostAgent::from_env() else {
        return redirect_with_error(
            "/dashboard",
            "A host agent must be configured before installing tools on the host.",
        );
    };
    match agent.install(&tool).await {
        Ok(()) => redirect_with_notice(
            "/dashboard",
            &format!(
                "{tool} installation started on the host. Readiness will refresh automatically."
            ),
        ),
        Err(error) => {
            tracing::warn!(tool = %tool, error = %error, "host install could not start");
            let detail = super::truncate(&error.to_string(), 500);
            redirect_with_error(
                "/dashboard",
                &format!("Could not start {tool} installation: {detail}"),
            )
        }
    }
}

/// Reports pending and completed agent jobs to the readiness polling region.
pub(crate) async fn host_install_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !authenticated(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let agent = HostAgent::from_env();
    let result = match agent.as_ref() {
        Some(agent) => agent.install_status().await,
        None => Ok(Default::default()),
    };
    let (jobs, error) = match result {
        Ok(jobs) => (jobs, None),
        Err(error) => {
            tracing::warn!(error = %error, "host install status unavailable");
            (
                Default::default(),
                Some("Host install status is temporarily unavailable."),
            )
        }
    };
    html_response(
        StatusCode::OK,
        templates::install_status_results(&jobs, error),
        Vec::new(),
    )
}

fn supported_tool(tool: &str) -> bool {
    matches!(tool, "docker" | "compose" | "kubectl" | "nix" | "nixpacks")
}
