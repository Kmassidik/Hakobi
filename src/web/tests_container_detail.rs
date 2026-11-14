use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{header::COOKIE, Request, StatusCode},
    Router,
};
use hakobi::{
    catalog::Catalog,
    providers::{
        ContainerBackend, ContainerPort, ContainerRow, DockerSnapshot, HostInfo, RunResult,
    },
};
use tempfile::TempDir;
use tower::ServiceExt;

use super::{
    router_with_backend,
    tests_support::{body_text, config, sign_in},
};

/// Backend that answers the container detail reads with fixed data, so the
/// detail page, stats partial, and logs partial can be rendered without a
/// Docker daemon.
struct DetailBackend;

fn detail_inspect() -> hakobi::providers::DockerInspect {
    hakobi::providers::DockerInspect {
        id: "0123456789abcdef".into(),
        name: "demo-api".into(),
        image: "hakobi/demo:latest".into(),
        image_id: "sha256:abcdef".into(),
        command: "/app/server --port 8080".into(),
        state: "running".into(),
        status: String::new(),
        health: Some("healthy".into()),
        created_at: "2026-01-01T00:00:00Z".into(),
        started_at: "2026-01-02T00:00:00Z".into(),
        restart_count: 1,
        platform: "linux".into(),
        ports: vec![ContainerPort {
            host_ip: Some("0.0.0.0".into()),
            host_port: Some(18080),
            container_port: 8080,
            protocol: "tcp".into(),
        }],
        mounts: vec![hakobi::providers::ContainerMount {
            source: "/var/lib/docker/volumes/data/_data".into(),
            destination: "/data".into(),
            mode: String::new(),
            read_only: false,
        }],
        env: vec!["RUST_LOG=info".into()],
        labels: vec![(
            "com.docker.compose.project".to_owned(),
            "hakobi-stack".to_owned(),
        )],
    }
}

#[async_trait]
impl ContainerBackend for DetailBackend {
    async fn test(&self, _endpoint: &str) -> anyhow::Result<String> {
        Ok("Detail backend".into())
    }

    async fn containers(&self, _endpoint: &str) -> anyhow::Result<Vec<ContainerRow>> {
        Ok(vec![ContainerRow {
            id: "0123456789abcdef".into(),
            name: "demo-api".into(),
            image: "hakobi/demo:latest".into(),
            state: "running".into(),
            status: "Up 2 hours".into(),
            health: Some("healthy".into()),
            compose_project: None,
            compose_service: None,
            ports: vec![],
        }])
    }

    async fn snapshot(&self, _endpoint: &str) -> anyhow::Result<DockerSnapshot> {
        Ok(DockerSnapshot {
            host: HostInfo {
                name: "detail-test".into(),
                server_version: "28.0.1".into(),
                containers: 1,
                running: 1,
                images: 1,
                operating_system: "Test".into(),
                architecture: "x86_64".into(),
                cpus: 4,
            },
            images: vec![],
            volumes: vec![],
            networks: vec![],
        })
    }

    async fn inspect_container(
        &self,
        _endpoint: &str,
        name: &str,
    ) -> anyhow::Result<hakobi::providers::DockerInspect> {
        if name == "demo-api" {
            Ok(detail_inspect())
        } else {
            Err(anyhow::anyhow!(
                "Docker responded with status code 404: No such container: {name}"
            ))
        }
    }

    async fn container_logs(
        &self,
        _endpoint: &str,
        name: &str,
        tail: usize,
    ) -> anyhow::Result<hakobi::providers::DockerLogs> {
        if name != "demo-api" {
            return Err(anyhow::anyhow!("No such container: {name}"));
        }
        let count = tail.min(3);
        Ok(hakobi::providers::DockerLogs {
            lines: (0..count).map(|index| format!("line-{index}")).collect(),
            truncated: false,
        })
    }

    async fn container_stats(
        &self,
        _endpoint: &str,
        name: &str,
    ) -> anyhow::Result<hakobi::providers::DockerContainerUsage> {
        if name != "demo-api" {
            return Err(anyhow::anyhow!("No such container: {name}"));
        }
        Ok(hakobi::providers::DockerContainerUsage {
            cpu_percent: 12.5,
            memory_used_bytes: 1_048_576,
            memory_limit_bytes: 8_388_608,
            network_rx_bytes: 4096,
            network_tx_bytes: 8192,
        })
    }

