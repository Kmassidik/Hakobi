use std::collections::BTreeSet;

use anyhow::{Context, Result};
use bollard::query_parameters::ListContainersOptions;
use bollard::Docker;

/// First host port handed out to deployed apps.
pub const PORT_RANGE_START: u16 = 18000;
/// Exclusive upper bound of the auto-assigned host port range.
pub const PORT_RANGE_END: u16 = 18200;

/// Picks the first host port in [`PORT_RANGE_START`]..[`PORT_RANGE_END`] that is
/// not in `used`.
///
/// When every port in the range is taken the range start is returned; callers
/// that can retry (see [`crate::providers::docker_exec::run_image`]) will still
/// attempt it.
pub fn choose_free_port(used: impl IntoIterator<Item = u16>) -> u16 {
    let used: BTreeSet<u16> = used.into_iter().collect();
    (PORT_RANGE_START..PORT_RANGE_END)
        .find(|port| !used.contains(port))
        .unwrap_or(PORT_RANGE_START)
}

pub(super) async fn used_host_ports(docker: &Docker) -> Result<BTreeSet<u16>> {
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

pub(super) fn is_port_collision(error: &bollard::errors::Error) -> bool {
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
