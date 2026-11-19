use std::sync::{Arc, Mutex};

use axum::{
    body::{to_bytes, Body},
    extract::State,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Json, Router,
};
use serde_json::{json, Value};

use super::compose::{
    compose_up_args, validate_compose_project, validate_compose_yaml, MAX_COMPOSE_BYTES,
};
use super::containers::{
    container_config, remove_options, validate_container_name, validate_container_port,
    validate_image_reference,
};
use super::ports::is_port_collision;
use super::{apply_compose, choose_free_port, run_image, PORT_RANGE_END, PORT_RANGE_START};
use crate::providers::ResourceLimits;

#[test]
fn free_port_starts_at_range_start() {
    assert_eq!(choose_free_port(std::iter::empty()), PORT_RANGE_START);
}

#[test]
fn free_port_skips_used_ports() {
    assert_eq!(choose_free_port([18000, 18002]), 18001);
    assert_eq!(choose_free_port(18000..18197), 18197);
}

#[test]
fn free_port_ignores_out_of_range_ports() {
    assert_eq!(choose_free_port([1, 2, 65535]), PORT_RANGE_START);
}

#[test]
fn free_port_falls_back_to_start_when_range_is_full() {
    assert_eq!(
        choose_free_port(PORT_RANGE_START..PORT_RANGE_END),
        PORT_RANGE_START
    );
}

#[test]
fn image_references_are_validated() {
    for image in [
        "nginx",
        "nginx:1.27",
        "ghcr.io/acme/app:v1.2.3",
        "localhost:5000/team/app@sha256:abc123",
    ] {
        assert!(validate_image_reference(image).is_ok(), "{image}");
    }
    for image in [
        "",
        "-rm",
        "nginx latest",
        "nginx;rm",
        "nginx\nlatest",
        "../app",
        "app//x",
    ] {
        assert!(validate_image_reference(image).is_err(), "{image}");
    }
}

#[test]
fn container_names_are_tightly_scoped() {
    for name in ["web", "web-1", "web_1.blue", "1web"] {
        assert!(validate_container_name(name).is_ok(), "{name}");
    }
    for name in ["", "-web", "web/evil", "web evil", "../web"] {
        assert!(validate_container_name(name).is_err(), "{name}");
    }
}

#[test]
fn container_ports_exclude_zero() {
    assert!(validate_container_port(0).is_err());
    assert!(validate_container_port(1).is_ok());
    assert!(validate_container_port(80).is_ok());
    assert!(validate_container_port(u16::MAX).is_ok());
}

#[test]
fn compose_project_is_validated() {
    for project in ["app", "my_app", "app-2", "1app"] {
        assert!(validate_compose_project(project).is_ok(), "{project}");
    }
    for project in ["", "MyApp", "-app", "app two", "app/2"] {
        assert!(validate_compose_project(project).is_err(), "{project}");
    }
}

#[test]
fn compose_file_is_validated() {
    assert!(validate_compose_yaml("services: {}").is_ok());
    assert!(validate_compose_yaml("   ").is_err());
    assert!(validate_compose_yaml("services:\0").is_err());
    assert!(validate_compose_yaml(&"a".repeat(MAX_COMPOSE_BYTES + 1)).is_err());
}

#[test]
fn compose_command_is_fixed_and_uses_stdin() {
    assert_eq!(
        compose_up_args("myapp"),
        vec![
            "compose",
            "-f",
            "-",
            "-p",
            "myapp",
            "up",
            "-d",
            "--remove-orphans"
        ]
    );
}

#[test]
fn compose_command_keeps_project_as_one_argument() {
    let args = compose_up_args("app-x");
    assert_eq!(args.iter().filter(|arg| *arg == "app-x").count(), 1);
    assert!(!args.iter().any(|arg| arg.contains(' ')));
}

#[test]
fn port_collision_messages_are_recognized() {
    let collision = bollard::errors::Error::DockerResponseServerError {
        status_code: 500,
        message: "Bind for 0.0.0.0:18000 failed: port is already allocated".into(),
    };
    assert!(is_port_collision(&collision));

    let conflict = bollard::errors::Error::DockerResponseServerError {
        status_code: 409,
        message: "Conflict. The container name is already in use".into(),
    };
    assert!(!is_port_collision(&conflict));

    let missing_image = bollard::errors::Error::DockerResponseServerError {
        status_code: 404,
        message: "No such image: nginx".into(),
    };
    assert!(!is_port_collision(&missing_image));
}

