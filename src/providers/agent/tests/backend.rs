use super::fake::spawn_fake_agent;
use super::TOKEN;
use crate::host_agent::{self, AgentEndpoint};
use crate::providers::agent::backend::{truncate_chars, MAX_ERROR_CHARS};
use crate::providers::agent::AgentBackend;
use crate::providers::docker::ContainerBackend;

#[tokio::test]
async fn agent_backend_reads_the_typed_agent_api() {
    let url = spawn_fake_agent(TOKEN).await;
    let backend = AgentBackend::new(AgentEndpoint::new(url, TOKEN).unwrap()).unwrap();

    assert_eq!(
        backend.test(host_agent::LOCAL_ENDPOINT).await.unwrap(),
        "Host agent · Docker 28.0.1 · API 1.47"
    );

    let rows = backend
        .containers(host_agent::LOCAL_ENDPOINT)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "demo-api");
    assert_eq!(rows[0].compose_project.as_deref(), Some("hakobi-stack"));
    assert_eq!(rows[0].health.as_deref(), Some("healthy"));
    assert_eq!(rows[0].ports.len(), 1);
    assert_eq!(rows[0].ports[0].host_port, Some(18080));

    let snapshot = backend.snapshot(host_agent::LOCAL_ENDPOINT).await.unwrap();
    assert_eq!(snapshot.host.name, "fedora-test");
    assert_eq!(snapshot.host.cpus, 16);
    assert_eq!(snapshot.images.len(), 1);
    assert_eq!(snapshot.volumes.len(), 1);
    assert_eq!(snapshot.networks.len(), 1);
}

#[tokio::test]
async fn agent_backend_reads_container_inspect_logs_and_stats() {
    let url = spawn_fake_agent(TOKEN).await;
    let backend = AgentBackend::new(AgentEndpoint::new(url, TOKEN).unwrap()).unwrap();

    let inspect = backend
        .inspect_container(host_agent::LOCAL_ENDPOINT, "demo-api")
        .await
        .unwrap();
    assert_eq!(inspect.name, "demo-api");
    assert_eq!(inspect.image, "hakobi/demo:latest");
    assert_eq!(inspect.state, "running");
    assert_eq!(inspect.health.as_deref(), Some("healthy"));
    assert_eq!(inspect.restart_count, 1);
    assert_eq!(inspect.ports.len(), 1);
    assert_eq!(inspect.ports[0].host_port, Some(18080));
    assert_eq!(inspect.mounts.len(), 1);
    assert_eq!(inspect.mounts[0].destination, "/data");
    assert_eq!(inspect.env, vec!["RUST_LOG=info".to_owned()]);
    assert_eq!(inspect.labels.len(), 1);

    let logs = backend
        .container_logs(host_agent::LOCAL_ENDPOINT, "demo-api", 100)
        .await
        .unwrap();
    assert_eq!(logs.lines.len(), 5);
    assert_eq!(logs.lines[0], "line-0");
    assert!(!logs.truncated);

    let stats = backend
        .container_stats(host_agent::LOCAL_ENDPOINT, "demo-api")
        .await
        .unwrap();
    assert!((stats.cpu_percent - 12.5).abs() < f64::EPSILON);
    assert_eq!(stats.memory_used_bytes, 1_048_576);
    assert_eq!(stats.memory_limit_bytes, 8_388_608);
    assert_eq!(stats.network_rx_bytes, 4096);
    assert_eq!(stats.network_tx_bytes, 8192);

    let missing = backend
        .inspect_container(host_agent::LOCAL_ENDPOINT, "other-api")
        .await
        .unwrap_err()
        .to_string();
    assert!(missing.contains("no such container"), "{missing}");
}

#[tokio::test]
async fn agent_backend_validates_detail_input_before_the_agent() {
    let url = spawn_fake_agent(TOKEN).await;
    let backend = AgentBackend::new(AgentEndpoint::new(url, TOKEN).unwrap()).unwrap();

    assert!(backend
        .inspect_container(host_agent::LOCAL_ENDPOINT, "bad name")
        .await
        .is_err());
    assert!(backend
        .container_logs(host_agent::LOCAL_ENDPOINT, "bad name", 100)
        .await
        .is_err());
    assert!(backend
        .container_stats(host_agent::LOCAL_ENDPOINT, "bad name")
        .await
        .is_err());
    // The log window is clamped to the protocol ceiling instead of passed
    // through unchecked.
    assert!(backend
        .container_logs(host_agent::LOCAL_ENDPOINT, "demo-api", 100_000)
        .await
        .is_ok());
}

#[tokio::test]
async fn agent_backend_sends_authenticated_mutations() {
    let url = spawn_fake_agent(TOKEN).await;
    let backend = AgentBackend::new(AgentEndpoint::new(url, TOKEN).unwrap()).unwrap();

    backend
        .start_container(host_agent::LOCAL_ENDPOINT, "demo-api")
        .await
        .unwrap();
    backend
        .stop_container(host_agent::LOCAL_ENDPOINT, "demo-api")
        .await
        .unwrap();
    backend
        .restart_container(host_agent::LOCAL_ENDPOINT, "demo-api")
        .await
        .unwrap();
    backend
        .remove_container(host_agent::LOCAL_ENDPOINT, "demo-api")
        .await
        .unwrap();

    let result = backend
        .run_container(
            host_agent::LOCAL_ENDPOINT,
            "alpine:3.20",
            "demo-api",
            8080,
            &Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.name, "demo-api");
    assert_eq!(result.host_port, 18000);
    assert_eq!(result.container_id, "id-demo-api");

    let missing = backend
        .start_container(host_agent::LOCAL_ENDPOINT, "other-api")
        .await
        .unwrap_err()
        .to_string();
    assert!(missing.contains("no such container"), "{missing}");
}

#[tokio::test]
async fn agent_backend_rejects_bad_input_before_calling_the_agent() {
    let url = spawn_fake_agent(TOKEN).await;
    let backend = AgentBackend::new(AgentEndpoint::new(url, TOKEN).unwrap()).unwrap();

    assert!(backend
        .start_container(host_agent::LOCAL_ENDPOINT, "bad name")
        .await
        .is_err());
    assert!(backend
        .run_container(
            host_agent::LOCAL_ENDPOINT,
            "alpine latest",
            "demo-api",
            8080,
            &Default::default(),
        )
        .await
        .is_err());
    assert!(backend
        .run_container(
            host_agent::LOCAL_ENDPOINT,
            "alpine:3.20",
            "demo-api",
            0,
            &Default::default()
        )
        .await
        .is_err());
    assert!(backend.test("unix:///var/run/docker.sock").await.is_err());
}

#[tokio::test]
async fn agent_backend_reports_authentication_failures() {
    let url = spawn_fake_agent(TOKEN).await;
    let backend =
        AgentBackend::new(AgentEndpoint::new(url, "another-token-0123456789abcdef").unwrap())
            .unwrap();

    let error = backend
        .containers(host_agent::LOCAL_ENDPOINT)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("unauthorized"), "{error}");
}

#[test]
fn error_messages_are_bounded() {
    let long = "x".repeat(MAX_ERROR_CHARS + 10);
    let bounded = truncate_chars(&long, MAX_ERROR_CHARS);
    assert_eq!(bounded.chars().count(), MAX_ERROR_CHARS + 1);
    assert!(bounded.ends_with('…'));
}
