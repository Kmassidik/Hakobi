//! Kubernetes provider backed by the `kubectl` binary.
//!
//! Every cluster operation is a direct `kubectl` invocation (argv only, never a
//! shell) carrying an explicit `--kubeconfig` and `--context`, so the caller's
//! ambient kubectl configuration is never consulted. Kubeconfig bytes are never
//! read, logged, or copied outside the file itself; only its path is passed on.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use tempfile::NamedTempFile;
use tokio::process::Command;

const KUBECTL: &str = "kubectl";

/// Default deadline for short cluster queries.
const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
/// Applying manifests can legitimately exceed the query deadline.
const APPLY_TIMEOUT: Duration = Duration::from_secs(120);
/// Longest DNS-1123 style name we accept.
const MAX_SEGMENT_LEN: usize = 253;

/// A kubeconfig + context pair identifying exactly one cluster.
#[derive(Debug, Clone)]
pub struct KubernetesProvider {
    kubeconfig: PathBuf,
    context: String,
}

impl KubernetesProvider {
    /// Builds a provider after verifying the kubeconfig is a readable file and
    /// the context is plausible. No cluster contact happens here.
    pub fn new(kubeconfig: impl Into<PathBuf>, context: impl Into<String>) -> Result<Self> {
        let provider = Self {
            kubeconfig: kubeconfig.into(),
            context: context.into(),
        };
        provider.validate()?;
        Ok(provider)
    }

    pub fn kubeconfig(&self) -> &Path {
        &self.kubeconfig
    }

    pub fn context(&self) -> &str {
        &self.context
    }

    /// Validates the stored connection settings again, useful before a long
    /// running operation in case the kubeconfig was removed meanwhile.
    pub fn validate(&self) -> Result<()> {
        validate_context(&self.context)?;
        validate_kubeconfig(&self.kubeconfig)
    }

    /// Runs `kubectl version --output=json` and reports client/server versions.
    pub async fn test_connection(&self) -> Result<ConnectionInfo> {
        let stdout = self.run("version", version_args()).await?;
        connection_info(&stdout, &self.context)
    }

    pub async fn list_namespaces(&self) -> Result<Vec<NamespaceRow>> {
        let stdout = self
            .run("get namespaces", self.get_args("namespaces", None))
            .await?;
        namespaces_from_json(&stdout)
    }

    pub async fn list_deployments(&self, namespace: &str) -> Result<Vec<DeploymentRow>> {
        validate_namespace(namespace)?;
        let stdout = self
            .run(
                "get deployments",
                self.get_args("deployments", Some(namespace)),
            )
            .await?;
        deployments_from_json(&stdout)
    }

    pub async fn list_services(&self, namespace: &str) -> Result<Vec<ServiceRow>> {
        validate_namespace(namespace)?;
        let stdout = self
            .run("get services", self.get_args("services", Some(namespace)))
            .await?;
        services_from_json(&stdout)
    }

    pub async fn list_pods(&self, namespace: &str) -> Result<Vec<PodRow>> {
        validate_namespace(namespace)?;
        let stdout = self
            .run("get pods", self.get_args("pods", Some(namespace)))
            .await?;
        pods_from_json(&stdout)
    }

    /// Applies a manifest bundle. The YAML is written to a private (0600)
    /// temporary file which is removed once kubectl exits.
    pub async fn apply(&self, namespace: &str, yaml: &str) -> Result<()> {
        validate_namespace(namespace)?;
        if yaml.trim().is_empty() {
            bail!("manifest must not be empty");
        }
        let manifest = manifest_file(yaml)?;
        let args = apply_args(namespace, manifest.path());
        let mut command = self.command();
        command.args(&args);
        run_command(command, "apply", APPLY_TIMEOUT).await?;
        Ok(())
    }

    /// Deletes one named resource of an explicit kind inside a namespace.
    pub async fn delete(&self, namespace: &str, resource: &str, name: &str) -> Result<()> {
        validate_namespace(namespace)?;
        validate_segment("resource", resource)?;
        validate_segment("name", name)?;
        let label = format!("delete {resource}");
        self.run(&label, delete_args(namespace, resource, name))
            .await?;
        Ok(())
    }

    /// Base argv shared by every invocation: pinned kubeconfig and context.
    fn connection_args(&self) -> Vec<OsString> {
        let mut kubeconfig = OsString::from("--kubeconfig=");
        kubeconfig.push(self.kubeconfig.as_os_str());
        vec![
            kubeconfig,
            OsString::from(format!("--context={}", self.context)),
        ]
    }

