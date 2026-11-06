//! Host readiness detection for the tooling Hakobi can orchestrate.
//!
//! [`detect`] inspects the local host without running installers or any
//! privileged command. Each probe uses a fixed executable name and a fixed
//! argument vector, never a shell, and is bounded by a short timeout:
//!
//! * Docker is ready when the daemon answers `docker version` or a local
//!   daemon socket exists.
//! * Compose is the Docker Compose plugin (`docker compose version --short`).
//! * Kubernetes is the `kubectl` client.
//! * Nixpacks is the `nixpacks` builder.
//!
//! Missing tools carry a distro-neutral, display-only installation hint. The
//! hint is never executed.

use std::{
    env,
    ffi::{OsStr, OsString},
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// How long a single tool probe may take before it is killed.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Poll interval while waiting for a probe process to exit.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Extra time allowed for draining a probe's stdout after it exits.
const OUTPUT_GRACE: Duration = Duration::from_millis(250);

/// Upper bound on a probe's stdout; version strings are tiny.
const MAX_OUTPUT_BYTES: u64 = 8 * 1024;

/// Distro-neutral installation hint shown when Docker is missing.
const DOCKER_INSTALL_COMMAND: &str = "curl -fsSL https://get.docker.com | sh";

/// Installation hint shown when the Compose plugin is missing.
const COMPOSE_INSTALL_COMMAND: &str =
    "install the Docker Compose plugin: https://docs.docker.com/compose/install/";

/// Installation hint shown when kubectl is missing.
const KUBECTL_INSTALL_COMMAND: &str =
    "install kubectl from https://kubernetes.io/docs/tasks/tools/";

/// Installation hint shown when nixpacks is missing.
const NIXPACKS_INSTALL_COMMAND: &str = "curl -sSL https://nixpacks.com/install.sh | bash";

/// Availability of a single host tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    /// The tool answered its version probe.
    Ready {
        /// Version reported by the tool.
        version: String,
    },
    /// The tool is not installed or did not answer.
    Missing {
        /// Safe, display-only installation hint.
        install_command: String,
    },
}

/// Readiness of every tool the stack lifecycle features can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readiness {
    pub docker: ToolStatus,
    pub compose: ToolStatus,
    pub kubectl: ToolStatus,
    pub nixpacks: ToolStatus,
}

/// Detects host readiness for all supported tools.
///
/// Probes run concurrently and are independent, so a slow tool cannot delay
/// the others. This function blocks for at most roughly [`PROBE_TIMEOUT`].
pub fn detect() -> Readiness {
    detect_with(&DetectConfig::from_env())
}

/// Hints and environment used by a detection run.
struct DetectConfig {
    /// `PATH` used to resolve executables.
    path: OsString,
    /// Existing local Docker daemon socket, when one was found.
    docker_socket: Option<PathBuf>,
    /// Per-probe timeout.
    timeout: Duration,
}

impl DetectConfig {
    fn from_env() -> Self {
        Self {
            path: env::var_os("PATH").unwrap_or_default(),
            docker_socket: local_docker_socket(
                env::var_os("DOCKER_HOST").as_deref(),
                env::var_os("XDG_RUNTIME_DIR").as_deref(),
                env::var_os("HOME").as_deref(),
            ),
            timeout: PROBE_TIMEOUT,
        }
    }
}

fn detect_with(config: &DetectConfig) -> Readiness {
    thread::scope(|scope| {
        let docker = scope.spawn(|| docker_status(config));
        let compose = scope.spawn(|| compose_status(config));
        let kubectl = scope.spawn(|| kubectl_status(config));
        let nixpacks = scope.spawn(|| nixpacks_status(config));
        Readiness {
            docker: join(docker, DOCKER_INSTALL_COMMAND),
            compose: join(compose, COMPOSE_INSTALL_COMMAND),
            kubectl: join(kubectl, KUBECTL_INSTALL_COMMAND),
            nixpacks: join(nixpacks, NIXPACKS_INSTALL_COMMAND),
        }
    })
}

/// Joins a probe thread, degrading a panic to a missing tool.
fn join(handle: thread::ScopedJoinHandle<'_, ToolStatus>, install_command: &str) -> ToolStatus {
    handle.join().unwrap_or_else(|_| missing(install_command))
}

fn ready(version: impl Into<String>) -> ToolStatus {
    ToolStatus::Ready {
        version: version.into(),
    }
}

fn missing(install_command: &str) -> ToolStatus {
    ToolStatus::Missing {
        install_command: install_command.to_owned(),
    }
}

