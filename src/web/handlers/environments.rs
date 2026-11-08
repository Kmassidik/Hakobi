//! CP5 environment handlers: list, create, and shared form helpers.

use axum::{
    extract::{Form, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use super::{
    html_response, internal_error, page_csrf, post_is_allowed, redirect_with_notice, with_catalog,
    ProviderNotice,
};
use crate::web::{templates, AppState};
use hakobi::catalog::{validate_build_type, validate_registry_mode, validate_source_type};

#[derive(Debug, Deserialize)]
pub(crate) struct EnvironmentForm {
    pub(crate) name: String,
    pub(crate) host_id: String,
    pub(crate) build_type: String,
    pub(crate) source_type: String,
    #[serde(default)]
    pub(crate) git_url: String,
    #[serde(default)]
    pub(crate) git_branch: String,
    #[serde(default)]
    pub(crate) registry_mode: String,
    pub(crate) csrf_token: String,
}

pub(crate) async fn environment_list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(notice): Query<ProviderNotice>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let environments = match with_catalog(&state, |catalog| catalog.list_environments()).await {
        Ok(environments) => environments,
        Err(error) => return internal_error(error),
    };
    let hosts = match with_catalog(&state, |catalog| catalog.list_host_agents()).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::environments(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            &environments,
            &hosts,
            notice.notice.as_deref(),
        ),
        cookies,
    )
}

pub(crate) async fn environment_new(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    let hosts = match enabled_hosts(&state).await {
        Ok(hosts) => hosts,
        Err(error) => return internal_error(error),
    };
    let values = templates::EnvironmentFormValues {
        name: String::new(),
        host_id: hosts
            .first()
            .map(|host| host.id.clone())
            .unwrap_or_default(),
        build_type: "compose".into(),
        source_type: "git".into(),
        git_url: String::new(),
        git_branch: "main".into(),
        registry_mode: "local".into(),
    };
    render_environment_form(
        &state,
        &headers,
        &hosts,
        &values,
        "/environments/new",
        "New environment",
        "Create environment",
        None,
        StatusCode::OK,
    )
}

pub(crate) async fn environment_create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<EnvironmentForm>,
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
    if let Err(message) = validate_environment_inputs(&values, &hosts) {
        return render_environment_form(
            &state,
            &headers,
            &hosts,
            &values,
            "/environments/new",
            "New environment",
            "Create environment",
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
    let result = with_catalog(&state, move |catalog| {
        catalog.create_environment(
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
        Ok(_) => redirect_with_notice("/environments", "Environment created."),
        Err(error) => render_environment_form(
            &state,
            &headers,
            &hosts,
            &values,
            "/environments/new",
            "New environment",
            "Create environment",
            Some(&display_error(&error)),
            StatusCode::BAD_REQUEST,
        ),
    }
}

pub(crate) async fn load_environment(
    state: &AppState,
    id: &str,
) -> anyhow::Result<Option<hakobi::catalog::Environment>> {
    let id = id.to_owned();
    with_catalog(state, move |catalog| catalog.get_environment(&id)).await
}

pub(crate) async fn enabled_hosts(
    state: &AppState,
) -> anyhow::Result<Vec<hakobi::catalog::HostAgent>> {
    let hosts = with_catalog(state, |catalog| catalog.list_host_agents()).await?;
    Ok(hosts.into_iter().filter(|host| host.enabled).collect())
}

pub(crate) fn form_values(form: &EnvironmentForm) -> templates::EnvironmentFormValues {
    templates::EnvironmentFormValues {
        name: form.name.trim().to_owned(),
        host_id: form.host_id.trim().to_owned(),
        build_type: form.build_type.trim().to_owned(),
        source_type: form.source_type.trim().to_owned(),
        git_url: form.git_url.trim().to_owned(),
        git_branch: form.git_branch.trim().to_owned(),
        registry_mode: if form.registry_mode.trim().is_empty() {
            "local".to_owned()
        } else {
            form.registry_mode.trim().to_owned()
        },
    }
}

pub(crate) fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

pub(crate) fn validate_environment_inputs(
    values: &templates::EnvironmentFormValues,
    hosts: &[hakobi::catalog::HostAgent],
) -> Result<(), String> {
    if values.name.is_empty()
        || values.name.len() > 64
        || !values
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(
            "Environment name must be 1–64 ASCII letters, digits, '-', '_', or '.'.".into(),
        );
    }
    if values.host_id.is_empty() {
        return Err("Choose an enabled host for this environment.".into());
    }
    if !hosts.iter().any(|host| host.id == values.host_id) {
        return Err("The selected host is missing or disabled.".into());
    }
    validate_build_type(&values.build_type)
        .map_err(|_| "Build type must be compose, dockerfile, or nixpacks.".to_owned())?;
    validate_source_type(&values.source_type)
        .map_err(|_| "Source type must be git or upload.".to_owned())?;
    if values.source_type == "git" && values.git_url.is_empty() {
        return Err("Git URL is required when the source type is git.".into());
    }
    if values.source_type == "upload" && !values.git_url.is_empty() {
        return Err("Git URL must be empty when the source type is upload.".into());
    }
    if values.git_url.len() > 2048 {
        return Err("Git URL is too long.".into());
    }
    validate_registry_mode(&values.registry_mode)
        .map_err(|_| "Registry mode must be local or push.".to_owned())?;
    Ok(())
}

pub(crate) fn display_error(error: &anyhow::Error) -> String {
    super::truncate(&error.to_string(), 500)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_environment_form(
    state: &AppState,
    headers: &HeaderMap,
    hosts: &[hakobi::catalog::HostAgent],
    values: &templates::EnvironmentFormValues,
    action: &str,
    heading: &str,
    submit_label: &str,
    error: Option<&str>,
    status: StatusCode,
) -> Response {
    let (csrf_token, cookies) = page_csrf(state, headers);
    html_response(
        status,
        templates::environment_form(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            hosts,
            values,
            action,
            heading,
            submit_label,
            error,
        ),
        cookies,
    )
}
