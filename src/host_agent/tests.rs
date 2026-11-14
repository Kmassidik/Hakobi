use super::*;

#[test]
fn local_endpoints_use_the_agent_scheme() {
    assert!(is_local_endpoint(LOCAL_ENDPOINT));
    assert!(is_local_endpoint("  agent://local  "));
    assert!(is_local_endpoint("AGENT://local"));
    assert!(!is_local_endpoint("agent://"));
    assert!(!is_local_endpoint(DOCKER_SOCKET_ENDPOINT));
    assert!(!is_local_endpoint("tcp://127.0.0.1:2375"));
    assert!(!is_local_endpoint(""));
}

#[test]
fn socket_endpoint_matches_the_socket_path() {
    assert_eq!(
        DOCKER_SOCKET_ENDPOINT,
        format!("unix://{DOCKER_SOCKET_PATH}")
    );
}

#[test]
fn agent_endpoints_require_a_loopback_style_url_and_token() {
    let token = "a".repeat(MIN_TOKEN_LEN);
    let endpoint = AgentEndpoint::new("http://127.0.0.1:8787/", token.as_str()).unwrap();
    assert_eq!(endpoint.url(), "http://127.0.0.1:8787");
    assert_eq!(endpoint.token(), token.as_str());

    assert!(AgentEndpoint::new("http://127.0.0.1:8787", "short").is_none());
    assert!(AgentEndpoint::new("ftp://127.0.0.1:8787", token.as_str()).is_none());
    assert!(AgentEndpoint::new("http:///v1/docker", token.as_str()).is_none());
    assert!(AgentEndpoint::new("http://127.0.0.1:8787/prefix", token.as_str()).is_none());
    assert!(AgentEndpoint::new("http://user@127.0.0.1:8787", token.as_str()).is_none());
    assert!(AgentEndpoint::new("http://127.0.0.1:8787?token=1", token.as_str()).is_none());
}

#[test]
fn agent_endpoint_debug_redacts_the_token() {
    let token = "b".repeat(MIN_TOKEN_LEN);
    let endpoint = AgentEndpoint::new("http://127.0.0.1:8787", token.as_str()).unwrap();
    let rendered = format!("{endpoint:?}");
    assert!(rendered.contains("http://127.0.0.1:8787"));
    assert!(!rendered.contains(&token));
    assert!(rendered.contains("<redacted>"));
}

#[test]
fn container_rows_round_trip_through_json() {
    let row = DockerContainerRow {
        id: "0123456789abcdef".into(),
        name: "demo-api".into(),
        image: "hakobi/demo:latest".into(),
        state: "running".into(),
        status: "Up 2 hours".into(),
        health: Some("healthy".into()),
        compose_project: Some("hakobi-stack".into()),
        compose_service: Some("api".into()),
        ports: vec![DockerContainerPort {
            host_ip: Some("0.0.0.0".into()),
            host_port: Some(18080),
            container_port: 8080,
            protocol: "tcp".into(),
        }],
    };
    let encoded = serde_json::to_string(&row).unwrap();
    let decoded: DockerContainerRow = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, row);

    let wrapped = serde_json::to_string(&DockerContainersResponse {
        containers: vec![row.clone()],
    })
    .unwrap();
    let decoded: DockerContainersResponse = serde_json::from_str(&wrapped).unwrap();
    assert_eq!(decoded.containers, vec![row]);
}

#[test]
fn snapshots_round_trip_through_json() {
    let snapshot = DockerSnapshotResponse {
        host: DockerHostInfo {
            name: "fedora-test".into(),
            server_version: "28.0.1".into(),
            containers: 2,
            running: 1,
            images: 3,
            operating_system: "Fedora Linux 44".into(),
            architecture: "x86_64".into(),
            cpus: 16,
        },
        images: vec![DockerImageRow {
            id: "sha256:abcdef".into(),
            tags: vec!["alpine:3.20".into()],
            size_bytes: 8 * 1_048_576,
        }],
        volumes: vec![DockerVolumeRow {
            name: "hakobi-data".into(),
            driver: "local".into(),
            mountpoint: "/var/lib/docker/volumes/hakobi-data/_data".into(),
        }],
        networks: vec![DockerNetworkRow {
            name: "bridge".into(),
            driver: "bridge".into(),
            scope: "local".into(),
        }],
    };
    let encoded = serde_json::to_string(&snapshot).unwrap();
    let decoded: DockerSnapshotResponse = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, snapshot);
}

#[test]
fn run_requests_and_results_round_trip_through_json() {
    let request = RunContainerRequest {
        env: Default::default(),
        image: "alpine:3.20".into(),
        name: "demo-api".into(),
        container_port: 8080,
    };
    let encoded = serde_json::to_string(&request).unwrap();
    assert_eq!(
        serde_json::from_str::<RunContainerRequest>(&encoded).unwrap(),
        request
    );

    let result = RunContainerResponse {
        container_id: "feedface".into(),
        host_port: 18000,
        name: "demo-api".into(),
    };
    let encoded = serde_json::to_string(&result).unwrap();
    assert_eq!(
        serde_json::from_str::<RunContainerResponse>(&encoded).unwrap(),
        result
    );
}

