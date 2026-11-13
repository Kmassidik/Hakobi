//! App create/edit form plumbing: form model, validation, and rendering.

use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;

use super::{html_response, page_csrf, with_catalog, AppState};
use crate::web::templates;
use hakobi::catalog::validate_build_type;

#[derive(Debug, Deserialize)]
pub(crate) struct AppForm {
    pub(crate) name: String,
    pub(crate) host_id: String,
    pub(crate) build_type: String,
    #[serde(default)]
    pub(crate) github_repo: String,
    #[serde(default)]
    pub(crate) github_branch: String,
    #[serde(default)]
    pub(crate) github_path: String,
    #[serde(default)]
    pub(crate) github_dockerfile: String,
    #[serde(default)]
    pub(crate) github_auto_deploy: Option<String>,
    #[serde(default)]
    pub(crate) container_port: String,
    #[serde(default)]
    pub(crate) env_json: String,
    pub(crate) csrf_token: String,
}

pub(crate) fn form_values(form: &AppForm) -> templates::AppFormValues {
    templates::AppFormValues {
        name: form.name.trim().to_owned(),
        host_id: form.host_id.trim().to_owned(),
        build_type: form.build_type.trim().to_owned(),
        github_repo: form.github_repo.trim().to_owned(),
        github_branch: form.github_branch.trim().to_owned(),
        github_path: form.github_path.trim().to_owned(),
        github_dockerfile: if form.github_dockerfile.trim().is_empty() {
            "Dockerfile".to_owned()
        } else {
            form.github_dockerfile.trim().to_owned()
        },
        github_auto_deploy: form.github_auto_deploy.as_deref() == Some("1"),
        container_port: form.container_port.trim().to_owned(),
        env_json: if form.env_json.trim().is_empty() {
            "{}".to_owned()
        } else {
            form.env_json.trim().to_owned()
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

pub(crate) fn parse_port(value: &str) -> Option<u16> {
    value.parse::<u16>().ok().filter(|port| *port > 0)
}

pub(crate) fn validate_app_inputs(
    values: &templates::AppFormValues,
    hosts: &[hakobi::catalog::HostAgent],
) -> Result<(), String> {
    if values.name.is_empty()
        || values.name.len() > 64
        || !values
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err("App name must be 1–64 ASCII letters, digits, '-', '_', or '.'.".into());
    }
    if values.host_id.is_empty() {
        return Err("Choose an enabled host for this app.".into());
    }
    if !hosts.iter().any(|host| host.id == values.host_id) {
        return Err("The selected host is missing or disabled.".into());
    }
    validate_build_type(&values.build_type)
        .map_err(|_| "Build type must be dockerfile or nixpacks.".to_owned())?;
    if values.github_path.len() > 512 {
        return Err("Context path is too long.".into());
    }
    if values.github_dockerfile.len() > 512 {
        return Err("Dockerfile path is too long.".into());
    }
    if !values.container_port.is_empty()
        && values.container_port.parse::<u16>().map(|port| port == 0) != Ok(false)
    {
        return Err("Container port must be between 1 and 65535.".into());
    }
    Ok(())
}

pub(crate) fn display_error(error: &anyhow::Error) -> String {
    super::truncate(&error.to_string(), 500)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_app_form(
    state: &AppState,
    headers: &HeaderMap,
    hosts: &[hakobi::catalog::HostAgent],
    values: &templates::AppFormValues,
    action: &str,
    heading: &str,
    submit_label: &str,
    error: Option<&str>,
    status: StatusCode,
) -> Response {
    let (csrf_token, cookies) = page_csrf(state, headers);
    html_response(
        status,
        templates::app_form(
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

pub(crate) async fn enabled_hosts(
    state: &AppState,
) -> anyhow::Result<Vec<hakobi::catalog::HostAgent>> {
    let hosts = with_catalog(state, |catalog| catalog.list_host_agents()).await?;
    Ok(hosts.into_iter().filter(|host| host.enabled).collect())
}
