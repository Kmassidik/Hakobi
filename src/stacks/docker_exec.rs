//! Docker execution primitives for app lifecycle operations.
//!
//! This module only talks to a Docker daemon (or the Docker Compose CLI) at an
//! explicit endpoint. It never pulls images, shells out with user-controlled
//! fragments, or removes volumes.

use std::collections::{BTreeSet, HashMap};
use std::io::Write as _;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, ListContainersOptions, RemoveContainerOptions,
    RemoveContainerOptionsBuilder,
};
use bollard::{Docker, API_DEFAULT_VERSION};

/// First host port handed out to deployed apps.
pub const PORT_RANGE_START: u16 = 18000;
/// Exclusive upper bound of the auto-assigned host port range.
pub const PORT_RANGE_END: u16 = 18200;
const CONNECT_TIMEOUT_SECS: u64 = 8;
const MAX_IMAGE_LEN: usize = 255;
const MAX_NAME_LEN: usize = 128;
const MAX_PROJECT_LEN: usize = 63;
const MAX_COMPOSE_BYTES: usize = 1024 * 1024;
const STDERR_EXCERPT_CHARS: usize = 4000;

/// Result of starting a single container from an existing image.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub container_id: String,
    pub host_port: u16,
    pub name: String,
}

/// Connects to the Docker daemon behind `endpoint`, which must be a
/// `unix:///absolute/path` socket or a `tcp://host:port` address.
fn connect(endpoint: &str) -> Result<Docker> {
    let endpoint = endpoint.trim();
    crate::providers::docker::validate_endpoint(endpoint)?;
    if let Some(path) = endpoint.strip_prefix("unix://") {
        return Docker::connect_with_unix(path, CONNECT_TIMEOUT_SECS, API_DEFAULT_VERSION)
            .context("connect to Docker Unix socket");
    }
    let host = endpoint
        .strip_prefix("tcp://")
        .context("invalid Docker endpoint")?;
    Docker::connect_with_http(
        &format!("http://{host}"),
        CONNECT_TIMEOUT_SECS,
        API_DEFAULT_VERSION,
    )
    .context("connect to Docker TCP endpoint")
}

/// Picks the first host port in [`PORT_RANGE_START`]..[`PORT_RANGE_END`] that is
/// not in `used`.
///
/// When every port in the range is taken the range start is returned; callers
/// that can retry (see [`run_image`]) will still attempt it.
pub fn choose_free_port(used: impl IntoIterator<Item = u16>) -> u16 {
    let used: BTreeSet<u16> = used.into_iter().collect();
    (PORT_RANGE_START..PORT_RANGE_END)
        .find(|port| !used.contains(port))
        .unwrap_or(PORT_RANGE_START)
}

/// Creates and starts a container from an already-present image.
///
/// No pull happens: if the image is missing locally Docker rejects the create
/// call and that error is returned. The host port is chosen automatically from
/// `18000..18200`, retrying the next candidate when Docker reports that a port
/// is already allocated.
pub async fn run_image(
    endpoint: &str,
    image: &str,
    name: &str,
    container_port: u16,
) -> Result<RunResult> {
    validate_image_reference(image)?;
    validate_container_name(name)?;
    validate_container_port(container_port)?;
    let docker = connect(endpoint)?;

    let used = used_host_ports(&docker).await?;
    let mut candidates: Vec<u16> = (PORT_RANGE_START..PORT_RANGE_END)
        .filter(|port| !used.contains(port))
        .collect();
    if candidates.is_empty() {
        candidates.extend(PORT_RANGE_START..PORT_RANGE_END);
    }

    let mut last_collision: Option<String> = None;
    for host_port in candidates {
        let config = container_config(image, container_port, host_port);
        let options = CreateContainerOptionsBuilder::new().name(name).build();
        match docker.create_container(Some(options), config).await {
            Ok(created) => {
                let container_id = created.id;
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
                    return Err(error)
                        .with_context(|| format!("start container `{name}` from image `{image}`"));
                }
                return Ok(RunResult {
                    container_id,
                    host_port,
                    name: name.to_owned(),
                });
            }
            Err(error) if is_port_collision(&error) => {
                last_collision = Some(format!("host port {host_port}: {error}"));
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("create container `{name}` from image `{image}` (image must already exist locally)")
                });
            }
        }
    }

    let detail = last_collision.unwrap_or_else(|| "all candidate ports are in use".to_owned());
    bail!("no free host port in {PORT_RANGE_START}–{PORT_RANGE_END} for image `{image}`: {detail}")
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
    let endpoint = endpoint.to_owned();
    let yaml = compose_yaml.to_owned();
    let output = tokio::task::spawn_blocking(move || -> Result<std::process::Output> {
        let mut child = Command::new("docker")
            .args(&args)
            .env("DOCKER_HOST", &endpoint)
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
    })
    .await
    .context("join `docker compose` task")??;

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

