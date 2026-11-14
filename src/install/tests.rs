use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use super::detection::{detect_with, DetectConfig};
use super::probes::kubectl_version;
use super::ToolStatus;

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
    // No external commands: the child PATH is the tempdir, and some
    // shells fall back to a default PATH and would find real `sleep`.
    write_fake(
        directory.path(),
        "docker",
        "#!/bin/sh\nn=0\nwhile [ $n -lt 200000000 ]; do n=$((n+1)); done\nprintf 'late\\n'\n",
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
