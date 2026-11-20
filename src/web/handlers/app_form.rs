//! App create/edit form plumbing: form model, validation, and rendering.

use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use serde::Deserialize;

use super::{html_response, page_csrf, with_catalog, AppState};
use crate::web::templates;
use hakobi::buildrunner::{validate_branch, validate_public_git_url};
use hakobi::catalog::validate_build_type;

#[derive(Debug, Deserialize)]
pub(crate) struct AppForm {
    pub(crate) name: String,
    pub(crate) host_id: String,
    pub(crate) build_type: String,
    #[serde(default)]
    pub(crate) source_kind: String,
    #[serde(default)]
    pub(crate) settings_tab: String,
    #[serde(default)]
    pub(crate) github_repo: String,
    #[serde(default)]
    pub(crate) public_git_url: String,
    #[serde(default)]
    pub(crate) public_branch: String,
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
    pub(crate) requested_host_port: String,
    #[serde(default)]
    pub(crate) cpu_limit: String,
    #[serde(default)]
    pub(crate) memory_limit_mb: String,
    #[serde(default)]
    pub(crate) env_json: String,
    #[serde(default)]
    pub(crate) start_command: String,
    #[serde(default)]
    pub(crate) release_command: String,
    #[serde(default)]
    pub(crate) volumes_json: String,
    #[serde(default)]
    pub(crate) processes_json: String,
    #[serde(default)]
    pub(crate) secrets_json: String,
    pub(crate) csrf_token: String,
}

pub(crate) fn form_values(form: &AppForm) -> templates::AppFormValues {
    let source_kind = match form.source_kind.trim() {
        "" => "connected".to_owned(),
        source => source.to_owned(),
    };
    let public = source_kind == "public" || form.github_repo.trim().starts_with("https://");
    let source_kind = if public {
        "public".to_owned()
    } else {
        source_kind
    };
    templates::AppFormValues {
        name: form.name.trim().to_owned(),
        host_id: form.host_id.trim().to_owned(),
        build_type: form.build_type.trim().to_owned(),
        source_kind,
        settings_tab: settings_tab(&form.settings_tab),
        github_repo: if public && !form.public_git_url.trim().is_empty() {
            form.public_git_url.trim().to_owned()
        } else {
            form.github_repo.trim().to_owned()
        },
        github_branch: if public && !form.public_branch.trim().is_empty() {
            form.public_branch.trim().to_owned()
        } else {
            form.github_branch.trim().to_owned()
        },
        github_path: form.github_path.trim().to_owned(),
        github_dockerfile: if form.github_dockerfile.trim().is_empty() {
            "Dockerfile".to_owned()
        } else {
            form.github_dockerfile.trim().to_owned()
        },
        github_auto_deploy: !public && form.github_auto_deploy.as_deref() == Some("1"),
        container_port: form.container_port.trim().to_owned(),
        requested_host_port: form.requested_host_port.trim().to_owned(),
        cpu_limit: form.cpu_limit.trim().to_owned(),
        memory_limit_mb: form.memory_limit_mb.trim().to_owned(),
        env_json: if form.env_json.trim().is_empty() {
            "{}".to_owned()
        } else {
            form.env_json.trim().to_owned()
        },
        start_command: form.start_command.clone(),
        release_command: form.release_command.clone(),
        volumes_json: form.volumes_json.clone(),
        processes_json: form.processes_json.clone(),
        secrets_json: form.secrets_json.clone(),
    }
}

fn edit_source_kind(app: &hakobi::catalog::App) -> &'static str {
    if app
        .github_repo
        .as_deref()
        .is_some_and(|repo| repo.starts_with("https://"))
    {
        "public"
    } else {
        "connected"
    }
}