fn compose_up_args(project: &str) -> Vec<String> {
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

fn container_config(image: &str, container_port: u16, host_port: u16) -> ContainerCreateBody {
    let key = format!("{container_port}/tcp");
    let binding = PortBinding {
        host_ip: Some("0.0.0.0".to_owned()),
        host_port: Some(host_port.to_string()),
    };
    let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
    port_bindings.insert(key.clone(), Some(vec![binding]));
    ContainerCreateBody {
        image: Some(image.to_owned()),
        exposed_ports: Some(vec![key]),
        host_config: Some(HostConfig {
            port_bindings: Some(port_bindings),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn remove_options() -> RemoveContainerOptions {
    RemoveContainerOptionsBuilder::new()
        .force(true)
        .v(false)
        .build()
}

async fn used_host_ports(docker: &Docker) -> Result<BTreeSet<u16>> {
    let options = ListContainersOptions {
        all: true,
        ..Default::default()
    };
    let containers = docker
        .list_containers(Some(options))
        .await
        .context("list Docker containers")?;
    Ok(containers
        .into_iter()
        .flat_map(|container| container.ports.unwrap_or_default())
        .filter_map(|port| port.public_port)
        .collect())
}

fn is_port_collision(error: &bollard::errors::Error) -> bool {
    let bollard::errors::Error::DockerResponseServerError {
        status_code,
        message,
    } = error
    else {
        return false;
    };
    if *status_code != 400 && *status_code != 409 && *status_code != 500 {
        return false;
    }
    let message = message.to_ascii_lowercase();
    [
        "port is already allocated",
        "address already in use",
        "ports are not available",
        "port already in use",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn validate_image_reference(image: &str) -> Result<()> {
    if image.is_empty() || image.len() > MAX_IMAGE_LEN {
        bail!("image reference must be 1–{MAX_IMAGE_LEN} characters");
    }
    if image
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        bail!("image reference must not contain whitespace or control characters");
    }
    let mut characters = image.chars();
    match characters.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => bail!("image reference must start with a letter or digit"),
    }
    if !image.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/' | ':' | '@')
    }) {
        bail!("image reference contains unsupported characters");
    }
    if image.contains("..") || image.contains("//") || image.contains("@@") {
        bail!("image reference contains an invalid separator sequence");
    }
    Ok(())
}

fn validate_container_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        bail!("container name must be 1–{MAX_NAME_LEN} characters");
    }
    match name.chars().next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => bail!("container name must start with a letter or digit"),
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        bail!("container name may only contain letters, digits, '.', '_' and '-'");
    }
    Ok(())
}

fn validate_container_port(container_port: u16) -> Result<()> {
    if container_port == 0 {
        bail!("container port must be between 1 and 65535");
    }
    Ok(())
}

