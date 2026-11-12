//! Deploy and rollback handlers for one app, plus the shared
//! queue-and-redirect tail into the deployment executor.

use axum::{
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{internal_error, post_is_allowed, redirect_with_notice, with_catalog, AppState};
use hakobi::catalog::App;

#[derive(Debug, Deserialize)]
pub(crate) struct DeployForm {
    csrf_token: String,
}

/// Loads one app by id from the app's catalog.
pub(crate) async fn load_app(state: &AppState, id: &str) -> anyhow::Result<Option<App>> {
    let id = id.to_owned();
    with_catalog(state, move |catalog| catalog.get_app(&id)).await
}

pub(crate) async fn app_deploy(
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
    let Some(app) = load_app(&state, &id).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    start_or_redirect(
        &state,
        &app,
        crate::web::deployment::Trigger::Manual,
        None,
        None,
    )
}

pub(crate) async fn app_rollback(
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
    let Some(app) = load_app(&state, &id).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    // Rollback redeploys the latest running deployment's version: the
    // executor resolves the previous version at run time and rebuilds it.
    let app_id = app.id.clone();
    let previous = match with_catalog(&state, move |catalog| {
        Ok(catalog
            .list_deployments(&app_id)?
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
            &format!("/apps/{}", app.id),
            "No running deployment to roll back to.",
        );
    };
    let notice = format!("Rolling back to v{}.", previous.version);
    start_or_redirect(
        &state,
        &app,
        crate::web::deployment::Trigger::Manual,
        None,
        Some(&notice),
    )
}

/// Shared tail of deploy/rollback: queue the deployment and redirect with a
/// notice, or surface the concurrent-deployment conflict.
fn start_or_redirect(
    state: &AppState,
    app: &App,
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
        &app.id,
        trigger,
        commit_sha,
        None,
    ) {
        Ok(Some(_)) => {
            let base = format!("/apps/{}", app.id);
            redirect_with_notice(&base, notice.unwrap_or("Deployment queued."))
        }
        Ok(None) => redirect_with_notice(
            &format!("/apps/{}", app.id),
            "A deployment is already running for this app.",
        ),
        Err(error) => internal_error(error),
    }
}