#[test]
fn remove_options_keep_volumes() {
    let options = remove_options();
    assert!(options.force);
    assert!(!options.v);
}

#[tokio::test]
async fn invalid_inputs_fail_before_connecting() {
    let error = run_image(
        "tcp://127.0.0.1:1",
        "bad image",
        "web",
        Some(80),
        None,
        &Default::default(),
        Default::default(),
        None,
        &[],
    )
    .await
    .expect_err("invalid image");
    assert!(error.to_string().contains("image reference"), "{error}");

    let error = run_image(
        "tcp://127.0.0.1:1",
        "nginx",
        "bad name",
        Some(80),
        None,
        &Default::default(),
        Default::default(),
        None,
        &[],
    )
    .await
    .expect_err("invalid name");
    assert!(error.to_string().contains("container name"), "{error}");

    let error = run_image(
        "tcp://127.0.0.1:1",
        "nginx",
        "web",
        Some(0),
        None,
        &Default::default(),
        Default::default(),
        None,
        &[],
    )
    .await
    .expect_err("invalid port");
    assert!(error.to_string().contains("container port"), "{error}");
}

#[tokio::test]
async fn compose_inputs_fail_before_invoking_docker() {
    let error = apply_compose("unix:///var/run/docker.sock", "Bad Project", "services: {}")
        .await
        .expect_err("invalid project");
    assert!(error.to_string().contains("Compose project"), "{error}");

    let error = apply_compose("unix:///var/run/docker.sock", "myapp", "  ")
        .await
        .expect_err("empty compose file");
    assert!(error.to_string().contains("must not be empty"), "{error}");
}

#[test]
fn container_config_binds_exposed_port() {
    let config = container_config(
        "nginx",
        Some(80),
        Some(18000),
        &Default::default(),
        ResourceLimits {
            cpu: Some(1.5),
            memory_bytes: Some(512 * 1024 * 1024),
        },
        None,
        &[],
    );
    assert_eq!(config.image.as_deref(), Some("nginx"));
    assert_eq!(config.cmd, None);
    assert_eq!(
        config.exposed_ports.as_deref(),
        Some(&["80/tcp".to_owned()][..])
    );
    let host = config.host_config.expect("host config");
    assert_eq!(host.mounts, None);
    assert_eq!(host.nano_cpus, Some(1_500_000_000));
    assert_eq!(host.memory, Some(512 * 1024 * 1024));
    let binding = host
        .port_bindings
        .expect("port bindings")
        .get("80/tcp")
        .cloned()
        .flatten()
        .expect("binding");
    assert_eq!(binding.len(), 1);
    assert_eq!(binding[0].host_ip.as_deref(), Some("127.0.0.1"));
    assert_eq!(binding[0].host_port.as_deref(), Some("18000"));
}

#[derive(Default)]
struct FakeDockerState {
    create_bodies: Vec<Value>,
    collisions_remaining: usize,
    collide_on_start: bool,
    list_calls: usize,
}

fn created_host_port(body: &Value) -> u16 {
    body.pointer("/HostConfig/PortBindings/80~1tcp/0/HostPort")
        .and_then(Value::as_str)
        .expect("host port in Docker create body")
        .parse()
        .unwrap()
}

async fn fake_create_container(
    state: &Arc<Mutex<FakeDockerState>>,
    request: Request<Body>,
) -> Response {
    let bytes = to_bytes(request.into_body(), 65_536).await.unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    let host_port = created_host_port(&body);
    let collide = {
        let mut state = state.lock().unwrap();
        state.create_bodies.push(body);
        if state.collisions_remaining == 0 {
            false
        } else {
            state.collisions_remaining -= 1;
            true
        }
    };
    if collide {
        let message = format!("Bind for 127.0.0.1:{host_port} failed: port is already allocated");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "message": message })),
        )
            .into_response();
    }
    (
        StatusCode::CREATED,
        Json(json!({ "Id": "container-id", "Warnings": [] })),
    )
        .into_response()
}

