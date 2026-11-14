use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};

use crate::host_agent::{
    AgentErrorBody, DockerContainerInspect, DockerContainerLogs, DockerContainerMount,
    DockerContainerPort, DockerContainerRow, DockerContainerStats, DockerContainersResponse,
    DockerHostInfo, DockerImageRow, DockerNetworkRow, DockerSnapshotResponse, DockerVersion,
    DockerVolumeRow, RunContainerRequest, RunContainerResponse,
};

/// Minimal in-process stand-in for the host agent Docker API.
#[derive(Clone)]
struct FakeAgent {
    token: String,
}

impl FakeAgent {
    fn authorized(&self, headers: &HeaderMap) -> bool {
        let expected = format!("Bearer {}", self.token);
        headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value == expected)
    }

    fn unauthorized_response() -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(AgentErrorBody {
                error: "unauthorized".into(),
            }),
        )
            .into_response()
    }
}

async fn fake_version(State(agent): State<FakeAgent>, headers: HeaderMap) -> Response {
    if !agent.authorized(&headers) {
        return FakeAgent::unauthorized_response();
    }
    Json(DockerVersion {
        version: "28.0.1".into(),
        api_version: "1.47".into(),
    })
    .into_response()
}

async fn fake_containers(State(agent): State<FakeAgent>, headers: HeaderMap) -> Response {
    if !agent.authorized(&headers) {
        return FakeAgent::unauthorized_response();
    }
    Json(DockerContainersResponse {
        containers: vec![DockerContainerRow {
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
        }],
    })
    .into_response()
}

async fn fake_snapshot(State(agent): State<FakeAgent>, headers: HeaderMap) -> Response {
    if !agent.authorized(&headers) {
        return FakeAgent::unauthorized_response();
    }
    Json(DockerSnapshotResponse {
        host: DockerHostInfo {
            name: "fedora-test".into(),
            server_version: "28.0.1".into(),
            containers: 2,
            running: 1,
            images: 1,
            operating_system: "Fedora Linux 44".into(),
            architecture: "x86_64".into(),
            cpus: 16,
        },
        images: vec![DockerImageRow {
            id: "sha256:abcdef".into(),
            tags: vec!["alpine:3.20".into()],
            size_bytes: 1_048_576,
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
    })
    .into_response()
}

async fn fake_action(
    State(agent): State<FakeAgent>,
    headers: HeaderMap,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    if !agent.authorized(&headers) {
        return FakeAgent::unauthorized_response();
    }
    if name != "demo-api" {
        return (
            StatusCode::NOT_FOUND,
            Json(AgentErrorBody {
                error: "no such container".into(),
            }),
        )
            .into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

async fn fake_inspect(
    State(agent): State<FakeAgent>,
    headers: HeaderMap,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    if !agent.authorized(&headers) {
        return FakeAgent::unauthorized_response();
    }
    if name != "demo-api" {
        return (
            StatusCode::NOT_FOUND,
            Json(AgentErrorBody {
                error: "no such container".into(),
            }),
        )
            .into_response();
    }
    detail_response().into_response()
}

/// Fixed inspect payload served by the fake agent endpoint.
fn detail_response() -> Json<DockerContainerInspect> {
    Json(DockerContainerInspect {
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
        ports: vec![DockerContainerPort {
            host_ip: Some("0.0.0.0".into()),
            host_port: Some(18080),
            container_port: 8080,
            protocol: "tcp".into(),
        }],
        mounts: vec![DockerContainerMount {
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
    })
}

async fn fake_logs(
    State(agent): State<FakeAgent>,
    headers: HeaderMap,
    axum::extract::Path(name): axum::extract::Path<String>,
    raw_query: axum::extract::RawQuery,
) -> Response {
    if !agent.authorized(&headers) {
        return FakeAgent::unauthorized_response();
    }
    if name != "demo-api" {
        return (
            StatusCode::NOT_FOUND,
            Json(AgentErrorBody {
                error: "no such container".into(),
            }),
        )
            .into_response();
    }
    let tail = raw_query
        .0
        .as_deref()
        .and_then(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(key, _)| key == "tail")
                .and_then(|(_, value)| value.parse::<usize>().ok())
        })
        .unwrap_or(100);
    let lines = (0..tail.min(5))
        .map(|index| format!("line-{index}"))
        .collect::<Vec<_>>();
    Json(DockerContainerLogs {
        lines,
        truncated: false,
    })
    .into_response()
}

async fn fake_stats(
    State(agent): State<FakeAgent>,
    headers: HeaderMap,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    if !agent.authorized(&headers) {
        return FakeAgent::unauthorized_response();
    }
    if name != "demo-api" {
        return (
            StatusCode::NOT_FOUND,
            Json(AgentErrorBody {
                error: "no such container".into(),
            }),
        )
            .into_response();
    }
    Json(DockerContainerStats {
        cpu_percent: 12.5,
        memory_used_bytes: 1_048_576,
        memory_limit_bytes: 8_388_608,
        network_rx_bytes: 4096,
        network_tx_bytes: 8192,
    })
    .into_response()
}

async fn fake_run(
    State(agent): State<FakeAgent>,
    headers: HeaderMap,
    Json(request): Json<RunContainerRequest>,
) -> Response {
    if !agent.authorized(&headers) {
        return FakeAgent::unauthorized_response();
    }
    (
        StatusCode::CREATED,
        Json(RunContainerResponse {
            container_id: format!("id-{}", request.name),
            host_port: 18000,
            name: request.name,
        }),
    )
        .into_response()
}

pub(super) async fn spawn_fake_agent(token: &str) -> String {
    let app = Router::new()
        .route("/v1/docker/version", get(fake_version))
        .route("/v1/docker/containers", get(fake_containers))
        .route("/v1/docker/snapshot", get(fake_snapshot))
        .route("/v1/docker/containers/run", post(fake_run))
        .route("/v1/docker/containers/{name}/inspect", get(fake_inspect))
        .route("/v1/docker/containers/{name}/logs", get(fake_logs))
        .route("/v1/docker/containers/{name}/stats", get(fake_stats))
        .route("/v1/docker/containers/{name}/start", post(fake_action))
        .route("/v1/docker/containers/{name}/stop", post(fake_action))
        .route("/v1/docker/containers/{name}/restart", post(fake_action))
        .route("/v1/docker/containers/{name}/delete", post(fake_action))
        .with_state(FakeAgent {
            token: token.to_owned(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{address}")
}