fn docker_status(config: &DetectConfig) -> ToolStatus {
    if let Some(version) = probe_version(
        config,
        "docker",
        &["version", "--format", "{{.Server.Version}}"],
    ) {
        return ready(version);
    }
    if config.docker_socket.is_some() {
        return ready("daemon socket detected");
    }
    missing(DOCKER_INSTALL_COMMAND)
}

fn compose_status(config: &DetectConfig) -> ToolStatus {
    match probe_version(config, "docker", &["compose", "version", "--short"]) {
        Some(version) => ready(version),
        None => missing(COMPOSE_INSTALL_COMMAND),
    }
}

fn kubectl_status(config: &DetectConfig) -> ToolStatus {
    match probe_version(config, "kubectl", &["version", "--client", "-o", "json"]) {
        Some(output) => ready(kubectl_version(&output)),
        None => missing(KUBECTL_INSTALL_COMMAND),
    }
}

fn nixpacks_status(config: &DetectConfig) -> ToolStatus {
    match probe_version(config, "nixpacks", &["--version"]) {
        Some(raw) => {
            let version = raw.strip_prefix("nixpacks ").unwrap_or(raw.as_str()).trim();
            ready(version)
        }
        None => missing(NIXPACKS_INSTALL_COMMAND),
    }
}

/// Extracts the client version from `kubectl version --client -o json`,
/// falling back to the first non-empty output line.
fn kubectl_version(output: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(output) {
        if let Some(version) = value
            .pointer("/clientVersion/gitVersion")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|version| !version.is_empty())
        {
            return version.to_owned();
        }
    }
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let line = line.strip_prefix("Client Version:").unwrap_or(line).trim();
    line.to_owned()
}

/// Runs a version probe with a hard timeout and returns trimmed stdout.
///
/// The process is spawned directly (no shell) and killed if it outlives the
/// configured timeout. `PATH` is set explicitly so detection can be pointed at
/// a controlled set of executables.
fn probe_version(config: &DetectConfig, program: &str, args: &[&str]) -> Option<String> {
    let executable = resolve_program(config, program)?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .env("PATH", &config.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut stdout = stdout.take(MAX_OUTPUT_BYTES);
        let mut output = Vec::new();
        let _ = stdout.read_to_end(&mut output);
        let _ = sender.send(output);
    });

    let deadline = Instant::now() + config.timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                break;
            }
            Ok(None) => {}
            Err(_) => return None,
        }
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }

    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .checked_add(OUTPUT_GRACE)
        .unwrap_or(OUTPUT_GRACE);
    let output = receiver.recv_timeout(remaining).ok()?;
    let output = String::from_utf8_lossy(&output);
    let version = output.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

/// Resolves an executable through the configured `PATH` without a shell.
fn resolve_program(config: &DetectConfig, program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() || program.contains(std::path::MAIN_SEPARATOR) {
        return is_executable(path).then(|| path.to_path_buf());
    }

    #[cfg(windows)]
    let names = [format!("{program}.exe"), program.to_owned()];
    #[cfg(not(windows))]
    let names = [program.to_owned()];

    for directory in env::split_paths(&config.path) {
        for name in &names {
            let candidate = directory.join(name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Finds an existing local Docker daemon socket.
///
/// `DOCKER_HOST` wins when it names a unix socket; TCP/SSH endpoints have no
/// local socket to inspect, so they are left to the CLI probe.
#[cfg(unix)]
fn local_docker_socket(
    docker_host: Option<&OsStr>,
    xdg_runtime_dir: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(host) = docker_host {
        let host = host.to_string_lossy();
        let path = PathBuf::from(host.strip_prefix("unix://")?);
        return path.exists().then_some(path);
    }

    let mut candidates = vec![PathBuf::from("/var/run/docker.sock")];
    if let Some(directory) = xdg_runtime_dir {
        candidates.push(Path::new(directory).join("docker.sock"));
    }
    if let Some(home) = home {
        candidates.push(Path::new(home).join(".docker/run/docker.sock"));
    }
    candidates.into_iter().find(|candidate| candidate.exists())
}

#[cfg(not(unix))]
fn local_docker_socket(
    _docker_host: Option<&OsStr>,
    _xdg_runtime_dir: Option<&OsStr>,
    _home: Option<&OsStr>,
) -> Option<PathBuf> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    const DOCKER_SCRIPT: &str = r#"#!/bin/sh
case "$1" in
    version) printf '27.3.1\n' ;;
    compose) printf '2.29.7\n' ;;
    *) exit 1 ;;
esac
"#;

    const DOCKER_WITHOUT_COMPOSE_SCRIPT: &str = r#"#!/bin/sh
case "$1" in
    version) printf '27.3.1\n' ;;
    *) exit 1 ;;
esac
"#;

    const KUBECTL_SCRIPT: &str = r#"#!/bin/sh
