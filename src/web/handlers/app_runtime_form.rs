use std::collections::{BTreeMap, HashMap};

use hakobi::catalog::{validate_runtime_options, validate_secret_name, AppProcess, AppVolume};

use crate::web::templates::AppFormValues;

#[derive(Clone)]
pub(crate) struct RuntimeOptions {
    pub start_command: Option<String>,
    pub release_command: Option<String>,
    pub volumes: Vec<AppVolume>,
    pub processes: Vec<AppProcess>,
    pub secrets: BTreeMap<String, String>,
}

fn optional_command(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub(crate) fn parse_runtime_options(values: &AppFormValues) -> Result<RuntimeOptions, String> {
    let options = RuntimeOptions {
        start_command: optional_command(&values.start_command),
        release_command: optional_command(&values.release_command),
        volumes: parse_json(&values.volumes_json, "volumes")?,
        processes: parse_json(&values.processes_json, "processes")?,
        secrets: parse_json(&values.secrets_json, "secrets")?,
    };
    validate_runtime_options(
        options.start_command.as_deref(),
        options.release_command.as_deref(),
        &options.volumes,
        &options.processes,
    )
    .map_err(|error| error.to_string())?;
    validate_secret_conflicts(values, &options.secrets)?;
    Ok(options)
}

fn parse_json<T: serde::de::DeserializeOwned>(value: &str, label: &str) -> Result<T, String> {
    serde_json::from_str(if value.trim().is_empty() {
        if label == "secrets" {
            "{}"
        } else {
            "[]"
        }
    } else {
        value
    })
    .map_err(|_| format!("Invalid {label} definition."))
}

fn validate_secret_conflicts(
    values: &AppFormValues,
    secrets: &BTreeMap<String, String>,
) -> Result<(), String> {
    let environment: HashMap<String, String> = serde_json::from_str(&values.env_json)
        .map_err(|_| "Environment must be a valid JSON object.".to_owned())?;
    for name in secrets.keys() {
        validate_secret_name(name).map_err(|error| error.to_string())?;
        if environment.contains_key(name) {
            return Err(format!("{name} cannot be both a variable and a secret."));
        }
    }
    Ok(())
}

pub(crate) fn save_runtime(
    catalog: &mut hakobi::catalog::Catalog,
    app_id: &str,
    options: &RuntimeOptions,
) -> anyhow::Result<()> {
    catalog.set_app_runtime(
        app_id,
        options.start_command.as_deref(),
        options.release_command.as_deref(),
        &options.volumes,
        &options.processes,
    )?;
    catalog.set_app_secrets(app_id, &options.secrets)
}

pub(crate) fn runtime_json(
    volumes: &[AppVolume],
    processes: &[AppProcess],
    secret_names: &[String],
) -> (String, String, String) {
    let secrets = secret_names
        .iter()
        .map(|name| (name, ""))
        .collect::<BTreeMap<_, _>>();
    (
        serde_json::to_string(volumes).unwrap_or_else(|_| "[]".into()),
        serde_json::to_string(processes).unwrap_or_else(|_| "[]".into()),
        serde_json::to_string(&secrets).unwrap_or_else(|_| "{}".into()),
    )
}

pub(crate) fn scrub_secrets(values: &mut AppFormValues) {
    if let Ok(mut secrets) = serde_json::from_str::<BTreeMap<String, String>>(&values.secrets_json)
    {
        secrets.values_mut().for_each(|value| value.clear());
        values.secrets_json = serde_json::to_string(&secrets).unwrap_or_else(|_| "{}".into());
    }
}
