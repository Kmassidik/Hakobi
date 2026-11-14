use std::collections::BTreeMap;

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::Value;

/// Result of a successful `kubectl version`.
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub context: String,
    pub client_version: Option<String>,
    pub server_version: String,
    pub platform: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NamespaceRow {
    pub name: String,
    pub status: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeploymentRow {
    pub name: String,
    pub replicas: u32,
    pub ready: u32,
    pub available: u32,
    pub updated: u32,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ServicePort {
    pub name: Option<String>,
    pub port: u16,
    pub target_port: Option<String>,
    pub protocol: String,
    pub node_port: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct ServiceRow {
    pub name: String,
    pub service_type: String,
    pub cluster_ip: Option<String>,
    pub external_ips: Vec<String>,
    pub selector: BTreeMap<String, String>,
    pub ports: Vec<ServicePort>,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PodRow {
    pub name: String,
    pub phase: String,
    pub node: Option<String>,
    pub pod_ip: Option<String>,
    pub restarts: u32,
    pub ready_containers: u32,
    pub container_count: u32,
    pub created_at: Option<String>,
    pub started_at: Option<String>,
}

fn parse_json<'a, T: Deserialize<'a>>(stdout: &'a [u8], label: &str) -> Result<T> {
    serde_json::from_slice(stdout)
        .map_err(|error| anyhow!("could not parse kubectl {label} output as JSON: {error}"))
}

fn clamp_count(value: Option<i64>) -> u32 {
    value.unwrap_or(0).clamp(0, i64::from(u32::MAX)) as u32
}

fn value_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

#[derive(Debug, Default, Deserialize)]
struct ObjectMeta {
    #[serde(default)]
    name: String,
    #[serde(rename = "creationTimestamp", default)]
    creation_timestamp: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KubeList<T> {
    #[serde(default)]
    items: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct VersionInfo {
    #[serde(rename = "clientVersion", default)]
    client_version: Option<VersionPayload>,
    #[serde(rename = "serverVersion", default)]
    server_version: Option<VersionPayload>,
}

#[derive(Debug, Default, Deserialize)]
struct VersionPayload {
    #[serde(rename = "gitVersion", default)]
    git_version: Option<String>,
    #[serde(default)]
    major: Option<Value>,
    #[serde(default)]
    minor: Option<Value>,
    #[serde(default)]
    platform: Option<String>,
}

fn version_label(payload: &VersionPayload) -> Option<String> {
    if let Some(git_version) = payload.git_version.as_deref() {
        if !git_version.is_empty() {
            return Some(git_version.to_owned());
        }
    }
    match (
        payload.major.as_ref().and_then(value_string),
        payload.minor.as_ref().and_then(value_string),
    ) {
        (Some(major), Some(minor)) => Some(format!("{major}.{minor}")),
        (Some(major), None) => Some(major),
        (None, Some(minor)) => Some(minor),
        (None, None) => None,
    }
}

pub(super) fn connection_info(stdout: &[u8], context: &str) -> Result<ConnectionInfo> {
    let version: VersionInfo = parse_json(stdout, "version")?;
    let server = version.server_version.ok_or_else(|| {
        anyhow!("kubectl reported no server version for context {context:?}; the cluster is unreachable")
    })?;
    Ok(ConnectionInfo {
        context: context.to_owned(),
        client_version: version.client_version.as_ref().and_then(version_label),
        server_version: version_label(&server).ok_or_else(|| {
            anyhow!("kubectl returned an incomplete server version for context {context:?}")
        })?,
        platform: server.platform.filter(|platform| !platform.is_empty()),
    })
}

#[derive(Debug, Deserialize, Default)]
struct RawNamespace {
    #[serde(default)]
    metadata: ObjectMeta,
    #[serde(default)]
    status: Option<NamespaceStatus>,
}

#[derive(Debug, Deserialize)]
struct NamespaceStatus {
    #[serde(default)]
    phase: Option<String>,
}

fn namespace_row(item: RawNamespace) -> NamespaceRow {
    NamespaceRow {
        name: item.metadata.name,
        status: item
            .status
            .and_then(|status| status.phase)
            .unwrap_or_else(|| "Unknown".to_owned()),
        created_at: item.metadata.creation_timestamp,
    }
}

pub(super) fn namespaces_from_json(stdout: &[u8]) -> Result<Vec<NamespaceRow>> {
    let list: KubeList<RawNamespace> = parse_json(stdout, "namespaces")?;
    let mut rows = list
        .items
        .into_iter()
        .map(namespace_row)
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(rows)
}

#[derive(Debug, Default, Deserialize)]
struct DeploymentSpec {
    #[serde(default)]
    replicas: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct DeploymentStatus {
    #[serde(rename = "readyReplicas", default)]
    ready_replicas: Option<i64>,
    #[serde(rename = "availableReplicas", default)]
    available_replicas: Option<i64>,
    #[serde(rename = "updatedReplicas", default)]
    updated_replicas: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDeployment {
    #[serde(default)]
    metadata: ObjectMeta,
    #[serde(default)]
    spec: DeploymentSpec,
    #[serde(default)]
    status: DeploymentStatus,
}

fn deployment_row(item: RawDeployment) -> DeploymentRow {
    DeploymentRow {
        name: item.metadata.name,
        replicas: clamp_count(item.spec.replicas),
        ready: clamp_count(item.status.ready_replicas),
        available: clamp_count(item.status.available_replicas),
        updated: clamp_count(item.status.updated_replicas),
        created_at: item.metadata.creation_timestamp,
    }
}

pub(super) fn deployments_from_json(stdout: &[u8]) -> Result<Vec<DeploymentRow>> {
    let list: KubeList<RawDeployment> = parse_json(stdout, "deployments")?;
    let mut rows = list
        .items
        .into_iter()
        .map(deployment_row)
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(rows)
}

#[derive(Debug, Default, Deserialize)]
struct ServiceSpec {
    #[serde(rename = "type", default)]
    service_type: Option<String>,
    #[serde(rename = "clusterIP", default)]
    cluster_ip: Option<String>,
    #[serde(rename = "externalIPs", default)]
    external_ips: Vec<String>,
    #[serde(default)]
    selector: BTreeMap<String, String>,
    #[serde(default)]
    ports: Vec<RawServicePort>,
}

#[derive(Debug, Deserialize)]
struct RawServicePort {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(rename = "targetPort", default)]
    target_port: Option<Value>,
    #[serde(default)]
    protocol: Option<String>,
    #[serde(rename = "nodePort", default)]
    node_port: Option<u16>,
}

#[derive(Debug, Deserialize, Default)]
struct RawService {
    #[serde(default)]
    metadata: ObjectMeta,
    #[serde(default)]
    spec: ServiceSpec,
}

fn service_port(port: RawServicePort) -> ServicePort {
    ServicePort {
        name: port.name.filter(|name| !name.is_empty()),
        port: port.port.unwrap_or_default(),
        target_port: port.target_port.as_ref().and_then(value_string),
        protocol: port.protocol.unwrap_or_else(|| "TCP".to_owned()),
        node_port: port.node_port,
    }
}

fn service_row(item: RawService) -> ServiceRow {
    let ports = item.spec.ports.into_iter().map(service_port).collect();
    ServiceRow {
        name: item.metadata.name,
        service_type: item
            .spec
            .service_type
            .unwrap_or_else(|| "ClusterIP".to_owned()),
        cluster_ip: item.spec.cluster_ip.filter(|ip| !ip.is_empty()),
        external_ips: item.spec.external_ips,
        selector: item.spec.selector,
        ports,
        created_at: item.metadata.creation_timestamp,
    }
}

pub(super) fn services_from_json(stdout: &[u8]) -> Result<Vec<ServiceRow>> {
    let list: KubeList<RawService> = parse_json(stdout, "services")?;
    let mut rows = list.items.into_iter().map(service_row).collect::<Vec<_>>();
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(rows)
}

#[derive(Debug, Default, Deserialize)]
struct PodSpec {
    #[serde(rename = "nodeName", default)]
    node_name: Option<String>,
    #[serde(default)]
    containers: Vec<RawContainer>,
}

#[derive(Debug, Deserialize)]
struct RawContainer {}

#[derive(Debug, Deserialize)]
struct ContainerStatus {
    #[serde(default)]
    ready: bool,
    #[serde(rename = "restartCount", default)]
    restart_count: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct PodStatus {
    #[serde(default)]
    phase: Option<String>,
    #[serde(rename = "podIP", default)]
    pod_ip: Option<String>,
    #[serde(rename = "startTime", default)]
    start_time: Option<String>,
    #[serde(rename = "containerStatuses", default)]
    container_statuses: Vec<ContainerStatus>,
}

#[derive(Debug, Deserialize, Default)]
struct RawPod {
    #[serde(default)]
    metadata: ObjectMeta,
    #[serde(default)]
    spec: PodSpec,
    #[serde(default)]
    status: PodStatus,
}

fn pod_restarts(statuses: &[ContainerStatus]) -> u32 {
    let restarts = statuses
        .iter()
        .map(|status| {
            status
                .restart_count
                .unwrap_or(0)
                .clamp(0, i64::from(u32::MAX))
        })
        .sum::<i64>();
    restarts.clamp(0, i64::from(u32::MAX)) as u32
}

fn pod_row(item: RawPod) -> PodRow {
    let ready_containers = item
        .status
        .container_statuses
        .iter()
        .filter(|status| status.ready)
        .count() as u32;
    PodRow {
        name: item.metadata.name,
        phase: item.status.phase.unwrap_or_else(|| "Unknown".to_owned()),
        node: item.spec.node_name,
        pod_ip: item.status.pod_ip.filter(|ip| !ip.is_empty()),
        restarts: pod_restarts(&item.status.container_statuses),
        ready_containers,
        container_count: item.spec.containers.len() as u32,
        created_at: item.metadata.creation_timestamp,
        started_at: item.status.start_time,
    }
}

pub(super) fn pods_from_json(stdout: &[u8]) -> Result<Vec<PodRow>> {
    let list: KubeList<RawPod> = parse_json(stdout, "pods")?;
    let mut rows = list.items.into_iter().map(pod_row).collect::<Vec<_>>();
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(rows)
}
