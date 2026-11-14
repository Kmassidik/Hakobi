use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use super::probes::{kubectl_version, nix_version, probe_version};
use super::{Readiness, ToolStatus};

/// How long a single tool probe may take before it is killed.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Distro-neutral installation hint shown when Docker is missing.
const DOCKER_INSTALL_COMMAND: &str = "curl -fsSL https://get.docker.com | sh";

/// Installation hint shown when the Compose plugin is missing.
const COMPOSE_INSTALL_COMMAND: &str =
    "install the Docker Compose plugin: https://docs.docker.com/compose/install/";

/// Installation hint shown when kubectl is missing.
const KUBECTL_INSTALL_COMMAND: &str =
    "install kubectl from https://kubernetes.io/docs/tasks/tools/";

/// Installation hint shown when Nix is missing.
const NIX_INSTALL_COMMAND: &str =
    "curl -fsSL https://install.determinate.systems/nix | sh -s -- install --no-confirm";

/// Installation hint shown when nixpacks is missing.
const NIXPACKS_INSTALL_COMMAND: &str = "curl -sSL https://nixpacks.com/install.sh | bash";

/// Hints and environment used by a detection run.
pub(super) struct DetectConfig {
    /// `PATH` used to resolve executables.
    pub(super) path: OsString,
    /// Existing local Docker daemon socket, when one was found.
    pub(super) docker_socket: Option<PathBuf>,
    /// Per-probe timeout.
    pub(super) timeout: Duration,
}

impl DetectConfig {
    fn from_env() -> Self {
        Self {
            path: detection_path(),
            docker_socket: local_docker_socket(
                env::var_os("DOCKER_HOST").as_deref(),
                env::var_os("XDG_RUNTIME_DIR").as_deref(),
                env::var_os("HOME").as_deref(),
            ),
            timeout: PROBE_TIMEOUT,
        }
    }
}

/// Detects host readiness for all supported tools.
///
/// Probes run concurrently and are independent, so a slow tool cannot delay
/// the others. This function blocks for at most roughly [`PROBE_TIMEOUT`].
pub fn detect() -> Readiness {
    detect_with(&DetectConfig::from_env())
}

fn detection_path() -> OsString {
    let path = env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = env::split_paths(&path).collect();
    for extra in [
        "/nix/var/nix/profiles/default/bin",
        "/root/.nix-profile/bin",
    ] {
        let extra = PathBuf::from(extra);
        if extra.is_dir() && !paths.contains(&extra) {
            paths.push(extra);
        }
    }
    env::join_paths(paths).unwrap_or(path)
}

pub(super) fn detect_with(config: &DetectConfig) -> Readiness {
    thread::scope(|scope| {
        let docker = scope.spawn(|| docker_status(config));
        let compose = scope.spawn(|| compose_status(config));
        let kubectl = scope.spawn(|| kubectl_status(config));
        let nix = scope.spawn(|| nix_status(config));
        let nixpacks = scope.spawn(|| nixpacks_status(config));
        Readiness {
            docker: join(docker, DOCKER_INSTALL_COMMAND),
            compose: join(compose, COMPOSE_INSTALL_COMMAND),
            kubectl: join(kubectl, KUBECTL_INSTALL_COMMAND),
            nix: join(nix, NIX_INSTALL_COMMAND),
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

fn nix_status(config: &DetectConfig) -> ToolStatus {
    match probe_version(config, "nix", &["--version"]) {
        Some(version) => ready(nix_version(&version)),
        None => missing(NIX_INSTALL_COMMAND),
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
