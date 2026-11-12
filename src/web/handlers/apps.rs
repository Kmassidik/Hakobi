//! CP9 app handlers: list, create, detail, edit, delete, deploy, rollback,
//! and the status/deployments/logs HTMX partials.

use axum::{
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};

use super::app_deploy::load_app;
use super::app_form::{
    display_error, enabled_hosts, form_values, non_empty, parse_port, render_app_form,
    validate_app_inputs, AppForm,
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

    let name = values.name.clone();
    let host_id = values.host_id.clone();
    let build_type = values.build_type.clone();
    let github_repo = non_empty(&values.github_repo);
    let github_branch = non_empty(&values.github_branch);
    let github_path = non_empty(&values.github_path);
    let github_dockerfile = non_empty(&values.github_dockerfile);
    let github_auto_deploy = values.github_auto_deploy;
    let container_port = parse_port(&values.container_port);
    let result = with_catalog(&state, move |catalog| {
        catalog.create_app(
            &name,
            &host_id,
            &build_type,
            github_repo.as_deref(),
            github_branch.as_deref(),
            github_path.as_deref(),
            github_dockerfile.as_deref(),
            github_auto_deploy,
            container_port,
        )
    })
    .await;
    match result {
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
    let values = templates::AppFormValues {
        name: app.name.clone(),
        host_id: app.host_id.clone(),
        build_type: app.build_type.clone(),
        github_repo: app.github_repo.clone().unwrap_or_default(),
        github_branch: app.github_branch.clone().unwrap_or_default(),
        github_path: app.github_path.clone().unwrap_or_default(),
        github_dockerfile: app.github_dockerfile.clone().unwrap_or_default(),
        github_auto_deploy: app.github_auto_deploy,
        container_port: app
            .container_port
            .map(|port| port.to_string())
            .unwrap_or_default(),
    };
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

    let name = values.name.clone();
    let host_id = values.host_id.clone();
    let build_type = values.build_type.clone();
    let github_repo = non_empty(&values.github_repo);
    let github_branch = non_empty(&values.github_branch);
    let github_path = non_empty(&values.github_path);
    let github_dockerfile = non_empty(&values.github_dockerfile);
    let github_auto_deploy = values.github_auto_deploy;
    let container_port = parse_port(&values.container_port);
    let app_id = id.clone();
    let result = with_catalog(&state, move |catalog| {
        catalog.update_app(
            &app_id,
            &name,
            &host_id,
            &build_type,
            github_repo.as_deref(),
            github_branch.as_deref(),
            github_path.as_deref(),
            github_dockerfile.as_deref(),
            github_auto_deploy,
            container_port,
        )
    })
    .await;
    match result {
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
    let container = crate::web::deployment::container_name_for(&id);
    let mut app_snapshot = None;
    if let Ok(Some(app)) = load_app(&state, &id).await {
        let _ = state
            .backend
            .remove_container(&format!("agent://{}", app.host_id), &container)
            .await;
        app_snapshot = Some((app.name, app.host_port));
    }
    let result = with_catalog(&state, move |catalog| catalog.delete_app(&id)).await;
    if result.is_ok() {
        if let Some((app_name, _host_port)) = app_snapshot {
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
    }
    match result {
        Ok(()) => redirect_with_notice("/apps", "App deleted."),
        Err(error) if error.to_string().contains("not found") => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => internal_error(error),
    }
}