#[test]
fn inspect_stats_and_logs_round_trip_through_json() {
    let inspect = sample_inspect();
    let encoded = serde_json::to_string(&inspect).unwrap();
    assert_eq!(
        serde_json::from_str::<DockerContainerInspect>(&encoded).unwrap(),
        inspect
    );

    let stats = DockerContainerStats {
        cpu_percent: 12.5,
        memory_used_bytes: 1_048_576,
        memory_limit_bytes: 8_388_608,
        network_rx_bytes: 4096,
        network_tx_bytes: 8192,
    };
    let encoded = serde_json::to_string(&stats).unwrap();
    assert_eq!(
        serde_json::from_str::<DockerContainerStats>(&encoded).unwrap(),
        stats
    );

    let logs = DockerContainerLogs {
        lines: vec!["listening on :8080".into()],
        truncated: false,
    };
    let encoded = serde_json::to_string(&logs).unwrap();
    assert_eq!(
        serde_json::from_str::<DockerContainerLogs>(&encoded).unwrap(),
        logs
    );
}

/// Fully populated inspect payload for round-trip assertions.
fn sample_inspect() -> DockerContainerInspect {
    DockerContainerInspect {
        id: "0123456789abcdef".into(),
        name: "demo-api".into(),
        image: "hakobi/demo:latest".into(),
        image_id: "sha256:abcdef".into(),
        command: "/app/server".into(),
        state: "running".into(),
        status: "Up 2 hours".into(),
        health: Some("healthy".into()),
        created_at: "2026-01-01T00:00:00Z".into(),
        started_at: "2026-01-02T00:00:00Z".into(),
        restart_count: 1,
        platform: "linux".into(),
        ports: vec![DockerContainerPort {
            host_ip: Some("0.0.0.0".into()),
            host_port: Some(18080),
            container_port: 8080,
            protocol: "tcp".into(),
        }],
        mounts: vec![DockerContainerMount {
            source: "/var/lib/docker/volumes/data/_data".into(),
            destination: "/data".into(),
            mode: "z".into(),
            read_only: false,
        }],
        env: vec!["RUST_LOG=info".into()],
        labels: vec![(
            "com.docker.compose.project".to_owned(),
            "hakobi-stack".to_owned(),
        )],
    }
}

#[test]
fn inspect_defaults_survive_missing_and_unknown_fields() {
    let decoded: DockerContainerInspect =
        serde_json::from_str(r#"{"name":"demo-api","future":{"a":1}}"#).unwrap();
    assert_eq!(decoded.name, "demo-api");
    assert_eq!(decoded.state, "");
    assert!(decoded.ports.is_empty());
    assert!(decoded.labels.is_empty());
    assert!(decoded.health.is_none());

    let decoded: DockerContainerStats = serde_json::from_str(r#"{"cpu_percent":1.5}"#).unwrap();
    assert_eq!(decoded.cpu_percent, 1.5);
    assert_eq!(decoded.memory_used_bytes, 0);

    let decoded: DockerContainerLogs = serde_json::from_str(r#"{"lines":["one"]}"#).unwrap();
    assert_eq!(decoded.lines, vec!["one".to_owned()]);
    assert!(!decoded.truncated);
}

#[test]
fn metrics_and_install_jobs_round_trip_through_json() {
    let metrics = DockerMetrics {
        containers: 2,
        cpu_percent: 12.5,
        memory_used_bytes: 1_048_576,
        memory_limit_bytes: 8_388_608,
    };
    let encoded = serde_json::to_string(&metrics).unwrap();
    assert_eq!(
        serde_json::from_str::<DockerMetrics>(&encoded).unwrap(),
        metrics
    );

    let job = InstallJob {
        running: true,
        success: false,
        output: "installing".into(),
    };
    let encoded = serde_json::to_string(&job).unwrap();
    assert_eq!(serde_json::from_str::<InstallJob>(&encoded).unwrap(), job);

    // Missing fields fall back to defaults for forward compatibility.
    let decoded: InstallJob = serde_json::from_str(r#"{"running":true}"#).unwrap();
    assert!(decoded.running);
    assert!(!decoded.success);
    assert!(decoded.output.is_empty());
}

#[test]
fn unknown_fields_and_missing_optional_fields_stay_compatible() {
    let decoded: DockerContainerRow = serde_json::from_str(
        r#"{"name":"demo-api","state":"running","future_field":{"nested":true}}"#,
    )
    .unwrap();
    assert_eq!(decoded.name, "demo-api");
    assert_eq!(decoded.state, "running");
    assert!(decoded.ports.is_empty());
    assert!(decoded.health.is_none());

    let decoded: DockerContainerPort = serde_json::from_str(r#"{"container_port":8080}"#).unwrap();
    assert_eq!(decoded.container_port, 8080);
    assert_eq!(decoded.protocol, "tcp");

    let decoded: DockerSnapshotResponse =
        serde_json::from_str(r#"{"host":{"name":"host"},"extra":[]}"#).unwrap();
    assert_eq!(decoded.host.name, "host");
    assert!(decoded.images.is_empty());

    let decoded: AgentErrorBody = serde_json::from_str(r#"{"error":"boom","code":7}"#).unwrap();
    assert_eq!(decoded.error, "boom");

    assert_eq!(
        serde_json::to_string(&DockerVersion::default()).unwrap(),
        r#"{"version":"","api_version":""}"#
    );
}
