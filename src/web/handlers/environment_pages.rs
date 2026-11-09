//! Environment detail, deployments, logs, deploy/rollback placeholders, and
//! delete handlers.

use axum::{
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::environments::{
    display_error, enabled_hosts, form_values, load_environment, non_empty,
    render_environment_form, validate_environment_inputs, EnvironmentForm,
};
use super::{
    html_response, internal_error, page_csrf, post_is_allowed, redirect_with_notice, with_catalog,
};
use crate::web::{templates, AppState};

#[derive(Debug, Deserialize)]
pub(crate) struct DeployForm {
    csrf_token: String,
}

pub(crate) async fn environment_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let environment = match load_environment(&state, &id).await {
        Ok(Some(environment)) => environment,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let environment_key = environment.id.clone();
    let deployments = match with_catalog(&state, move |catalog| {
        catalog.list_deployments(&environment_key)
    })
    .await
    {
        Ok(deployments) => deployments,
        Err(error) => return internal_error(error),
    };
    let hosts = match with_catalog(&state, |catalog| catalog.list_host_agents()).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let latest = deployments.first().cloned();
    let previous = deployments.get(1).cloned();
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::environment_detail(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &environment,
            &hosts,
            &deployments,
            latest.as_ref(),
            previous.as_ref(),
        ),
        cookies,
    )
}

pub(crate) async fn environment_status_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    let environment = match load_environment(&state, &id).await {
        Ok(Some(environment)) => environment,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let environment_key = environment.id.clone();
    let latest = match with_catalog(&state, move |catalog| {
        catalog.get_latest_deployment(&environment_key)
    })
    .await
    {
        Ok(latest) => latest,
        Err(error) => return internal_error(error),
    };
    html_response(
        StatusCode::OK,
        templates::environment_status_body(&environment, latest.as_ref()),
        Vec::new(),
    )
}

pub(crate) async fn environment_deployments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let environment = match load_environment(&state, &id).await {
        Ok(Some(environment)) => environment,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let environment_key = environment.id.clone();
    let deployments = match with_catalog(&state, move |catalog| {
        catalog.list_deployments(&environment_key)
    })
    .await
    {
        Ok(deployments) => deployments,
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::environment_deployments(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &environment,
            &deployments,
        ),
        cookies,
    )
}

pub(crate) async fn environment_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let environment = match load_environment(&state, &id).await {
        Ok(Some(environment)) => environment,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let environment_key = environment.id.clone();
    let deployment = match with_catalog(&state, move |catalog| {
        catalog.get_latest_deployment(&environment_key)
    })
    .await
    {
        Ok(deployment) => deployment,
        Err(error) => return internal_error(error),
    };
    let logs = match &deployment {
        Some(record) => {
            let deployment_id = record.id.clone();
            with_catalog(&state, move |catalog| {
                catalog.list_deployment_logs(&deployment_id)
            })
            .await
            .unwrap_or_default()
        }
        None => Vec::new(),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::environment_logs(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &environment,
            deployment.as_ref(),
            &logs,
        ),
        cookies,
    )
}

pub(crate) async fn environment_deploy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<DeployForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(environment) = load_environment(&state, &id).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    start_or_redirect(
        &state,
        &environment,
        crate::web::deployment::Trigger::Manual,
        None,
        None,
    )
}

pub(crate) async fn environment_rollback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<DeployForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(environment) = load_environment(&state, &id).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Rollback redeploys the latest running deployment's version: the
    // executor resolves the previous version at run time and rebuilds it.
    let environment_key = environment.id.clone();
    let previous = match with_catalog(&state, move |catalog| {
        Ok(catalog
            .list_deployments(&environment_key)?
            .into_iter()
            .find(|record| record.status == "running"))
    })
    .await
    {
        Ok(previous) => previous,
        Err(error) => return internal_error(error),
    };
    let Some(previous) = previous else {
        return redirect_with_notice(
            &format!("/environments/{}", environment.id),
            "No running deployment to roll back to.",
        );
    };
    let notice = format!("Rolling back to v{}.", previous.version);
    start_or_redirect(
        &state,
        &environment,
        crate::web::deployment::Trigger::Manual,
        None,
        Some(&notice),
    )
}