    async fn start_container(&self, _endpoint: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stop_container(&self, _endpoint: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn restart_container(&self, _endpoint: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_container(&self, _endpoint: &str, _name: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn run_container(
        &self,
        _endpoint: &str,
        _image: &str,
        name: &str,
        _container_port: u16,
        _env: &std::collections::HashMap<String, String>,
    ) -> anyhow::Result<RunResult> {
        Ok(RunResult {
            container_id: "fedcba9876543210".into(),
            host_port: 18050,
            name: name.to_owned(),
        })
    }
}

fn detail_app() -> (TempDir, Router) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("catalog.sqlite3");
    (
        directory,
        router_with_backend(config(), path, Arc::new(DetailBackend)),
    )
}

fn detail_provider(data: &TempDir) -> String {
    let path = data.path().join("catalog.sqlite3");
    let mut catalog = Catalog::open(&path).unwrap();
    catalog
        .create_provider_connection("local", "docker", "unix:///var/run/docker.sock", "")
        .unwrap()
}

async fn check_detail_access_and_page(app: &Router, authenticated: &str, provider: &str) -> String {
    let anonymous = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);

    let page = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let body = body_text(page, 256 * 1024).await;
    assert!(body.contains("demo-api"));
    assert!(body.contains("hakobi/demo:latest"));
    assert!(body.contains("healthy"));
    assert!(body.contains("/app/server --port 8080"));
    assert!(body.contains("RUST_LOG=info"));
    assert!(body.contains("com.docker.compose.project=hakobi-stack"));
    assert!(body.contains("0.0.0.0:18080"));
    assert!(body.contains("12.5 %"));
    assert!(body.contains("line-0"));
    assert!(body.contains(
        format!("hx-get=\"/containers/{provider}/demo-api/stats\" hx-trigger=\"every 2s\"")
            .as_str()
    ));
    assert!(body.contains(
        format!("hx-get=\"/containers/{provider}/demo-api/logs\" hx-trigger=\"every 2s\"").as_str()
    ));
    body
}

fn check_detail_actions(body: &str, provider: &str) {
    assert!(body.contains(&format!("/containers/{provider}/demo-api/start")));
    assert!(body.contains(&format!("/containers/{provider}/demo-api/stop")));
    assert!(body.contains(&format!("/containers/{provider}/demo-api/restart")));
    assert!(body.contains(&format!(
        "data-delete-action=\"/containers/{provider}/demo-api/delete\""
    )));
    assert!(body.contains("data-delete-name=\"demo-api\""));
    assert!(body.contains("data-confirm-input"));
}

async fn check_detail_list_link(app: &Router, authenticated: &str, provider: &str) {
    let list = app
        .clone()
        .oneshot(
            Request::get("/containers")
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = body_text(list, 256 * 1024).await;
    assert!(list_body.contains(&format!("href=\"/containers/{provider}/demo-api\"")));
}

#[tokio::test]
async fn container_detail_page_renders_metadata_stats_and_logs() {
    let (data, app) = detail_app();
    let (authenticated, _csrf) = sign_in(&app).await;
    let provider = detail_provider(&data);
    let body = check_detail_access_and_page(&app, &authenticated, &provider).await;
    check_detail_actions(&body, &provider);
    check_detail_list_link(&app, &authenticated, &provider).await;
}

async fn check_stats_partial(app: &Router, authenticated: &str, provider: &str) {
    let stats = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api/stats"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stats.status(), StatusCode::OK);
    let stats_body = body_text(stats, 64 * 1024).await;
    assert!(stats_body.contains("Resource usage"));
    assert!(stats_body.contains("12.5 %"));
    assert!(stats_body.contains("1.0 MiB"));
}

async fn check_logs_partial(app: &Router, authenticated: &str, provider: &str) {
    let logs = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api/logs"))
                .header(COOKIE, authenticated)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logs.status(), StatusCode::OK);
    let logs_body = body_text(logs, 64 * 1024).await;
    assert!(logs_body.contains("line-0"));
    assert!(logs_body.contains("line-2"));
}

async fn check_anonymous_logs(app: &Router, provider: &str) {
    let anonymous = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/demo-api/logs"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(anonymous.headers().get("hx-redirect").unwrap(), "/login");
}

#[tokio::test]
async fn container_detail_partials_return_stats_and_logs() {
    let (data, app) = detail_app();
    let (authenticated, _csrf) = sign_in(&app).await;
    let provider = detail_provider(&data);
    check_stats_partial(&app, &authenticated, &provider).await;
    check_logs_partial(&app, &authenticated, &provider).await;
    check_anonymous_logs(&app, &provider).await;
}

#[tokio::test]
async fn container_detail_returns_404_for_missing_containers_and_providers() {
    let (data, app) = detail_app();
    let (authenticated, _csrf) = sign_in(&app).await;
    let provider = detail_provider(&data);

    let missing_container = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/ghost-api"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_container.status(), StatusCode::NOT_FOUND);

    let missing_provider = app
        .clone()
        .oneshot(
            Request::get("/containers/missing/demo-api")
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_provider.status(), StatusCode::NOT_FOUND);

    let bad_name = app
        .clone()
        .oneshot(
            Request::get(format!("/containers/{provider}/bad%2Fname"))
                .header(COOKIE, authenticated.clone())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_name.status(), StatusCode::BAD_REQUEST);
}
