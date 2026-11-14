use std::collections::HashMap;

use anyhow::{bail, Context, Result};
use bollard::query_parameters::CreateContainerOptionsBuilder;
use bollard::Docker;

use super::super::endpoint::connect;
use super::super::ports::{is_port_collision, used_host_ports, PORT_RANGE_END, PORT_RANGE_START};
use super::{
    container_config, remove_options, validate_container_name, validate_container_port,
    validate_image_reference,
};

/// Result of starting a single container from an existing image.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub container_id: String,
    pub host_port: u16,
    pub name: String,
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
    env: &HashMap<String, String>,
) -> Result<RunResult> {
    validate_image_reference(image)?;
    validate_container_name(name)?;
    validate_container_port(container_port)?;
    let docker = connect(endpoint)?;
    let candidates = candidate_ports(&docker).await?;

    let mut last_collision: Option<String> = None;
    for host_port in candidates {
        let config = container_config(image, container_port, host_port, env);
        let options = CreateContainerOptionsBuilder::new().name(name).build();
        match docker.create_container(Some(options), config).await {
            Ok(created) => {
                return start_container(&docker, image, name, host_port, created.id).await;
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
    host_port: u16,
    container_id: String,
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
        return Err(error)
            .with_context(|| format!("start container `{name}` from image `{image}`"));
    }
    Ok(RunResult {
        container_id,
        host_port,
        name: name.to_owned(),
    })
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
