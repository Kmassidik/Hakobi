use std::collections::HashMap;

use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{RemoveContainerOptions, RemoveContainerOptionsBuilder};

pub(in crate::providers::docker_exec) fn container_config(
    image: &str,
    container_port: u16,
    host_port: u16,
    env: &HashMap<String, String>,
) -> ContainerCreateBody {
    let key = format!("{container_port}/tcp");
    let binding = PortBinding {
        // Loopback only: the Cloudflare tunnel is the public entry point.
        host_ip: Some("127.0.0.1".to_owned()),
        host_port: Some(host_port.to_string()),
    };
    let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
    port_bindings.insert(key.clone(), Some(vec![binding]));
    let env_list: Vec<String> = env
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    ContainerCreateBody {
        image: Some(image.to_owned()),
        env: Some(env_list),
        exposed_ports: Some(vec![key]),
        host_config: Some(HostConfig {
            port_bindings: Some(port_bindings),
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub(in crate::providers::docker_exec) fn remove_options() -> RemoveContainerOptions {
    RemoveContainerOptionsBuilder::new()
        .force(true)
        .v(false)
        .build()
}