fn validate_compose_project(project: &str) -> Result<()> {
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

fn validate_compose_yaml(compose_yaml: &str) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_port_starts_at_range_start() {
        assert_eq!(choose_free_port(std::iter::empty()), PORT_RANGE_START);
    }

    #[test]
    fn free_port_skips_used_ports() {
        assert_eq!(choose_free_port([18000, 18002]), 18001);
        assert_eq!(choose_free_port(18000..18197), 18197);
    }

    #[test]
    fn free_port_ignores_out_of_range_ports() {
        assert_eq!(choose_free_port([1, 2, 65535]), PORT_RANGE_START);
    }

    #[test]
    fn free_port_falls_back_to_start_when_range_is_full() {
        assert_eq!(
            choose_free_port(PORT_RANGE_START..PORT_RANGE_END),
            PORT_RANGE_START
        );
    }

    #[test]
    fn image_references_are_validated() {
        for image in [
            "nginx",
            "nginx:1.27",
            "ghcr.io/acme/app:v1.2.3",
            "localhost:5000/team/app@sha256:abc123",
        ] {
            assert!(validate_image_reference(image).is_ok(), "{image}");
        }
        for image in [
            "",
            "-rm",
            "nginx latest",
            "nginx;rm",
            "nginx\nlatest",
            "../app",
            "app//x",
        ] {
            assert!(validate_image_reference(image).is_err(), "{image}");
        }
    }

    #[test]
    fn container_names_are_tightly_scoped() {
        for name in ["web", "web-1", "web_1.blue", "1web"] {
            assert!(validate_container_name(name).is_ok(), "{name}");
        }
        for name in ["", "-web", "web/evil", "web evil", "../web"] {
            assert!(validate_container_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn container_ports_exclude_zero() {
        assert!(validate_container_port(0).is_err());
        assert!(validate_container_port(1).is_ok());
        assert!(validate_container_port(80).is_ok());
        assert!(validate_container_port(u16::MAX).is_ok());
    }

    #[test]
    fn compose_project_is_validated() {
        for project in ["app", "my_app", "app-2", "1app"] {
            assert!(validate_compose_project(project).is_ok(), "{project}");
        }
        for project in ["", "MyApp", "-app", "app two", "app/2"] {
            assert!(validate_compose_project(project).is_err(), "{project}");
        }
    }

    #[test]
    fn compose_file_is_validated() {
        assert!(validate_compose_yaml("services: {}").is_ok());
        assert!(validate_compose_yaml("   ").is_err());
        assert!(validate_compose_yaml("services:\0").is_err());
        assert!(validate_compose_yaml(&"a".repeat(MAX_COMPOSE_BYTES + 1)).is_err());
    }

    #[test]
    fn compose_command_is_fixed_and_uses_stdin() {
        assert_eq!(
            compose_up_args("myapp"),
            vec![
                "compose",
                "-f",
                "-",
                "-p",
                "myapp",
                "up",
                "-d",
                "--remove-orphans"
            ]
        );
    }

    #[test]
    fn compose_command_keeps_project_as_one_argument() {
        let args = compose_up_args("app-x");
        assert_eq!(args.iter().filter(|arg| *arg == "app-x").count(), 1);
        assert!(!args.iter().any(|arg| arg.contains(' ')));
    }

    #[test]
    fn port_collision_messages_are_recognized() {
        let collision = bollard::errors::Error::DockerResponseServerError {
            status_code: 500,
            message: "Bind for 0.0.0.0:18000 failed: port is already allocated".into(),
        };
        assert!(is_port_collision(&collision));

        let conflict = bollard::errors::Error::DockerResponseServerError {
            status_code: 409,
            message: "Conflict. The container name is already in use".into(),
        };
        assert!(!is_port_collision(&conflict));

        let missing_image = bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            message: "No such image: nginx".into(),
        };
        assert!(!is_port_collision(&missing_image));
    }

    #[test]
    fn remove_options_keep_volumes() {
        let options = remove_options();
        assert!(options.force);
        assert!(!options.v);
    }

    #[tokio::test]
    async fn invalid_inputs_fail_before_connecting() {
        let error = run_image("tcp://127.0.0.1:1", "bad image", "web", 80)
            .await
            .expect_err("invalid image");
        assert!(error.to_string().contains("image reference"), "{error}");

        let error = run_image("tcp://127.0.0.1:1", "nginx", "bad name", 80)
            .await
            .expect_err("invalid name");
        assert!(error.to_string().contains("container name"), "{error}");

        let error = run_image("tcp://127.0.0.1:1", "nginx", "web", 0)
            .await
            .expect_err("invalid port");
        assert!(error.to_string().contains("container port"), "{error}");
    }

    #[tokio::test]
    async fn compose_inputs_fail_before_invoking_docker() {
        let error = apply_compose("unix:///var/run/docker.sock", "Bad Project", "services: {}")
            .await
            .expect_err("invalid project");
        assert!(error.to_string().contains("Compose project"), "{error}");

        let error = apply_compose("unix:///var/run/docker.sock", "myapp", "  ")
            .await
            .expect_err("empty compose file");
        assert!(error.to_string().contains("must not be empty"), "{error}");
    }

    #[test]
    fn container_config_binds_exposed_port() {
        let config = container_config("nginx", 80, 18000);
        assert_eq!(config.image.as_deref(), Some("nginx"));
        assert_eq!(
            config.exposed_ports.as_deref(),
            Some(&["80/tcp".to_owned()][..])
        );
        let host = config.host_config.expect("host config");
        let binding = host
            .port_bindings
            .expect("port bindings")
            .get("80/tcp")
            .cloned()
            .flatten()
            .expect("binding");
        assert_eq!(binding.len(), 1);
        assert_eq!(binding[0].host_port.as_deref(), Some("18000"));
    }
}