    fn command(&self) -> Command {
        let mut command = Command::new(KUBECTL);
        command
            .args(self.connection_args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command
    }

    fn get_args(&self, resource: &str, namespace: Option<&str>) -> Vec<OsString> {
        let mut args = vec![OsString::from("get"), OsString::from(resource)];
        if let Some(namespace) = namespace {
            args.push(OsString::from(format!("--namespace={namespace}")));
        }
        args.push(OsString::from("--output=json"));
        args
    }

    async fn run(&self, label: &str, args: Vec<OsString>) -> Result<Vec<u8>> {
        let mut command = self.command();
        command.args(&args);
        run_command(command, label, QUERY_TIMEOUT).await
    }
}

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

/// Rejects namespace names and resource names that could escape the argv slot
/// or confuse kubectl. Kubernetes names are DNS-1123-ish: `[a-zA-Z0-9._-]+`.
fn validate_segment(field: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{field} must not be empty");
    }
    if value.len() > MAX_SEGMENT_LEN {
        bail!("{field} is too long (max {MAX_SEGMENT_LEN} bytes)");
    }
    if value.starts_with('-') {
        bail!("{field} {value:?} must not start with '-'");
    }
    if value == "." || value == ".." || value.contains("..") {
        bail!("{field} {value:?} must not contain '..'");
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        bail!("{field} {value:?} may only contain ASCII letters, digits, '.', '_' and '-'");
    }
    Ok(())
}

fn validate_namespace(namespace: &str) -> Result<()> {
    validate_segment("namespace", namespace)
}

fn validate_context(context: &str) -> Result<()> {
    if context.trim().is_empty() {
        bail!("kube context must not be empty");
    }
    if context.starts_with('-') {
        bail!("kube context {context:?} must not start with '-'");
    }
    if context.chars().any(|c| c.is_control()) {
        bail!("kube context {context:?} must not contain control characters");
    }
    Ok(())
}

fn validate_kubeconfig(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("kubeconfig path must not be empty");
    }
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("kubeconfig {} cannot be accessed", path.display()))?;
    if !metadata.is_file() {
        bail!("kubeconfig {} is not a regular file", path.display());
    }
    // Opening proves the file is readable without loading its contents.
    std::fs::File::open(path)
        .with_context(|| format!("kubeconfig {} is not readable", path.display()))?;
    Ok(())
}

fn version_args() -> Vec<OsString> {
    vec![OsString::from("version"), OsString::from("--output=json")]
}

fn delete_args(namespace: &str, resource: &str, name: &str) -> Vec<OsString> {
    vec![
        OsString::from("delete"),
        OsString::from(resource),
        OsString::from(name),
        OsString::from(format!("--namespace={namespace}")),
        OsString::from("--ignore-not-found=true"),
    ]
}

fn apply_args(namespace: &str, manifest: &Path) -> Vec<OsString> {
    vec![
        OsString::from("apply"),
        OsString::from(format!("--namespace={namespace}")),
        OsString::from("-f"),
        manifest.as_os_str().to_owned(),
    ]
}

/// Writes `yaml` to a fresh temporary file that only the owner can read.
fn manifest_file(yaml: &str) -> Result<NamedTempFile> {
    let mut manifest = tempfile::Builder::new()
        .prefix("hakobi-kube-")
        .suffix(".yaml")
        .tempfile()
        .context("create temporary manifest file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        manifest
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .context("restrict temporary manifest permissions")?;
    }
    manifest
        .write_all(yaml.as_bytes())
        .and_then(|_| manifest.flush())
        .context("write temporary manifest file")?;
    Ok(manifest)
}

async fn run_command(mut command: Command, label: &str, timeout: Duration) -> Result<Vec<u8>> {
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| anyhow!("kubectl {label} timed out after {}s", timeout.as_secs()))?
        .map_err(|error| spawn_error(error, label))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            bail!("kubectl {label} failed without output ({})", output.status);
        }
        bail!("kubectl {label} failed: {stderr}");
    }
    Ok(output.stdout)
}