/// Shared tail of deploy/rollback: queue the deployment and redirect with a
/// notice, or surface the concurrent-deployment conflict.
fn start_or_redirect(
    state: &AppState,
    environment: &hakobi::catalog::Environment,
    trigger: crate::web::deployment::Trigger,
    commit_sha: Option<&str>,
    notice: Option<&str>,
) -> Response {
    let deps = crate::web::deployment::ExecutorDeps {
        catalog_path: (*state.catalog_path).clone(),
        build_runner: state.build_runner.clone(),
        backend: state.backend.clone(),
    };
    match crate::web::deployment::start_deployment(
        &deps,
        &state.deployments,
        &environment.id,
        trigger,
        commit_sha,
        None,
    ) {
        Ok(Some(_)) => {
            let base = format!("/environments/{}", environment.id);
            redirect_with_notice(&base, notice.unwrap_or("Deployment queued."))
        }
        Ok(None) => redirect_with_notice(
            &format!("/environments/{}", environment.id),
            "A deployment is already running for this environment.",
        ),
        Err(error) => internal_error(error),
    }
}

pub(crate) async fn environment_edit_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let environment = match load_environment(&state, &id).await {
        Ok(Some(environment)) => environment,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let hosts = match enabled_hosts(&state).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let values = templates::EnvironmentFormValues {
        name: environment.name.clone(),
        host_id: environment.host_id.clone(),
        build_type: environment.build_type.clone(),
        source_type: environment.source_type.clone(),
        git_url: environment.git_url.clone().unwrap_or_default(),
        git_branch: environment.git_branch.clone().unwrap_or_default(),
        registry_mode: environment.registry_mode.clone(),
    };
    render_environment_form(
        &state,
        &headers,
        &hosts,
        &values,
        &format!("/environments/{id}/edit"),
        "Edit environment",
        "Save changes",
        None,
        StatusCode::OK,
    )
}

pub(crate) async fn environment_edit_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<EnvironmentForm>,
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
    let hosts = match enabled_hosts(&state).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let values = form_values(&form);
    if let Err(message) = validate_environment_inputs(&values, &hosts) {
        return render_environment_form(
            &state,
            &headers,
            &hosts,
            &values,
            &format!("/environments/{id}/edit"),
            "Edit environment",
            "Save changes",
            Some(&message),
            StatusCode::BAD_REQUEST,
        );
    }

    let name = values.name.clone();
    let host_id = values.host_id.clone();
    let build_type = values.build_type.clone();
    let source_type = values.source_type.clone();
    let git_url = non_empty(&values.git_url);
    let git_branch = non_empty(&values.git_branch);
    let registry_mode = values.registry_mode.clone();
    let environment_id = id.clone();
    let result = with_catalog(&state, move |catalog| {
        catalog.update_environment(
            &environment_id,
            &name,
            &host_id,
            &build_type,
            &source_type,
            git_url.as_deref(),
            git_branch.as_deref(),
            &registry_mode,
        )
    })
    .await;
    match result {
        Ok(()) => redirect_with_notice(&format!("/environments/{id}"), "Environment updated."),
        Err(error) => render_environment_form(
            &state,
            &headers,
            &hosts,
            &values,
            &format!("/environments/{id}/edit"),
            "Edit environment",
            "Save changes",
            Some(&display_error(&error)),
            StatusCode::BAD_REQUEST,
        ),
    }
}

pub(crate) async fn environment_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<super::CsrfForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let result = with_catalog(&state, move |catalog| catalog.delete_environment(&id)).await;
    match result {
        Ok(()) => redirect_with_notice("/environments", "Environment deleted."),
        Err(error) if error.to_string().contains("not found") => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => internal_error(error),
    }
}
