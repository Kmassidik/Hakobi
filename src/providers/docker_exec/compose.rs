use std::io::Write as _;
use std::process::{Command, Output, Stdio};

use anyhow::{bail, Context, Result};

pub(super) const MAX_COMPOSE_BYTES: usize = 1024 * 1024;
const MAX_PROJECT_LEN: usize = 63;
const STDERR_EXCERPT_CHARS: usize = 4000;

/// Applies a Compose project through the Docker Compose CLI, reading the
/// Compose file from stdin.
///
/// Requires the Compose v2 plugin (`docker compose`). Returns the command's
/// standard output on success.
pub async fn apply_compose(endpoint: &str, project: &str, compose_yaml: &str) -> Result<String> {
    let endpoint = endpoint.trim();
    crate::providers::docker::validate_endpoint(endpoint)?;
    validate_compose_project(project)?;
    validate_compose_yaml(compose_yaml)?;

    if !compose_available().await {
        bail!(
            "Docker Compose is not available; install the Docker Compose v2 plugin so \
             `docker compose version` succeeds"
        );
    }

    let args = compose_up_args(project);
    let output = run_compose(args, endpoint.to_owned(), compose_yaml.to_owned()).await?;
    compose_output(output)
}

/// Reports whether the Docker Compose v2 plugin can be invoked.
pub async fn compose_available() -> bool {
    match tokio::process::Command::new("docker")
        .args(["compose", "version"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

async fn run_compose(args: Vec<String>, endpoint: String, yaml: String) -> Result<Output> {
    tokio::task::spawn_blocking(move || spawn_compose(&args, &endpoint, &yaml))
        .await
        .context("join `docker compose` task")?
}

fn spawn_compose(args: &[String], endpoint: &str, yaml: &str) -> Result<Output> {
    let mut child = Command::new("docker")
        .args(args)
        .env("DOCKER_HOST", endpoint)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn `docker compose`")?;
    child
        .stdin
        .take()
        .context("open `docker compose` stdin")?
        .write_all(yaml.as_bytes())
        .context("write Compose file to `docker compose` stdin")?;
    child
        .wait_with_output()
        .context("wait for `docker compose`")
}

fn compose_output(output: Output) -> Result<String> {
    if !output.status.success() {
        let stderr = stderr_excerpt(&output.stderr);
        let status = output
            .status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".to_owned());
        bail!("`docker compose up` failed (exit {status}): {stderr}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(super) fn compose_up_args(project: &str) -> Vec<String> {
    [
        "compose",
        "-f",
        "-",
        "-p",
        project,
        "up",
        "-d",
        "--remove-orphans",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(super) fn validate_compose_project(project: &str) -> Result<()> {
    if project.is_empty() || project.len() > MAX_PROJECT_LEN {
        bail!("Compose project name must be 1–{MAX_PROJECT_LEN} characters");
    }
    match project.chars().next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => bail!("Compose project name must start with a lowercase letter or digit"),
    }
    if !project.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '_' | '-')
    }) {
        bail!("Compose project name may only contain lowercase letters, digits, '_' and '-'");
    }
    Ok(())
}

pub(super) fn validate_compose_yaml(compose_yaml: &str) -> Result<()> {
    if compose_yaml.trim().is_empty() {
        bail!("Compose file must not be empty");
    }
    if compose_yaml.len() > MAX_COMPOSE_BYTES {
        bail!("Compose file must not exceed {MAX_COMPOSE_BYTES} bytes");
    }
    if compose_yaml.contains('\0') {
        bail!("Compose file must not contain NUL bytes");
    }
    Ok(())
}

fn stderr_excerpt(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "no error output".to_owned();
    }
    let tail: Vec<char> = trimmed.chars().rev().take(STDERR_EXCERPT_CHARS).collect();
    tail.into_iter().rev().collect()
}
