use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use bollard::container::LogOutput;
use bollard::models::ContainerCreateBody;
use bollard::query_parameters::{CreateContainerOptionsBuilder, LogsOptionsBuilder};
use bollard::Docker;
use futures_util::{Stream, StreamExt};

use crate::providers::docker::{validate_shell_command, validate_volumes, ResourceLimits};
use crate::providers::ContainerVolume;

use super::super::endpoint::connect;
use super::super::ports::{is_port_collision, used_host_ports, PORT_RANGE_END, PORT_RANGE_START};
use super::{
    container_config, environment, remove_options, shell_command, validate_container_name,
    validate_container_port, validate_image_reference, volume_mounts,
};

pub(crate) const MAX_RELEASE_LOG_BYTES: usize = 64 * 1024;

/// Result of starting a single container from an existing image.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub container_id: String,
    pub host_port: Option<u16>,
    pub name: String,
}

/// Completed one-shot release command. Nonzero exit codes are returned here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleaseResult {
    pub exit_code: i64,
    pub logs: String,
}

/// Creates and starts a web container or a private worker from a local image.
#[allow(clippy::too_many_arguments)]
pub async fn run_image(
    endpoint: &str,
    image: &str,
    name: &str,
    container_port: Option<u16>,
    requested_host_port: Option<u16>,
    env: &HashMap<String, String>,
    resources: ResourceLimits,
    start_command: Option<&str>,
    volumes: &[ContainerVolume],
) -> Result<RunResult> {
    validate_run_input(
        image,
        name,
        container_port,
        requested_host_port,
        &resources,
        start_command,
        volumes,
    )?;
    let docker = connect(endpoint)?;
    let candidates = host_port_candidates(&docker, container_port, requested_host_port).await?;
    let mut last_collision: Option<String> = None;
    for host_port in candidates {
        match run_candidate(
            &docker,
            image,
            name,
            container_port,
            host_port,
            requested_host_port.is_some(),
            env,
            resources,
            start_command,
            volumes,
        )
        .await?
        {
            RunAttempt::Started(result) => return Ok(result),
            RunAttempt::Collision(detail) => last_collision = Some(detail),
        }
    }
    let detail = last_collision.unwrap_or_else(|| "all candidate ports are in use".to_owned());
    bail!("no free host port in {PORT_RANGE_START}–{PORT_RANGE_END} for image `{image}`: {detail}")
}

enum RunAttempt {
    Started(RunResult),
    Collision(String),
}

#[allow(clippy::too_many_arguments)]
async fn run_candidate(
    docker: &Docker,
    image: &str,
    name: &str,
    container_port: Option<u16>,
    host_port: Option<u16>,
    requested: bool,
    env: &HashMap<String, String>,
    resources: ResourceLimits,
    start_command: Option<&str>,
    volumes: &[ContainerVolume],
) -> Result<RunAttempt> {
    let created = create_run_container(
        docker,
        image,
        name,
        container_port,
        host_port,
        env,
        resources,
        start_command,
        volumes,
    )
    .await;
    match created {
        Ok(id) => start_container(docker, image, name, host_port, id, requested)
            .await
            .map(RunAttempt::Started),
        Err(error) if is_port_collision(&error) && requested => {
            bail!(
                "requested host port {} is unavailable: {error}",
                host_port.unwrap_or(0)
            )
        }
        Err(error) if is_port_collision(&error) => Ok(RunAttempt::Collision(format!(
            "host port {host_port:?}: {error}"
        ))),
        Err(error) => Err(error).with_context(|| {
            format!(
                "create container `{name}` from image `{image}` (image must already exist locally)"
            )
        }),
    }
}

#[allow(clippy::too_many_arguments)]
async fn create_run_container(
    docker: &Docker,
    image: &str,
    name: &str,
    container_port: Option<u16>,
    host_port: Option<u16>,
    env: &HashMap<String, String>,
    resources: ResourceLimits,
    start_command: Option<&str>,
    volumes: &[ContainerVolume],
) -> std::result::Result<String, bollard::errors::Error> {
    let config = container_config(
        image,
        container_port,
        host_port,
        env,
        resources,
        start_command,
        volumes,
    );
    let options = CreateContainerOptionsBuilder::new().name(name).build();
    docker
        .create_container(Some(options), config)
        .await
        .map(|created| created.id)
}

#[allow(clippy::too_many_arguments)]
fn validate_run_input(
    image: &str,
    name: &str,
    container_port: Option<u16>,
    requested_host_port: Option<u16>,
    resources: &ResourceLimits,
    start_command: Option<&str>,
    volumes: &[ContainerVolume],
) -> Result<()> {
    validate_image_reference(image)?;
    validate_container_name(name)?;
    if let Some(port) = container_port {
        validate_container_port(port)?;
    }
    if container_port.is_none() && requested_host_port.is_some() {
        bail!("a host port requires a container port");
    }
    if let Some(host_port) = requested_host_port {
        validate_container_port(host_port).context("invalid requested host port")?;
    }
    if let Some(command) = start_command {
        validate_shell_command(command)?;
    }
    validate_volumes(volumes)?;
    resources.validate()
}

