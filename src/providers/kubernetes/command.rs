use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use tempfile::NamedTempFile;
use tokio::process::Command;

const KUBECTL: &str = "kubectl";

/// Default deadline for short cluster queries.
pub(super) const QUERY_TIMEOUT: Duration = Duration::from_secs(30);
/// Applying manifests can legitimately exceed the query deadline.
pub(super) const APPLY_TIMEOUT: Duration = Duration::from_secs(120);
/// Longest DNS-1123 style name we accept.
const MAX_SEGMENT_LEN: usize = 253;

/// Rejects namespace names and resource names that could escape the argv slot
/// or confuse kubectl. Kubernetes names are DNS-1123-ish: `[a-zA-Z0-9._-]+`.
pub(super) fn validate_segment(field: &str, value: &str) -> Result<()> {
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

pub(super) fn validate_namespace(namespace: &str) -> Result<()> {
    validate_segment("namespace", namespace)
}

pub(super) fn validate_context(context: &str) -> Result<()> {
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

pub(super) fn validate_kubeconfig(path: &Path) -> Result<()> {
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

/// Base argv shared by every invocation: pinned kubeconfig and context.
pub(super) fn connection_args(kubeconfig: &Path, context: &str) -> Vec<OsString> {
    let mut kubeconfig_arg = OsString::from("--kubeconfig=");
    kubeconfig_arg.push(kubeconfig.as_os_str());
    vec![
        kubeconfig_arg,
        OsString::from(format!("--context={context}")),
    ]
}

pub(super) fn command(kubeconfig: &Path, context: &str) -> Command {
    let mut command = Command::new(KUBECTL);
    command
        .args(connection_args(kubeconfig, context))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

pub(super) fn get_args(resource: &str, namespace: Option<&str>) -> Vec<OsString> {
    let mut args = vec![OsString::from("get"), OsString::from(resource)];
    if let Some(namespace) = namespace {
        args.push(OsString::from(format!("--namespace={namespace}")));
    }
    args.push(OsString::from("--output=json"));
    args
}

pub(super) fn version_args() -> Vec<OsString> {
    vec![OsString::from("version"), OsString::from("--output=json")]
}

pub(super) fn delete_args(namespace: &str, resource: &str, name: &str) -> Vec<OsString> {
    vec![
        OsString::from("delete"),
        OsString::from(resource),
        OsString::from(name),
        OsString::from(format!("--namespace={namespace}")),
        OsString::from("--ignore-not-found=true"),
    ]
}

pub(super) fn apply_args(namespace: &str, manifest: &Path) -> Vec<OsString> {
    vec![
        OsString::from("apply"),
        OsString::from(format!("--namespace={namespace}")),
        OsString::from("-f"),
        manifest.as_os_str().to_owned(),
    ]
}

/// Writes `yaml` to a fresh temporary file that only the owner can read.
pub(super) fn manifest_file(yaml: &str) -> Result<NamedTempFile> {
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

pub(super) async fn run_command(
    mut command: Command,
    label: &str,
    timeout: Duration,
) -> Result<Vec<u8>> {
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