fn spawn_error(error: std::io::Error, label: &str) -> anyhow::Error {
    if error.kind() == std::io::ErrorKind::NotFound {
        anyhow!(
            "kubectl executable not found in PATH (required for `{label}`); \
             install kubectl and make it available to hakobi"
        )
    } else {
        anyhow::Error::new(error).context(format!("failed to start kubectl for `{label}`"))
    }
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

fn connection_info(stdout: &[u8], context: &str) -> Result<ConnectionInfo> {
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

fn namespaces_from_json(stdout: &[u8]) -> Result<Vec<NamespaceRow>> {
    let list: KubeList<RawNamespace> = parse_json(stdout, "namespaces")?;
    let mut rows = list
        .items
        .into_iter()
        .map(|item| NamespaceRow {
            name: item.metadata.name,
            status: item
                .status
                .and_then(|status| status.phase)
                .unwrap_or_else(|| "Unknown".to_owned()),
            created_at: item.metadata.creation_timestamp,
        })
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

fn deployments_from_json(stdout: &[u8]) -> Result<Vec<DeploymentRow>> {
    let list: KubeList<RawDeployment> = parse_json(stdout, "deployments")?;
    let mut rows = list
        .items
        .into_iter()
        .map(|item| DeploymentRow {
            name: item.metadata.name,
            replicas: clamp_count(item.spec.replicas),
            ready: clamp_count(item.status.ready_replicas),
            available: clamp_count(item.status.available_replicas),
            updated: clamp_count(item.status.updated_replicas),
            created_at: item.metadata.creation_timestamp,
        })
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

fn services_from_json(stdout: &[u8]) -> Result<Vec<ServiceRow>> {
    let list: KubeList<RawService> = parse_json(stdout, "services")?;
    let mut rows = list
        .items
        .into_iter()
        .map(|item| {
            let ports = item
                .spec
                .ports
                .into_iter()
                .map(|port| ServicePort {
                    name: port.name.filter(|name| !name.is_empty()),
                    port: port.port.unwrap_or_default(),
                    target_port: port.target_port.as_ref().and_then(value_string),
                    protocol: port.protocol.unwrap_or_else(|| "TCP".to_owned()),
                    node_port: port.node_port,
                })
                .collect::<Vec<_>>();
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
        })
        .collect::<Vec<_>>();
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

fn pods_from_json(stdout: &[u8]) -> Result<Vec<PodRow>> {
    let list: KubeList<RawPod> = parse_json(stdout, "pods")?;
    let mut rows = list
        .items
        .into_iter()
        .map(|item| {
            let restarts = item
                .status
                .container_statuses
                .iter()
                .map(|status| {
                    status
                        .restart_count
                        .unwrap_or(0)
                        .clamp(0, i64::from(u32::MAX))
                })
                .sum::<i64>();
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
                restarts: restarts.clamp(0, i64::from(u32::MAX)) as u32,
                ready_containers,
                container_count: item.spec.containers.len() as u32,
                created_at: item.metadata.creation_timestamp,
                started_at: item.status.start_time,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args_strings<I, S>(args: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        args.into_iter()
            .map(|arg| arg.as_ref().to_string_lossy().into_owned())
            .collect()
    }

    fn provider(context: &str) -> (tempfile::TempDir, KubernetesProvider) {
        let directory = tempfile::tempdir().unwrap();
        let kubeconfig = directory.path().join("config");
        std::fs::write(&kubeconfig, "apiVersion: v1\nkind: Config\n").unwrap();
        let provider = KubernetesProvider::new(&kubeconfig, context).unwrap();
        (directory, provider)
    }

    #[test]
    fn names_are_restricted_to_a_safe_charset() {
        for valid in ["default", "kube-system", "app_1", "a.b-c", "A9"] {
            assert!(
                validate_namespace(valid).is_ok(),
                "{valid} should be accepted"
            );
        }
        for invalid in [
            "",
            ".",
            "..",
            "a..b",
            "a/b",
            "../etc",
            "$(id)",
            "ns;rm -rf /",
            "ns name",
            "ns\nrm",
            "-n",
            "naïve",
        ] {
            assert!(
                validate_namespace(invalid).is_err(),
                "{invalid:?} should be rejected"
            );
        }
        assert!(validate_segment("name", "web-1").is_ok());
        assert!(validate_segment("resource", "deployment").is_ok());
        assert!(validate_segment("resource", "deployment/web").is_err());
    }

    #[test]
    fn contexts_must_be_non_empty_and_not_flag_like() {
        assert!(validate_context("homelab").is_ok());
        assert!(validate_context("gke_project_zone_cluster").is_ok());
        assert!(validate_context("").is_err());
        assert!(validate_context("   ").is_err());
        assert!(validate_context("--context").is_err());
        assert!(validate_context("bad\ncontext").is_err());
    }

    #[test]
    fn kubeconfig_must_be_a_readable_file() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("absent");
        assert!(validate_kubeconfig(&missing).is_err());
        assert!(validate_kubeconfig(directory.path()).is_err());
        assert!(validate_kubeconfig(Path::new("")).is_err());

        let present = directory.path().join("config");
        std::fs::write(&present, "apiVersion: v1\n").unwrap();
        assert!(validate_kubeconfig(&present).is_ok());
        assert!(KubernetesProvider::new(&present, "").is_err());
        assert!(KubernetesProvider::new(&present, "homelab").is_ok());
    }

    #[test]
    fn argv_pins_kubeconfig_and_context_without_a_shell() {
        let (directory, provider) = provider("homelab");
        let kubeconfig = directory.path().join("config");
        let expected = vec![
            format!("--kubeconfig={}", kubeconfig.display()),
            "--context=homelab".to_owned(),
        ];
        assert_eq!(args_strings(provider.connection_args()), expected);

        let command = provider.command();
        assert_eq!(
            command.as_std().get_program(),
            std::ffi::OsStr::new("kubectl")
        );
        assert_eq!(args_strings(command.as_std().get_args()), expected);
        assert_eq!(
            args_strings(version_args()),
            vec!["version", "--output=json"]
        );
    }

    #[test]
    fn get_argv_scopes_namespace_and_requests_json() {
        let (_directory, provider) = provider("homelab");
        assert_eq!(
            args_strings(provider.get_args("deployments", Some("prod"))),
            vec!["get", "deployments", "--namespace=prod", "--output=json"]
        );
        assert_eq!(
            args_strings(provider.get_args("namespaces", None)),
            vec!["get", "namespaces", "--output=json"]
        );
    }

    #[test]
    fn delete_and_apply_argv_are_explicit() {
        assert_eq!(
            args_strings(delete_args("prod", "deployment", "web")),
            vec![
                "delete",
                "deployment",
                "web",
                "--namespace=prod",
                "--ignore-not-found=true",
            ]
        );
        let path = Path::new("/tmp/hakobi-kube-test.yaml");
        assert_eq!(
            args_strings(apply_args("prod", path)),
            vec![
                "apply",
                "--namespace=prod",
                "-f",
                "/tmp/hakobi-kube-test.yaml",
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn manifests_land_in_owner_only_temp_files() {
        use std::os::unix::fs::PermissionsExt;

        let manifest = manifest_file("apiVersion: v1\nkind: ConfigMap\n").unwrap();
        let mode = manifest.as_file().metadata().unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let contents = std::fs::read_to_string(manifest.path()).unwrap();
        assert!(contents.contains("kind: ConfigMap"));
    }

    #[test]
    fn version_output_parses_client_and_server() {
        let stdout = serde_json::to_vec(&json!({
            "clientVersion": {
                "major": "1",
                "minor": "33",
                "gitVersion": "v1.33.9",
                "platform": "darwin/arm64"
            },
            "serverVersion": {
                "major": "1",
                "minor": "30",
                "gitVersion": "v1.30.2",
                "platform": "linux/amd64"
            }
        }))
        .unwrap();
        let info = connection_info(&stdout, "homelab").unwrap();
        assert_eq!(info.context, "homelab");
        assert_eq!(info.client_version.as_deref(), Some("v1.33.9"));
        assert_eq!(info.server_version, "v1.30.2");
        assert_eq!(info.platform.as_deref(), Some("linux/amd64"));

        let client_only = serde_json::to_vec(&json!({
            "clientVersion": {"gitVersion": "v1.33.9"}
        }))
        .unwrap();
        let error = connection_info(&client_only, "homelab").unwrap_err();
        assert!(error.to_string().contains("no server version"));
    }

    #[test]
    fn deployment_json_maps_counts_and_sorts() {
        let stdout = serde_json::to_vec(&json!({
            "items": [
                {
                    "metadata": {"name": "web", "creationTimestamp": "2026-01-02T03:04:05Z"},
                    "spec": {"replicas": 3},
                    "status": {"readyReplicas": 2, "availableReplicas": 2, "updatedReplicas": 3}
                },
                {
                    "metadata": {"name": "api"},
                    "spec": {"replicas": 1}
                }
            ]
        }))
        .unwrap();
        let rows = deployments_from_json(&stdout).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "api");
        assert_eq!(rows[0].replicas, 1);
        assert_eq!(rows[0].ready, 0);
        assert_eq!(rows[0].created_at, None);
        assert_eq!(rows[1].name, "web");
        assert_eq!(rows[1].replicas, 3);
        assert_eq!(rows[1].ready, 2);
        assert_eq!(rows[1].available, 2);
        assert_eq!(rows[1].updated, 3);
        assert_eq!(rows[1].created_at.as_deref(), Some("2026-01-02T03:04:05Z"));
    }

    #[test]
    fn service_json_maps_ports_and_selector() {
        let stdout = serde_json::to_vec(&json!({
            "items": [{
                "metadata": {"name": "web"},
                "spec": {
                    "type": "NodePort",
                    "clusterIP": "10.0.0.12",
                    "externalIPs": ["192.168.1.5"],
                    "selector": {"app": "web"},
                    "ports": [
                        {"name": "http", "port": 80, "targetPort": 8080, "protocol": "TCP", "nodePort": 30080},
                        {"port": 443, "targetPort": "https", "protocol": "TCP"}
                    ]
                }
            }]
        }))
        .unwrap();
        let rows = services_from_json(&stdout).unwrap();
        assert_eq!(rows.len(), 1);
        let service = &rows[0];
        assert_eq!(service.name, "web");
        assert_eq!(service.service_type, "NodePort");
        assert_eq!(service.cluster_ip.as_deref(), Some("10.0.0.12"));
        assert_eq!(service.external_ips, vec!["192.168.1.5"]);
        assert_eq!(service.selector.get("app").map(String::as_str), Some("web"));
        assert_eq!(service.ports.len(), 2);
        assert_eq!(service.ports[0].name.as_deref(), Some("http"));
        assert_eq!(service.ports[0].port, 80);
        assert_eq!(service.ports[0].target_port.as_deref(), Some("8080"));
        assert_eq!(service.ports[0].node_port, Some(30080));
        assert_eq!(service.ports[1].port, 443);
        assert_eq!(service.ports[1].target_port.as_deref(), Some("https"));
    }

    #[test]
    fn pod_json_maps_readiness_and_restarts() {
        let stdout = serde_json::to_vec(&json!({
            "items": [{
                "metadata": {"name": "web-0"},
                "spec": {"nodeName": "node-a", "containers": [{"name": "web"}, {"name": "sidecar"}]},
                "status": {
                    "phase": "Running",
                    "podIP": "10.244.0.7",
                    "startTime": "2026-01-02T03:04:05Z",
                    "containerStatuses": [
                        {"ready": true, "restartCount": 1},
                        {"ready": false, "restartCount": 2}
                    ]
                }
            }]
        }))
        .unwrap();
        let rows = pods_from_json(&stdout).unwrap();
        assert_eq!(rows.len(), 1);
        let pod = &rows[0];
        assert_eq!(pod.name, "web-0");
        assert_eq!(pod.phase, "Running");
        assert_eq!(pod.node.as_deref(), Some("node-a"));
        assert_eq!(pod.pod_ip.as_deref(), Some("10.244.0.7"));
        assert_eq!(pod.restarts, 3);
        assert_eq!(pod.ready_containers, 1);
        assert_eq!(pod.container_count, 2);
        assert_eq!(pod.started_at.as_deref(), Some("2026-01-02T03:04:05Z"));
    }

    #[test]
    fn namespace_json_maps_phase() {
        let stdout = serde_json::to_vec(&json!({
            "items": [
                {"metadata": {"name": "prod"}, "status": {"phase": "Active"}},
                {"metadata": {"name": "kube-system"}}
            ]
        }))
        .unwrap();
        let rows = namespaces_from_json(&stdout).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "kube-system");
        assert_eq!(rows[0].status, "Unknown");
        assert_eq!(rows[1].name, "prod");
        assert_eq!(rows[1].status, "Active");
    }

    #[test]
    fn malformed_json_reports_actionable_error() {
        let error = deployments_from_json(b"not json").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("deployments"), "{message}");
        assert!(message.contains("JSON"), "{message}");
    }

    #[tokio::test]
    async fn missing_kubectl_binary_reports_clear_error() {
        let command = Command::new("kubectl-hakobi-test-does-not-exist");
        let error = run_command(command, "version", Duration::from_secs(5))
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("kubectl executable not found"),
            "{message}"
        );
        assert!(message.contains("PATH"), "{message}");
    }

    #[tokio::test]
    async fn destructive_inputs_are_rejected_before_spawning() {
        let (_directory, provider) = provider("homelab");
        assert!(provider.apply("prod", "   ").await.is_err());
        assert!(provider.apply("../prod", "kind: ConfigMap").await.is_err());
        assert!(provider
            .delete("prod", "deployment", "$(rm -rf /)")
            .await
            .is_err());
        assert!(provider.delete("..", "deployment", "web").await.is_err());
    }
}
