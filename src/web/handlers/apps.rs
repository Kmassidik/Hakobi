//! CP9 app handlers: list, create, detail, edit, delete, deploy, rollback,
//! and the status/deployments/logs HTMX partials.

use axum::{
    extract::{Form, Path, RawQuery, State},
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
};
use crate::web::{
    pagination::{page_number, paginate, parse_query},
    templates, AppState,
};

pub(crate) async fn app_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let query = parse_query(raw_query.as_deref());
    let apps = match with_catalog(&state, |catalog| catalog.list_apps()).await {
        Ok(apps) => apps,
        Err(error) => return internal_error(error),
    };
    let apps_page = paginate(&apps, page_number(&query, "page"), 20);
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::apps(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &apps_page,
            &query,
            query.get("notice").map(String::as_str),
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
        container_port: "8080".into(),
        requested_host_port: String::new(),
        cpu_limit: "1".into(),
        memory_limit_mb: "512".into(),
        env_json: "{}".into(),
    };
    render_app_form(
        &state,
        &headers,
        &hosts,
        &values,
        "/apps",
        "New app",
        "Build application",
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
            "/apps",
            "New app",
            "Build application",
            Some(&message),
            StatusCode::BAD_REQUEST,
        );
    }
    match create_app_record(&state, &values).await {
        Ok(app_id) => start_initial_build(&state, &app_id),
        Err(error) => render_app_form(
            &state,
            &headers,
            &hosts,
            &values,
            "/apps",
            "New app",
            "Build application",
            Some(&display_error(&error)),
            StatusCode::BAD_REQUEST,
        ),
    }
}

fn start_initial_build(state: &AppState, app_id: &str) -> Response {
    let deps = crate::web::deployment::ExecutorDeps {
        catalog_path: (*state.catalog_path).clone(),
        build_runner: state.build_runner.clone(),
        backend: state.backend.clone(),
    };
    let started = crate::web::deployment::start_deployment(
        &deps,
        &state.deployments,
        app_id,
        crate::web::deployment::Trigger::Manual,
        None,
        Some("Initial application build"),
    );
    match started {
        Ok(Some(_)) => redirect_with_notice(
            &format!("/apps/{app_id}"),
            "Build started. Your app will remain private until you publish a subdomain.",
        ),
        Ok(None) => redirect_with_notice(
            &format!("/apps/{app_id}"),
            "App created, but another deployment is already active. Use Deploy to retry the initial build.",
        ),
        Err(error) => {
            tracing::error!(app_id = %app_id, error = %error, "initial build queue failed");
            redirect_with_notice(
                &format!("/apps/{app_id}"),
                "App created, but the initial build could not be queued. Use Deploy on this page to retry.",
            )
        }
    }
}

pub(crate) async fn app_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw_query): RawQuery,
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
    let query = parse_query(raw_query.as_deref());
    let deployments_page = paginate(&deployments, page_number(&query, "deployments_page"), 10);
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    let apps_domain = super::app_publish::apps_domain(&state).await;
    html_response(
        StatusCode::OK,
        templates::app_detail(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &app,
            &deployments_page,
            &query,
            latest.as_ref(),
            previous.as_ref(),
            apps_domain.as_deref(),
        ),
        cookies,
    )
}

pub(crate) async fn app_deployments(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    RawQuery(raw_query): RawQuery,
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
    let query = parse_query(raw_query.as_deref());
    let deployments_page = paginate(&deployments, page_number(&query, "deployments_page"), 10);
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::app_deployments(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &app,
            &deployments_page,
            &query,
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

#[derive(Debug, serde::Deserialize)]
pub(crate) struct DeleteAppForm {
    confirm_name: String,
    #[serde(default)]
    delete_data: Option<String>,
    csrf_token: String,
}

pub(crate) async fn app_delete(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Form(form): Form<DeleteAppForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let app = match load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    if form.confirm_name != app.name {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let image_tags = match app_image_tags(&state, &app).await {
        Ok(tags) => tags,
        Err(error) => return internal_error(error),
    };
    let delete_data = form.delete_data.as_deref() == Some("on");
    let resources = super::app_cleanup::snapshot_owned_resources(&state, &app, delete_data).await;
    // Best-effort cleanup: container, owned resources, images, and Cloudflare
    // route. The DB delete is authoritative.
    let route = remove_app_container(&state, &app).await;
    super::app_cleanup::remove_owned_resources(&state, &app, resources).await;
    remove_app_images(&state, &app, &image_tags).await;
    let result = with_catalog(&state, move |catalog| catalog.delete_app(&id)).await;
    if result.is_ok() {
        if let Some(subdomain) = route {
            spawn_cloudflare_route_cleanup(&state, subdomain);
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

async fn app_image_tags(
    state: &AppState,
    app: &hakobi::catalog::App,
) -> anyhow::Result<Vec<String>> {
    let app_id = app.id.clone();
    let app_name = app.name.clone();
    with_catalog(state, move |catalog| {
        Ok(catalog
            .list_deployments(&app_id)?
            .into_iter()
            .map(|deployment| {
                deployment.image_tag.unwrap_or_else(|| {
                    crate::web::deployment::image_tag_for(&app_name, deployment.version)
                })
            })
            .collect())
    })
    .await
}

async fn remove_app_container(state: &AppState, app: &hakobi::catalog::App) -> Option<String> {
    let container = crate::web::deployment::container_name_for(&app.id);
    let _ = state
        .backend
        .remove_container(&format!("agent://{}", app.host_id), &container)
        .await;
    app.subdomain.clone()
}

async fn remove_app_images(state: &AppState, app: &hakobi::catalog::App, tags: &[String]) {
    let endpoint = format!("agent://{}", app.host_id);
    for tag in tags {
        if let Err(error) = state.backend.remove_image(&endpoint, tag).await {
            tracing::warn!(app = %app.name, image = %tag, %error, "app image removal skipped");
        }
    }
}

fn spawn_cloudflare_route_cleanup(state: &AppState, subdomain: String) {
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
            crate::cloudflare::apps_route::remove_app_route_with(&settings, &subdomain).await
        {
            tracing::warn!(error = %error, app = %subdomain, "cloudflare route removal skipped");
        }
    });
}
