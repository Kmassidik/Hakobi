use std::path::Path;
use std::time::Duration;

use serde_json::json;
use tokio::process::Command;

use super::{command::*, rows::*, KubernetesProvider};

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
    assert_eq!(
        args_strings(connection_args(provider.kubeconfig(), provider.context())),
        expected
    );

    let command = command(provider.kubeconfig(), provider.context());
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
    let (_directory, _provider) = provider("homelab");
    assert_eq!(
        args_strings(get_args("deployments", Some("prod"))),
        vec!["get", "deployments", "--namespace=prod", "--output=json"]
    );
    assert_eq!(
        args_strings(get_args("namespaces", None)),
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