async fn host_port_candidates(
    docker: &Docker,
    container_port: Option<u16>,
    requested_host_port: Option<u16>,
) -> Result<Vec<Option<u16>>> {
    if container_port.is_none() {
        return Ok(vec![None]);
    }
    if let Some(host_port) = requested_host_port {
        return Ok(vec![Some(host_port)]);
    }
    Ok(candidate_ports(docker)
        .await?
        .into_iter()
        .map(Some)
        .collect())
}

async fn candidate_ports(docker: &Docker) -> Result<Vec<u16>> {
    let used = used_host_ports(docker).await?;
    let mut candidates: Vec<u16> = (PORT_RANGE_START..PORT_RANGE_END)
        .filter(|port| !used.contains(port))
        .collect();
    if candidates.is_empty() {
        candidates.extend(PORT_RANGE_START..PORT_RANGE_END);
    }
    Ok(candidates)
}

async fn start_container(
    docker: &Docker,
    image: &str,
    name: &str,
    host_port: Option<u16>,
    container_id: String,
    requested: bool,
) -> Result<RunResult> {
    if let Err(error) = docker.start_container(&container_id, None).await {
        if let Err(cleanup) = docker
            .remove_container(&container_id, Some(remove_options()))
            .await
        {
            tracing::warn!(
                container = %container_id,
                "failed to remove container after start failure: {cleanup}"
            );
        }
        if requested && is_port_collision(&error) {
            return Err(error).with_context(|| {
                format!(
                    "requested host port {} is unavailable",
                    host_port.unwrap_or(0)
                )
            });
        }
        return Err(error)
            .with_context(|| format!("start container `{name}` from image `{image}`"));
    }
    Ok(RunResult {
        container_id,
        host_port,
        name: name.to_owned(),
    })
}

/// Runs an auto-removed release container and returns its exit code and logs.
pub async fn release(
    endpoint: &str,
    image: &str,
    name: &str,
    env: &HashMap<String, String>,
    volumes: &[ContainerVolume],
    command: &str,
) -> Result<ReleaseResult> {
    validate_image_reference(image)?;
    validate_container_name(name)?;
    validate_volumes(volumes)?;
    validate_shell_command(command)?;
    if command.is_empty() {
        bail!("release command must not be empty");
    }
    let docker = connect(endpoint)?;
    let id = create_release_container(&docker, image, name, env, volumes, command).await?;
    wait_for_release(&docker, name, &id).await
}

async fn create_release_container(
    docker: &Docker,
    image: &str,
    name: &str,
    env: &HashMap<String, String>,
    volumes: &[ContainerVolume],
    command: &str,
) -> Result<String> {
    let config = ContainerCreateBody {
        image: Some(image.to_owned()),
        env: Some(environment(env)),
        cmd: shell_command(Some(command)),
        host_config: Some(bollard::models::HostConfig {
            auto_remove: Some(true),
            mounts: (!volumes.is_empty()).then(|| volume_mounts(volumes)),
            ..Default::default()
        }),
        ..Default::default()
    };
    let options = CreateContainerOptionsBuilder::new().name(name).build();
    let created = docker
        .create_container(Some(options), config)
        .await
        .with_context(|| format!("create release container `{name}` from image `{image}`"))?;
    if let Err(error) = docker.start_container(&created.id, None).await {
        let _ = docker
            .remove_container(&created.id, Some(remove_options()))
            .await;
        return Err(error).with_context(|| format!("start release container `{name}`"));
    }
    Ok(created.id)
}

async fn wait_for_release(docker: &Docker, name: &str, id: &str) -> Result<ReleaseResult> {
    let options = LogsOptionsBuilder::new()
        .follow(true)
        .stdout(true)
        .stderr(true)
        .build();
    let logs = collect_release_logs(docker.logs(id, Some(options)));
    let mut wait_stream = docker.wait_container(id, None);
    let wait = wait_stream.next();
    let (logs, waited) = tokio::join!(logs, wait);
    let logs = logs.context("read release container logs")?;
    let waited = match waited.context("wait for release container response")? {
        Ok(waited) => waited,
        Err(bollard::errors::Error::DockerContainerWaitError { code, .. }) => {
            return Ok(ReleaseResult {
                exit_code: code,
                logs,
            });
        }
        Err(error) => {
            return Err(error).with_context(|| format!("wait for release container `{name}`"));
        }
    };
    if let Some(error) = waited.error {
        bail!(error
            .message
            .unwrap_or_else(|| "Docker failed while waiting for the release command".into()));
    }
    Ok(ReleaseResult {
        exit_code: waited.status_code,
        logs,
    })
}

async fn collect_release_logs(
    mut stream: impl Stream<Item = Result<LogOutput, bollard::errors::Error>> + Unpin,
) -> Result<String> {
    let mut output = String::new();
    while let Some(chunk) = stream.next().await {
        let text = chunk?.to_string();
        let remaining = MAX_RELEASE_LOG_BYTES - output.len();
        if text.len() <= remaining {
            output.push_str(&text);
            continue;
        }
        let mut end = remaining;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&text[..end]);
        break;
    }
    Ok(output)
}

/// Removes a named container, stopping it first if needed.
///
/// Volumes attached to the container are left untouched.
pub async fn remove_container(endpoint: &str, name: &str) -> Result<()> {
    validate_container_name(name)?;
    let docker = connect(endpoint)?;
    docker
        .remove_container(name, Some(remove_options()))
        .await
        .with_context(|| format!("remove container `{name}`"))
}