printf '{"clientVersion":{"major":"1","minor":"30","gitVersion":"v1.30.2"}}\n'
"#;

    const NIXPACKS_SCRIPT: &str = r#"#!/bin/sh
printf 'nixpacks 1.29.1\n'
"#;

    fn write_fake(directory: &Path, name: &str, script: &str) {
        let path = directory.join(name);
        fs::write(&path, script).expect("write fake executable");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
            .expect("mark fake executable executable");
    }

    fn test_config(directory: &Path) -> DetectConfig {
        DetectConfig {
            path: directory.as_os_str().to_owned(),
            docker_socket: None,
            timeout: Duration::from_secs(5),
        }
    }

    fn version_of(status: &ToolStatus) -> Option<&str> {
        match status {
            ToolStatus::Ready { version } => Some(version),
            ToolStatus::Missing { .. } => None,
        }
    }

    fn install_of(status: &ToolStatus) -> Option<&str> {
        match status {
            ToolStatus::Missing { install_command } => Some(install_command),
            ToolStatus::Ready { .. } => None,
        }
    }

    fn assert_missing(status: &ToolStatus) {
        let install = install_of(status).expect("expected a missing tool");
        assert!(!install.trim().is_empty(), "install hint must not be empty");
        assert!(
            !install.contains('\n'),
            "install hint must be a single line"
        );
    }

    #[test]
    fn reports_ready_versions_from_path() {
        let directory = tempfile::tempdir().expect("temp dir");
        write_fake(directory.path(), "docker", DOCKER_SCRIPT);
        write_fake(directory.path(), "kubectl", KUBECTL_SCRIPT);
        write_fake(directory.path(), "nixpacks", NIXPACKS_SCRIPT);

        let readiness = detect_with(&test_config(directory.path()));

        assert_eq!(version_of(&readiness.docker), Some("27.3.1"));
        assert_eq!(version_of(&readiness.compose), Some("2.29.7"));
        assert_eq!(version_of(&readiness.kubectl), Some("v1.30.2"));
        assert_eq!(version_of(&readiness.nixpacks), Some("1.29.1"));
    }

    #[test]
    fn reports_missing_when_not_on_path() {
        let directory = tempfile::tempdir().expect("temp dir");

        let readiness = detect_with(&test_config(directory.path()));

        assert_missing(&readiness.docker);
        assert_missing(&readiness.compose);
        assert_missing(&readiness.kubectl);
        assert_missing(&readiness.nixpacks);
    }

    #[test]
    fn docker_is_ready_when_only_the_socket_exists() {
        let directory = tempfile::tempdir().expect("temp dir");
        let socket = tempfile::NamedTempFile::new().expect("socket placeholder");
        let config = DetectConfig {
            path: directory.path().as_os_str().to_owned(),
            docker_socket: Some(socket.path().to_path_buf()),
            timeout: Duration::from_secs(5),
        };

        let readiness = detect_with(&config);

        assert_eq!(
            version_of(&readiness.docker),
            Some("daemon socket detected")
        );
        assert_missing(&readiness.compose);
        assert_missing(&readiness.kubectl);
        assert_missing(&readiness.nixpacks);
    }

    #[test]
    fn compose_reports_missing_when_docker_has_no_plugin() {
        let directory = tempfile::tempdir().expect("temp dir");
        write_fake(directory.path(), "docker", DOCKER_WITHOUT_COMPOSE_SCRIPT);

        let readiness = detect_with(&test_config(directory.path()));

        assert_eq!(version_of(&readiness.docker), Some("27.3.1"));
        assert_missing(&readiness.compose);
    }

    #[test]
    fn probes_time_out_instead_of_hanging() {
        let directory = tempfile::tempdir().expect("temp dir");
        write_fake(
            directory.path(),
            "docker",
            "#!/bin/sh\nsleep 5\nprintf 'late\\n'\n",
        );
        let config = DetectConfig {
            path: directory.path().as_os_str().to_owned(),
            docker_socket: None,
            timeout: Duration::from_millis(100),
        };

        let started = Instant::now();
        let readiness = detect_with(&config);

        assert!(
            started.elapsed() < Duration::from_secs(2),
            "probe must respect its timeout"
        );
        assert_missing(&readiness.docker);
    }

    #[test]
    fn parses_kubectl_client_version() {
        assert_eq!(
            kubectl_version(r#"{"clientVersion":{"gitVersion":"v1.31.0"}}"#),
            "v1.31.0"
        );
        assert_eq!(
            kubectl_version("Client Version: v1.30.1\nKustomize Version: v5.0.4\n"),
            "v1.30.1"
        );
    }
}