async fn fake_docker_request(
    State(state): State<Arc<Mutex<FakeDockerState>>>,
    request: Request<Body>,
) -> Response {
    let path = request.uri().path().to_owned();
    if path.ends_with("/containers/json") {
        state.lock().unwrap().list_calls += 1;
        return Json(json!([])).into_response();
    }
    if path.ends_with("/containers/create") {
        return fake_create_container(&state, request).await;
    }
    if path.ends_with("/start") {
        if state.lock().unwrap().collide_on_start {
            let message = "Bind for 127.0.0.1:18080 failed: port is already allocated";
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "message": message })),
            )
                .into_response();
        }
        return StatusCode::NO_CONTENT.into_response();
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "message": format!("unexpected Docker path: {path}") })),
    )
        .into_response()
}

async fn spawn_fake_docker(
    collisions: usize,
    collide_on_start: bool,
) -> (String, Arc<Mutex<FakeDockerState>>) {
    let state = Arc::new(Mutex::new(FakeDockerState {
        collisions_remaining: collisions,
        collide_on_start,
        ..Default::default()
    }));
    let app = Router::new()
        .fallback(any(fake_docker_request))
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("tcp://{address}"), state)
}

#[tokio::test]
async fn custom_host_port_uses_the_exact_loopback_mapping() {
    let (endpoint, state) = spawn_fake_docker(0, false).await;
    let result = run_image(
        &endpoint,
        "nginx",
        "web",
        Some(80),
        Some(18080),
        &Default::default(),
        Default::default(),
        None,
        &[],
    )
    .await
    .unwrap();
    assert_eq!(result.host_port, Some(18080));
    let state = state.lock().unwrap();
    assert_eq!(state.list_calls, 0);
    assert_eq!(state.create_bodies.len(), 1);
    assert_eq!(created_host_port(&state.create_bodies[0]), 18080);
    assert_eq!(
        state.create_bodies[0]
            .pointer("/HostConfig/PortBindings/80~1tcp/0/HostIp")
            .and_then(Value::as_str),
        Some("127.0.0.1")
    );
}

#[tokio::test]
async fn automatic_host_port_retries_the_next_candidate() {
    let (endpoint, state) = spawn_fake_docker(1, false).await;
    let result = run_image(
        &endpoint,
        "nginx",
        "web",
        Some(80),
        None,
        &Default::default(),
        Default::default(),
        None,
        &[],
    )
    .await
    .unwrap();
    assert_eq!(result.host_port, Some(18001));
    let state = state.lock().unwrap();
    assert_eq!(state.list_calls, 1);
    let ports: Vec<u16> = state.create_bodies.iter().map(created_host_port).collect();
    assert_eq!(ports, vec![18000, 18001]);
}

#[tokio::test]
async fn custom_host_port_collision_never_falls_back() {
    let (endpoint, state) = spawn_fake_docker(1, false).await;
    let error = run_image(
        &endpoint,
        "nginx",
        "web",
        Some(80),
        Some(18080),
        &Default::default(),
        Default::default(),
        None,
        &[],
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requested host port 18080 is unavailable"),
        "{error}"
    );
    let state = state.lock().unwrap();
    assert_eq!(state.list_calls, 0);
    assert_eq!(state.create_bodies.len(), 1);
    assert_eq!(created_host_port(&state.create_bodies[0]), 18080);
}

#[tokio::test]
async fn custom_host_port_start_collision_is_clear_and_final() {
    let (endpoint, state) = spawn_fake_docker(0, true).await;
    let error = run_image(
        &endpoint,
        "nginx",
        "web",
        Some(80),
        Some(18080),
        &Default::default(),
        Default::default(),
        None,
        &[],
    )
    .await
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requested host port 18080 is unavailable"),
        "{error}"
    );
    assert_eq!(state.lock().unwrap().create_bodies.len(), 1);
}

#[tokio::test]
async fn requested_host_port_zero_fails_before_connecting() {
    let error = run_image(
        "tcp://127.0.0.1:1",
        "nginx",
        "web",
        Some(80),
        Some(0),
        &Default::default(),
        Default::default(),
        None,
        &[],
    )
    .await
    .expect_err("invalid requested host port");
    assert!(error.to_string().contains("requested host port"), "{error}");
}