pub(crate) fn edit_form_values(
    app: hakobi::catalog::App,
    volumes: &[hakobi::catalog::AppVolume],
    processes: &[hakobi::catalog::AppProcess],
    secret_names: &[String],
    active_tab: &str,
) -> templates::AppFormValues {
    let source_kind = edit_source_kind(&app);
    let (volumes_json, processes_json, secrets_json) =
        super::app_runtime_form::runtime_json(volumes, processes, secret_names);
    templates::AppFormValues {
        name: app.name,
        host_id: app.host_id,
        build_type: app.build_type,
        source_kind: source_kind.to_owned(),
        settings_tab: settings_tab(active_tab),
        github_repo: app.github_repo.unwrap_or_default(),
        github_branch: app.github_branch.unwrap_or_default(),
        github_path: app.github_path.unwrap_or_default(),
        github_dockerfile: app.github_dockerfile.unwrap_or_default(),
        github_auto_deploy: app.github_auto_deploy && source_kind == "connected",
        container_port: app
            .container_port
            .map(|port| port.to_string())
            .unwrap_or_default(),
        requested_host_port: app
            .requested_host_port
            .map(|port| port.to_string())
            .unwrap_or_default(),
        cpu_limit: app
            .cpu_limit
            .map(|limit| limit.to_string())
            .unwrap_or_default(),
        memory_limit_mb: app
            .memory_limit_bytes
            .map(|bytes| (bytes / (1024 * 1024)).to_string())
            .unwrap_or_default(),
        env_json: app.env_json,
        start_command: app.start_command.unwrap_or_default(),
        release_command: app.release_command.unwrap_or_default(),
        volumes_json,
        processes_json,
        secrets_json,
    }
}

