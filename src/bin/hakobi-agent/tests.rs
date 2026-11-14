use axum::{
    body::{to_bytes, Body},
    http::{header::CONTENT_TYPE, Method, Request},
};
use tower::ServiceExt;

use super::{
    docker::{parse_tail, DEFAULT_TAIL_LINES},
    install::installer,
    sampler::SystemSampler,
    *,
};

/// Token long enough to satisfy the agent's minimum length rule.
const TEST_TOKEN: &str = "test-token-0123456789abcdef";

fn state() -> AgentState {
    AgentState {
        token: Arc::from(TEST_TOKEN),
        jobs: Arc::new(Mutex::new(HashMap::new())),
        system: {
            let sampler = Arc::new(Mutex::new(SystemSampler::new()));
            tokio::spawn(SystemSampler::run_refresh_loop(Arc::clone(&sampler)));
            sampler
        },
        build: agent_build::BuildState {
            runs_root: std::env::temp_dir().join("hakobi-agent-tests"),
            slots: Arc::new(tokio::sync::Semaphore::new(1)),
        },
    }
}

fn docker_routes() -> Vec<(Method, &'static str)> {
    vec![
        (Method::GET, "/v1/docker/version"),
        (Method::GET, "/v1/docker/containers"),
        (Method::GET, "/v1/docker/containers/demo-api/inspect"),
        (Method::GET, "/v1/docker/containers/demo-api/logs"),
        (Method::GET, "/v1/docker/containers/demo-api/stats"),
        (Method::GET, "/v1/docker/snapshot"),
        (Method::POST, "/v1/docker/containers/demo-api/start"),
        (Method::POST, "/v1/docker/containers/demo-api/stop"),
        (Method::POST, "/v1/docker/containers/demo-api/restart"),
        (Method::POST, "/v1/docker/containers/demo-api/delete"),
        (Method::POST, "/v1/docker/containers/run"),
    ]
}

fn request(method: Method, uri: &str, token: Option<&str>) -> Request<Body> {
    let builder = Request::builder().method(method).uri(uri);
    let builder = match token {
        Some(token) => builder.header(AUTHORIZATION, format!("Bearer {token}")),
        None => builder,
    };
    builder.body(Body::empty()).unwrap()
}

fn json_request(uri: &str, token: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}

async fn status_of(request: Request<Body>) -> StatusCode {
    app(state()).oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn docker_endpoints_require_the_bearer_token() {
    for (method, uri) in docker_routes() {
        assert_eq!(
            status_of(request(method.clone(), uri, None)).await,
            StatusCode::UNAUTHORIZED,
            "{method} {uri} without a token"
        );
        assert_eq!(
            status_of(request(
                method.clone(),
                uri,
                Some("wrong-token-0123456789abcdef")
            ))
            .await,
            StatusCode::UNAUTHORIZED,
            "{method} {uri} with a wrong token"
        );
    }
}

#[tokio::test]
async fn docker_run_requires_the_bearer_token_even_with_an_invalid_body() {
    assert_eq!(
        status_of(json_request(
            "/v1/docker/containers/run",
            "wrong-token-0123456789abcdef",
            "not json"
        ))
        .await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn docker_container_names_are_validated_before_docker_is_contacted() {
    for action in ["start", "stop", "restart", "delete"] {
        let uri = format!("/v1/docker/containers/bad%20name/{action}");
        let response = app(state())
            .oneshot(request(Method::POST, &uri, Some(TEST_TOKEN)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let body: AgentErrorBody = serde_json::from_slice(&body).unwrap();
        assert!(body.error.contains("container name"), "{uri}");
    }
}

#[tokio::test]
async fn docker_run_rejects_invalid_requests_before_touching_docker() {
    let cases = [
        r#"{"image":"alpine latest","name":"demo-api","container_port":8080}"#,
        r#"{"image":"alpine;rm -rf /","name":"demo-api","container_port":8080}"#,
        r#"{"image":"alpine:3.20","name":"bad name","container_port":8080}"#,
        r#"{"image":"alpine:3.20","name":"demo-api","container_port":0}"#,
        r#"{"image":42}"#,
    ];
    for body in cases {
        let response = app(state())
            .oneshot(json_request("/v1/docker/containers/run", TEST_TOKEN, body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let error: AgentErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert!(!error.error.is_empty(), "{body}");
    }
}

#[tokio::test]
async fn container_detail_routes_validate_names_before_docker() {
    for path in [
        "/v1/docker/containers/bad%20name/inspect",
        "/v1/docker/containers/bad%2Fname/logs",
        "/v1/docker/containers/bad%20name/stats",
    ] {
        let response = app(state())
            .oneshot(request(Method::GET, path, Some(TEST_TOKEN)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let error: AgentErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert!(error.error.contains("container name"), "{path}");
    }
}

#[tokio::test]
async fn container_logs_validate_the_tail_parameter() {
    // A valid window passes validation; whether Docker then answers is the
    // daemon's business, so the assertion excludes validation errors only.
    let valid = app(state())
        .oneshot(request(
            Method::GET,
            "/v1/docker/containers/demo-api/logs?tail=25",
            Some(TEST_TOKEN),
        ))
        .await
        .unwrap();
    assert_ne!(valid.status(), StatusCode::BAD_REQUEST);

    for uri in [
        "/v1/docker/containers/demo-api/logs?tail=0",
        "/v1/docker/containers/demo-api/logs?tail=abc",
        "/v1/docker/containers/demo-api/logs?tail=99999",
        "/v1/docker/containers/demo-api/logs?other=1",
    ] {
        let response = app(state())
            .oneshot(request(Method::GET, uri, Some(TEST_TOKEN)))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{uri}");
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let error: AgentErrorBody = serde_json::from_slice(&bytes).unwrap();
        assert!(!error.error.is_empty(), "{uri}");
    }
}

#[test]
fn parse_tail_defaults_and_rejects_out_of_range_windows() {
    assert_eq!(parse_tail(None).unwrap(), DEFAULT_TAIL_LINES);
    assert_eq!(parse_tail(Some("")).unwrap(), DEFAULT_TAIL_LINES);
    assert_eq!(parse_tail(Some("tail=25")).unwrap(), 25);
    assert!(parse_tail(Some("tail=0")).is_err());
    assert!(parse_tail(Some("tail=-1")).is_err());
    assert!(parse_tail(Some("tail=99999")).is_err());
    assert!(parse_tail(Some("tail=")).is_err());
    assert!(parse_tail(Some("since=7")).is_err());
}

#[test]
fn bounded_truncation_keeps_character_boundaries() {
    assert_eq!(truncate_to("short", 32), "short");
    assert_eq!(truncate_to("abcdef", 3), "abc…");
    let unicode = "ü".repeat(4);
    assert_eq!(truncate_to(&unicode, 3), "ü…");
}

#[test]
fn installers_stay_fixed_and_finite() {
    assert!(installer("docker").is_some());
    assert!(installer("compose").is_some());
    assert!(installer("kubectl").is_some());
    assert!(installer("nix").is_some());
    assert!(installer("nixpacks").is_some());
    assert!(installer("sh -c whoami").is_none());
    assert!(installer("").is_none());
}
