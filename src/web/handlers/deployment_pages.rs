//! Deployment history page (`/deployment`), its HTMX refresh partial, and
//! the per-environment webhook configuration page.

use axum::{
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::environments::load_environment;
use super::{
    html_response, internal_error, page_csrf, post_is_allowed, redirect_with_notice, with_catalog,
    ProviderNotice,
};
use crate::web::{templates, AppState};

/// A deployment row joined with its environment name.
pub(crate) struct DeploymentRow {
    pub environment_name: String,
    pub deployment: hakobi::catalog::Deployment,
}

impl DeploymentRow {
    /// Borrows the pair as the template view.
    fn view(&self) -> templates::deployment::DeploymentRowView<'_> {
        templates::deployment::DeploymentRowView {
            deployment: &self.deployment,
            environment_name: &self.environment_name,
        }
    }
}

pub(crate) async fn deployment_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(notice): Query<ProviderNotice>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let (rows, environments) = match load_deployment_rows(&state).await {
        Ok(result) => result,
        Err(error) => return internal_error(error),
    };
    let views: Vec<_> = rows.iter().map(DeploymentRow::view).collect();
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::deployment_page(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &views,
            &environments,
            notice.notice.as_deref(),
        ),
        cookies,
    )
}

/// HTMX partial: the table body of the history page, refreshed every 5s.
pub(crate) async fn deployment_list_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    let (rows, _environments) = match load_deployment_rows(&state).await {
        Ok(result) => result,
        Err(error) => return internal_error(error),
    };
    let views: Vec<_> = rows.iter().map(DeploymentRow::view).collect();
    html_response(
        StatusCode::OK,
        templates::deployment_rows(&views),
        Vec::new(),
    )
}

/// Loads recent deployments with environment names in one pass.
async fn load_deployment_rows(
    state: &AppState,
) -> anyhow::Result<(Vec<DeploymentRow>, Vec<hakobi::catalog::Environment>)> {
    with_catalog(state, |catalog| {
        let deployments =
            catalog.list_recent_deployments(hakobi::catalog::DEPLOYMENT_PAGE_LIMIT)?;
        let environments = catalog.list_environments()?;
        let names: std::collections::HashMap<&str, &str> = environments
            .iter()
            .map(|environment| (environment.id.as_str(), environment.name.as_str()))
            .collect();
        let rows = deployments
            .into_iter()
            .map(|deployment| DeploymentRow {
                environment_name: names
                    .get(deployment.environment_id.as_str())
                    .map(|name| (*name).to_owned())
                    .unwrap_or_else(|| "deleted".to_owned()),
                deployment,
            })
            .collect();
        Ok((rows, environments))
    })
    .await
}

#[derive(Debug, Deserialize)]
pub(crate) struct WebhookForm {
    csrf_token: String,
    #[serde(default)]
    webhook_secret: String,
    #[serde(default)]
    webhook_enabled: Option<String>,
}

pub(crate) async fn environment_webhook_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let Some(environment) = load_environment(&state, &id).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::environment_webhook(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &environment,
        ),
        cookies,
    )
}

pub(crate) async fn environment_webhook_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<WebhookForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if load_environment(&state, &id).await.ok().flatten().is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let enabled = form.webhook_enabled.as_deref() == Some("on");
    let secret = form.webhook_secret.trim().to_owned();
    if enabled && secret.is_empty() {
        return redirect_with_notice(
            &format!("/environments/{id}/webhook"),
            "A secret is required to enable webhooks.",
        );
    }
    let id_key = id.clone();
    let result = with_catalog(&state, move |catalog| {
        catalog.set_environment_webhook(&id_key, Some(&secret), enabled)
    })
    .await;
    match result {
        Ok(()) => redirect_with_notice(
            &format!("/environments/{id}/webhook"),
            if enabled {
                "Webhook enabled. Use the secret in the webhook URL."
            } else {
                "Webhook disabled."
            },
        ),
        Err(error) => internal_error(error),
    }
}