pub(crate) fn settings_tab(value: &str) -> String {
    match value {
        "source" | "build" | "processes" | "data" => value.to_owned(),
        _ => "general".to_owned(),
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

pub(crate) fn resource_limits(
    values: &templates::AppFormValues,
) -> anyhow::Result<(Option<f64>, Option<u64>)> {
    let cpu = if values.cpu_limit.is_empty() {
        None
    } else {
        Some(
            values
                .cpu_limit
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("CPU limit must be a decimal number of cores."))?,
        )
    };
    let memory = if values.memory_limit_mb.is_empty() {
        None
    } else {
        let mib = values
            .memory_limit_mb
            .parse::<u64>()
            .map_err(|_| anyhow::anyhow!("Memory limit must be a whole number of MiB."))?;
        Some(
            mib.checked_mul(1024 * 1024)
                .ok_or_else(|| anyhow::anyhow!("Memory limit must not exceed 1 TiB."))?,
        )
    };
    hakobi::catalog::validate_resource_limits(cpu, memory)?;
    Ok((cpu, memory))
}

fn validate_ports(values: &templates::AppFormValues) -> Result<(), String> {
    if !values.container_port.is_empty()
        && values.container_port.parse::<u16>().map(|port| port == 0) != Ok(false)
    {
        return Err("Container port must be between 1 and 65535.".into());
    }
    if !values.requested_host_port.is_empty()
        && values
            .requested_host_port
            .parse::<u16>()
            .map(|port| port == 0)
            != Ok(false)
    {
        return Err("Host port must be between 1 and 65535.".into());
    }
    Ok(())
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
    validate_source(values)?;
    if values.github_path.len() > 512 {
        return Err("Context path is too long.".into());
    }
    if values.github_dockerfile.len() > 512 {
        return Err("Dockerfile path is too long.".into());
    }
    validate_ports(values)?;
    resource_limits(values).map_err(|error| error.to_string())?;
    let env: serde_json::Value = serde_json::from_str(&values.env_json)
        .map_err(|_| "Environment must be a valid JSON object.".to_owned())?;
    if !env.is_object() {
        return Err("Environment must be a JSON object.".into());
    }
    super::app_runtime_form::parse_runtime_options(values)?;
    Ok(())
}

fn validate_source(values: &templates::AppFormValues) -> Result<(), String> {
    match values.source_kind.as_str() {
        "connected" => return Ok(()),
        "public" => {}
        _ => return Err("Choose your repositories or a public repository.".into()),
    }
    validate_public_git_url(&values.github_repo).map_err(|_| {
        "Public repository URL must be HTTPS and contain no credentials, query, or fragment."
            .to_owned()
    })?;
    validate_branch(&values.github_branch)
        .map_err(|_| "Enter a valid branch or tag for the public repository.".to_owned())?;
    Ok(())
}

pub(crate) async fn create_app_record(
    state: &AppState,
    values: &templates::AppFormValues,
) -> anyhow::Result<String> {
    let name = values.name.clone();
    let host_id = values.host_id.clone();
    let build_type = values.build_type.clone();
    let github_repo = non_empty(&values.github_repo);
    let github_branch = non_empty(&values.github_branch);
    let github_path = non_empty(&values.github_path);
    let github_dockerfile = non_empty(&values.github_dockerfile);
    let github_auto_deploy = values.github_auto_deploy;
    let container_port = parse_port(&values.container_port);
    let requested_host_port = parse_port(&values.requested_host_port);
    let (cpu_limit, memory_limit_bytes) = resource_limits(values)?;
    let runtime = super::app_runtime_form::parse_runtime_options(values)
        .map_err(|error| anyhow::anyhow!(error))?;
    if runtime.secrets.values().any(String::is_empty) {
        anyhow::bail!("new secrets must have values");
    }
    let env_json = values.env_json.clone();
    with_catalog(state, move |catalog| {
        let app_id = catalog.create_app(
            &name,
            &host_id,
            &build_type,
            github_repo.as_deref(),
            github_branch.as_deref(),
            github_path.as_deref(),
            github_dockerfile.as_deref(),
            github_auto_deploy,
            container_port,
            requested_host_port,
            cpu_limit,
            memory_limit_bytes,
        )?;
        catalog.set_app_env_json(&app_id, &env_json)?;
        super::app_runtime_form::save_runtime(catalog, &app_id, &runtime)?;
        Ok(app_id)
    })
    .await
}

pub(crate) async fn update_app_record(
    state: &AppState,
    id: &str,
    values: &templates::AppFormValues,
) -> anyhow::Result<()> {
    let name = values.name.clone();
    let host_id = values.host_id.clone();
    let build_type = values.build_type.clone();
    let github_repo = non_empty(&values.github_repo);
    let github_branch = non_empty(&values.github_branch);
    let github_path = non_empty(&values.github_path);
    let github_dockerfile = non_empty(&values.github_dockerfile);
    let github_auto_deploy = values.github_auto_deploy;
    let container_port = parse_port(&values.container_port);
    let requested_host_port = parse_port(&values.requested_host_port);
    let (cpu_limit, memory_limit_bytes) = resource_limits(values)?;
    let runtime = super::app_runtime_form::parse_runtime_options(values)
        .map_err(|error| anyhow::anyhow!(error))?;
    let env_json = values.env_json.clone();
    let app_id = id.to_owned();
    with_catalog(state, move |catalog| {
        let host_changed = catalog
            .get_app(&app_id)?
            .is_some_and(|app| app.host_id != host_id);
        if host_changed && catalog.latest_in_flight_deployment(&app_id)?.is_some() {
            anyhow::bail!(
                "The host cannot change while a deployment is active. Cancel it or wait for it to finish."
            );
        }
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
            requested_host_port,
            cpu_limit,
            memory_limit_bytes,
            &env_json,
        )?;
        super::app_runtime_form::save_runtime(catalog, &app_id, &runtime)
    })
    .await
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
    let mut safe_values = values.clone();
    super::app_runtime_form::scrub_secrets(&mut safe_values);
    html_response(
        status,
        templates::app_form(
            &state.config.base_url,
            &state.config.username,
            &csrf_token,
            hosts,
            &safe_values,
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
