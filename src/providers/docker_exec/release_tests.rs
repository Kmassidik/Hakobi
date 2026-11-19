use std::collections::HashMap;
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

use super::{release, run_image};
use crate::providers::{
    validate_shell_command, validate_volumes, ContainerVolume, MAX_SHELL_COMMAND_CHARS,
};

#[derive(Default)]
struct FakeDockerState {
    create_bodies: Vec<Value>,
    exit_code: i64,
    logs: Vec<u8>,
    list_calls: usize,
}

async fn fake_docker(
    State(state): State<Arc<Mutex<FakeDockerState>>>,
    request: Request<Body>,
) -> Response {
    let path = request.uri().path().to_owned();
    if path.ends_with("/containers/create") {
        let bytes = to_bytes(request.into_body(), 65_536).await.unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        state.lock().unwrap().create_bodies.push(body);
        return (
            StatusCode::CREATED,
            Json(json!({"Id": "runtime-id", "Warnings": []})),
        )
            .into_response();
    }
    if path.ends_with("/start") {
        return StatusCode::NO_CONTENT.into_response();
    }
    if path.ends_with("/wait") {
        let exit_code = state.lock().unwrap().exit_code;
        return Json(json!({"StatusCode": exit_code})).into_response();
    }
    if path.ends_with("/logs") {
        let logs = state.lock().unwrap().logs.clone();
        return Body::from(docker_log_frame(&logs)).into_response();
    }
    if path.ends_with("/containers/json") {
        state.lock().unwrap().list_calls += 1;
        return Json(json!([])).into_response();
    }
    (
        StatusCode::NOT_FOUND,
        Json(json!({"message": format!("unexpected Docker path: {path}")})),
    )
        .into_response()
}

fn docker_log_frame(logs: &[u8]) -> Vec<u8> {
    let mut framed = vec![1, 0, 0, 0];
    framed.extend((logs.len() as u32).to_be_bytes());
    framed.extend(logs);
    framed
}

async fn spawn_fake_docker(exit_code: i64, logs: &[u8]) -> (String, Arc<Mutex<FakeDockerState>>) {
    let state = Arc::new(Mutex::new(FakeDockerState {
        exit_code,
        logs: logs.to_vec(),
        ..Default::default()
    }));
    let app = Router::new()
        .fallback(any(fake_docker))
        .with_state(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("tcp://{address}"), state)
}

fn volume() -> ContainerVolume {
    ContainerVolume {
        name: "app-data".into(),
        mount_path: "/var/lib/app".into(),
        read_only: true,
    }
}

#[tokio::test]
async fn release_is_private_auto_removed_and_preserves_nonzero_status() {
    let (endpoint, state) = spawn_fake_docker(17, b"migration failed\n").await;
    let env = HashMap::from([("DATABASE_URL".into(), "postgres://db/app".into())]);
    let result = release(
        &endpoint,
        "example/app:1.2",
        "app-release",
        &env,
        &[volume()],
        "exit 17",
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 17);
    assert_eq!(result.logs, "migration failed\n");

    let state = state.lock().unwrap();
    let body = &state.create_bodies[0];
    assert_eq!(
        body.pointer("/Image").and_then(Value::as_str),
        Some("example/app:1.2")
    );
    assert_eq!(
        body.pointer("/Cmd"),
        Some(&json!(["/bin/sh", "-c", "exit 17"]))
    );
    assert!(body.get("Entrypoint").is_none_or(Value::is_null));
    assert_eq!(body.pointer("/HostConfig/AutoRemove"), Some(&json!(true)));
    assert!(body
        .pointer("/HostConfig/PortBindings")
        .is_none_or(Value::is_null));
    assert!(body.get("ExposedPorts").is_none_or(Value::is_null));
    assert_eq!(
        body.pointer("/HostConfig/Mounts/0"),
        Some(&json!({
            "Source": "app-data",
            "Target": "/var/lib/app",
            "Type": "volume",
            "ReadOnly": true
        }))
    );
    assert!(body
        .pointer("/Env")
        .and_then(Value::as_array)
        .unwrap()
        .contains(&json!("DATABASE_URL=postgres://db/app")));
}

#[tokio::test]
async fn private_worker_uses_the_same_command_and_volume_options() {
    let (endpoint, state) = spawn_fake_docker(0, b"").await;
    let result = run_image(
        &endpoint,
        "example/worker:latest",
        "app-worker",
        None,
        None,
        &Default::default(),
        Default::default(),
        Some("exec /bin/worker --consume"),
        &[volume()],
    )
    .await
    .unwrap();
    assert_eq!(result.host_port, None);
    assert_eq!(result.container_id, "runtime-id");
    let state = state.lock().unwrap();
    let body = &state.create_bodies[0];
    assert_eq!(state.list_calls, 0);
    assert_eq!(
        body.pointer("/Cmd"),
        Some(&json!(["/bin/sh", "-c", "exec /bin/worker --consume"]))
    );
    assert!(body.get("ExposedPorts").is_none_or(Value::is_null));
    assert!(body
        .pointer("/HostConfig/PortBindings")
        .is_none_or(Value::is_null));
    assert!(body
        .pointer("/HostConfig/AutoRemove")
        .is_none_or(Value::is_null));
    assert_eq!(
        body.pointer("/HostConfig/Mounts/0/Type"),
        Some(&json!("volume"))
    );
}

#[tokio::test]
async fn release_logs_stop_at_the_byte_ceiling() {
    let (endpoint, _) = spawn_fake_docker(0, &vec![b'x'; 70 * 1024]).await;
    let result = release(
        &endpoint,
        "alpine:3.20",
        "app-release",
        &Default::default(),
        &[],
        "generate-output",
    )
    .await
    .unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.logs.len(), super::containers::MAX_RELEASE_LOG_BYTES);
}

#[test]
fn shell_commands_reject_nul_and_count_unicode_characters() {
    assert!(validate_shell_command(&"é".repeat(MAX_SHELL_COMMAND_CHARS)).is_ok());
    assert!(validate_shell_command(&"é".repeat(MAX_SHELL_COMMAND_CHARS + 1)).is_err());
    assert!(validate_shell_command("echo before\0after").is_err());
}

#[test]
fn volumes_reject_bind_sources_and_unclean_destinations() {
    for name in ["", "/var/lib/docker", "../data", "data:bind", "bad name"] {
        let mut value = volume();
        value.name = name.into();
        assert!(validate_volumes(&[value]).is_err(), "name: {name}");
    }
    for path in [
        "relative",
        "../data",
        "/data/../root",
        "/data/./state",
        "/data//state",
        "/data/",
    ] {
        let mut value = volume();
        value.mount_path = path.into();
        assert!(validate_volumes(&[value]).is_err(), "path: {path}");
    }
    assert!(validate_volumes(&[volume()]).is_ok());
}
