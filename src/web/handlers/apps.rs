//! CP9 app handlers: list, create, detail, edit, delete, deploy, rollback,
//! and the status/deployments/logs HTMX partials.

use axum::{
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::app_deploy::load_app;
use super::app_form::{
    create_app_record, display_error, edit_form_values, enabled_hosts, form_values,
    render_app_form, update_app_record, validate_app_inputs, AppForm,
};
use super::{
    html_response, internal_error, page_csrf, post_is_allowed, redirect_with_notice, with_catalog,
    CsrfForm, ProviderNotice,
};
use crate::web::{templates, AppState};

pub(crate) async fn app_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(notice): Query<ProviderNotice>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let apps = match with_catalog(&state, |catalog| catalog.list_apps()).await {
        Ok(apps) => apps,
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::apps(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &apps,
            notice.notice.as_deref(),
        ),
        cookies,
    )
}

pub(crate) async fn app_new(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let hosts = match enabled_hosts(&state).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let values = templates::AppFormValues {
        name: String::new(),
        host_id: hosts
            .first()
            .map(|host| host.id.clone())
            .unwrap_or_default(),
        build_type: "dockerfile".into(),
        github_repo: String::new(),
        github_branch: "main".into(),
        github_path: String::new(),
        github_dockerfile: "Dockerfile".into(),
        github_auto_deploy: true,
        container_port: String::new(),
        cpu_limit: String::new(),
        memory_limit_mb: String::new(),
        env_json: "{}".into(),
    };
    render_app_form(
        &state,
        &headers,
        &hosts,
        &values,
        "/apps",
        "New app",
        "Create app",
        None,
        StatusCode::OK,
    )
}

pub(crate) async fn app_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<AppForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let hosts = match enabled_hosts(&state).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let values = form_values(&form);
    if let Err(message) = validate_app_inputs(&values, &hosts) {
        return render_app_form(
            &state,
            &headers,
            &hosts,
            &values,
            "/apps/new",
            "New app",
            "Create app",
            Some(&message),
            StatusCode::BAD_REQUEST,
        );
    }
    match create_app_record(&state, &values).await {
        Ok(_) => redirect_with_notice("/apps", "App created."),
        Err(error) => render_app_form(
            &state,
            &headers,
            &hosts,
            &values,
            "/apps/new",
            "New app",
            "Create app",
            Some(&display_error(&error)),
            StatusCode::BAD_REQUEST,
        ),
    }
}

pub(crate) async fn app_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let app_id = app.id.clone();
    let deployments =
        match with_catalog(&state, move |catalog| catalog.list_deployments(&app_id)).await {
            Ok(deployments) => deployments,
            Err(error) => return internal_error(error),
        };
    let latest = deployments.first().cloned();
    let previous = deployments.get(1).cloned();
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::app_detail(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &app,
            &deployments,
            latest.as_ref(),
            previous.as_ref(),
        ),
        cookies,
    )
}

pub(crate) async fn app_status_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let app_id = app.id.clone();
    let latest = match with_catalog(&state, move |catalog| {
        catalog.get_latest_deployment(&app_id)
    })
    .await
    {
        Ok(latest) => latest,
        Err(error) => return internal_error(error),
    };
    html_response(
        StatusCode::OK,
        templates::app_status_body(&app, latest.as_ref()),
        Vec::new(),
    )
}

pub(crate) async fn app_deployments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let app_id = app.id.clone();
    let deployments =
        match with_catalog(&state, move |catalog| catalog.list_deployments(&app_id)).await {
            Ok(deployments) => deployments,
            Err(error) => return internal_error(error),
        };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::app_deployments(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &app,
            &deployments,
        ),
        cookies,
    )
}

pub(crate) async fn app_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let app_id = app.id.clone();
    let deployment = match with_catalog(&state, move |catalog| {
        catalog.get_latest_deployment(&app_id)
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
        templates::app_logs(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &app,
            deployment.as_ref(),
            &logs,
        ),
        cookies,
    )
}

pub(crate) async fn app_edit_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let hosts = match enabled_hosts(&state).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let values = edit_form_values(app);
    render_app_form(
        &state,
        &headers,
        &hosts,
        &values,
        &format!("/apps/{id}/edit"),
        "Edit app",
        "Save changes",
        None,
        StatusCode::OK,
    )
}

pub(crate) async fn app_edit_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<AppForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if load_app(&state, &id).await.ok().flatten().is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let hosts = match enabled_hosts(&state).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let values = form_values(&form);
    if let Err(message) = validate_app_inputs(&values, &hosts) {
        return render_app_form(
            &state,
            &headers,
            &hosts,
            &values,
            &format!("/apps/{id}/edit"),
            "Edit app",
            "Save changes",
            Some(&message),
            StatusCode::BAD_REQUEST,
        );
    }
    match update_app_record(&state, &id, &values).await {
        Ok(()) => redirect_with_notice(&format!("/apps/{id}"), "App updated."),
        Err(error) => render_app_form(
            &state,
            &headers,
            &hosts,
            &values,
            &format!("/apps/{id}/edit"),
            "Edit app",
            "Save changes",
            Some(&display_error(&error)),
            StatusCode::BAD_REQUEST,
        ),
    }
}

pub(crate) async fn app_delete(
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
    // Best-effort cleanup: container + Cloudflare route. The DB delete is
    // authoritative.
    let app_name = remove_app_container(&state, &id).await;
    let result = with_catalog(&state, move |catalog| catalog.delete_app(&id)).await;
    if result.is_ok() {
        if let Some(app_name) = app_name {
            spawn_cloudflare_route_cleanup(&state, app_name);
        }
    }
    match result {
        Ok(()) => redirect_with_notice("/apps", "App deleted."),
        Err(error) if error.to_string().contains("not found") => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => internal_error(error),
    }
}

async fn remove_app_container(state: &AppState, id: &str) -> Option<String> {
    let container = crate::web::deployment::container_name_for(id);
    let app = load_app(state, id).await.ok().flatten()?;
    let _ = state
        .backend
        .remove_container(&format!("agent://{}", app.host_id), &container)
        .await;
    Some(app.name)
}

fn spawn_cloudflare_route_cleanup(state: &AppState, app_name: String) {
    let cf_path = state.catalog_path.clone();
    tokio::spawn(async move {
        let settings = tokio::task::spawn_blocking(move || {
            let catalog = hakobi::catalog::Catalog::open(cf_path.as_ref())?;
            crate::cloudflare::apps_route::load_settings(&catalog)
        })
        .await
        .unwrap_or_else(|error| Err(anyhow::anyhow!(error.to_string())));
        let settings = match settings {
            Ok(settings) => settings,
            Err(_) => return,
        };
        if let Err(error) =
            crate::cloudflare::apps_route::remove_app_route_with(&settings, &app_name).await
        {
            tracing::warn!(error = %error, app = %app_name, "cloudflare route removal skipped");
        }
    });
}
