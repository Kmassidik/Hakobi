use std::collections::HashMap;

use bollard::models::{ContainerCreateBody, HostConfig, Mount, MountType, PortBinding};
use bollard::query_parameters::{RemoveContainerOptions, RemoveContainerOptionsBuilder};

use crate::providers::docker::ResourceLimits;

type PortConfiguration = (
    Option<Vec<String>>,
    Option<HashMap<String, Option<Vec<PortBinding>>>>,
);

pub(in crate::providers::docker_exec) fn container_config(
    image: &str,
    container_port: Option<u16>,
    host_port: Option<u16>,
    env: &HashMap<String, String>,
    resources: ResourceLimits,
    start_command: Option<&str>,
    volumes: &[crate::providers::ContainerVolume],
) -> ContainerCreateBody {
    let (exposed_ports, port_bindings) = port_configuration(container_port, host_port);
    ContainerCreateBody {
        image: Some(image.to_owned()),
        env: Some(environment(env)),
        cmd: shell_command(start_command),
        exposed_ports,
        host_config: Some(HostConfig {
            port_bindings,
            mounts: (!volumes.is_empty()).then(|| volume_mounts(volumes)),
            nano_cpus: resources.cpu.map(|cores| (cores * 1_000_000_000.0) as i64),
            memory: resources.memory_bytes.map(|bytes| bytes as i64),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn port_configuration(container_port: Option<u16>, host_port: Option<u16>) -> PortConfiguration {
    let (Some(container_port), Some(host_port)) = (container_port, host_port) else {
        return (None, None);
    };
    let key = format!("{container_port}/tcp");
    let binding = PortBinding {
        // Loopback only: the Cloudflare tunnel is the public entry point.
        host_ip: Some("127.0.0.1".to_owned()),
        host_port: Some(host_port.to_string()),
    };
    (
        Some(vec![key.clone()]),
        Some(HashMap::from([(key, Some(vec![binding]))])),
    )
}

pub(in crate::providers::docker_exec) fn environment(env: &HashMap<String, String>) -> Vec<String> {
    env.iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect()
}

pub(in crate::providers::docker_exec) fn shell_command(
    command: Option<&str>,
) -> Option<Vec<String>> {
    command.map(|command| vec!["/bin/sh".into(), "-c".into(), command.to_owned()])
}

pub(in crate::providers::docker_exec) fn volume_mounts(
    volumes: &[crate::providers::ContainerVolume],
) -> Vec<Mount> {
    volumes
        .iter()
        .map(|volume| Mount {
            source: Some(volume.name.clone()),
            target: Some(volume.mount_path.clone()),
            typ: Some(MountType::VOLUME),
            read_only: Some(volume.read_only),
            ..Default::default()
        })
        .collect()
}

pub(in crate::providers::docker_exec) fn remove_options() -> RemoveContainerOptions {
    RemoveContainerOptionsBuilder::new()
        .force(true)
        .v(false)
        .build()
}
