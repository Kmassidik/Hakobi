use std::{future::Future, time::Duration};

use anyhow::Result;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::{
    host_agent::{
        self, DockerContainerInspect, DockerContainerLogs, DockerContainerRow,
        DockerContainerStats, DockerContainersResponse, DockerMetrics, DockerSnapshotResponse,
        DockerVersion, RunContainerRequest, RunContainerResponse,
    },
    providers::{
        validate_container_name, validate_image_reference, ContainerBackend, DockerBackend,
    },
};
use tokio::{process::Command, time::timeout};

use super::{authorized, error_response, truncate_to, unauthorized, AgentState};

const MAX_REPORTED_ROWS: usize = 512;
const MAX_ERROR_DETAIL: usize = 500;
const DOCKER_TIMEOUT: Duration = Duration::from_secs(20);
const DOCKER_RUN_TIMEOUT: Duration = Duration::from_secs(120);
/// Log window served when the dashboard omits `tail`, matching the 100 lines
/// the detail page polls for.
pub(super) const DEFAULT_TAIL_LINES: usize = 100;

pub(super) async fn docker_metrics(
    State(state): State<AgentState>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let output = match docker_stats_output().await {
        Ok(output) => output,
        Err(response) => return *response,
    };
    if !output.status.success() {
        return error_response(
            StatusCode::BAD_GATEWAY,
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Json(parse_docker_metrics(&stdout)).into_response()
}

async fn docker_stats_output() -> std::result::Result<std::process::Output, Box<Response>> {
    let mut child = Command::new("docker");
    child
        .args(["stats", "--no-stream", "--format", "{{json .}}"])
        .kill_on_drop(true);
    match timeout(Duration::from_secs(20), child.output()).await {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(error)) => Err(Box::new(error_response(
            StatusCode::BAD_GATEWAY,
            error.to_string(),
        ))),
        Err(_) => Err(Box::new(error_response(
            StatusCode::GATEWAY_TIMEOUT,
            "docker stats exceeded its 20 second limit",
        ))),
    }
}

fn parse_docker_metrics(stdout: &str) -> DockerMetrics {
    let mut metrics = DockerMetrics::default();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(name) = value.get("Name").and_then(|value| value.as_str()) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        metrics.containers += 1;
        metrics.cpu_percent += value
            .get("CPUPerc")
            .and_then(|value| value.as_str())
            .and_then(parse_percent)
            .unwrap_or(0.0);
        if let Some((used, _)) = value
            .get("MemUsage")
            .and_then(|value| value.as_str())
            .and_then(parse_memory_pair)
        {
            metrics.memory_used_bytes = metrics.memory_used_bytes.saturating_add(used);
        }
    }
    if let Some(total) = host_memory_total() {
        metrics.memory_limit_bytes = total;
    }
    metrics
}

/// Runs a Docker operation against the fixed host socket with a time limit.
async fn docker_call<T>(
    operation: impl Future<Output = Result<T>>,
    limit: Duration,
) -> std::result::Result<T, Box<Response>> {
    match timeout(limit, operation).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(Box::new(docker_failure(error))),
        Err(_) => Err(Box::new(error_response(
            StatusCode::GATEWAY_TIMEOUT,
            "the Docker daemon did not answer in time",
        ))),
    }
}

/// Maps a Docker failure to a safe, bounded error body.
///
/// The full error is logged locally; the response only carries a truncated
/// detail, so no credential or environment value can be reflected back to the
/// caller.
fn docker_failure(error: anyhow::Error) -> Response {
    tracing::warn!(error = %error, "host Docker request failed");
    error_response(
        StatusCode::BAD_GATEWAY,
        truncate_to(&error.to_string(), MAX_ERROR_DETAIL),
    )
}

pub(super) async fn docker_version(
    State(state): State<AgentState>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let backend = DockerBackend;
    match docker_call(
        backend.version(host_agent::DOCKER_SOCKET_ENDPOINT),
        DOCKER_TIMEOUT,
    )
    .await
    {
        Ok((version, api_version)) => Json(DockerVersion {
            version,
            api_version,
        })
        .into_response(),
        Err(response) => *response,
    }
}

pub(super) async fn docker_containers(
    State(state): State<AgentState>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let backend = DockerBackend;
    match docker_call(
        backend.containers(host_agent::DOCKER_SOCKET_ENDPOINT),
        DOCKER_TIMEOUT,
    )
    .await
    {
        Ok(rows) => {
            let containers = rows
                .into_iter()
                .take(MAX_REPORTED_ROWS)
                .map(DockerContainerRow::from)
                .collect::<Vec<_>>();
            Json(DockerContainersResponse { containers }).into_response()
        }
        Err(response) => *response,
    }
}

pub(super) async fn docker_snapshot(
    State(state): State<AgentState>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let backend = DockerBackend;
    match docker_call(
        backend.snapshot(host_agent::DOCKER_SOCKET_ENDPOINT),
        DOCKER_TIMEOUT,
    )
    .await
    {
        Ok(snapshot) => {
            let mut response = DockerSnapshotResponse::from(snapshot);
            response.images.truncate(MAX_REPORTED_ROWS);
            response.volumes.truncate(MAX_REPORTED_ROWS);
            response.networks.truncate(MAX_REPORTED_ROWS);
            Json(response).into_response()
        }
        Err(response) => *response,
    }
}

/// Parses the `tail` query parameter, defaulting to the same window the
/// dashboard asks for and rejecting anything a URL should not carry.
pub(super) fn parse_tail(query: Option<&str>) -> std::result::Result<usize, Box<Response>> {
    let Some((key, value)) =
        query.and_then(|query| form_urlencoded_pairs(query).into_iter().next())
    else {
        return Ok(DEFAULT_TAIL_LINES);
    };
    if key != "tail" {
        return Err(Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "only the tail query parameter is supported",
        )));
    }
    if value.is_empty() || value.len() > 4 || !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "tail must be a number between 1 and 9999",
        )));
    }
    let Ok(tail) = value.parse::<usize>() else {
        return Err(Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "tail must be a number between 1 and 9999",
        )));
    };
    if tail == 0 {
        return Err(Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "tail must be at least 1",
        )));
    }
    Ok(tail)
}

/// Splits a query string into decoded `(key, value)` pairs.
fn form_urlencoded_pairs(query: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(query.as_bytes())
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
}

pub(super) async fn docker_container_inspect(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    if let Err(error) = validate_container_name(&name) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    let backend = DockerBackend;
    match docker_call(
        backend.inspect_container(host_agent::DOCKER_SOCKET_ENDPOINT, &name),
        DOCKER_TIMEOUT,
    )
    .await
    {
        Ok(inspect) => Json(DockerContainerInspect::from(inspect)).into_response(),
        Err(response) => *response,
    }
}

pub(super) async fn docker_container_logs(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    raw_query: axum::extract::RawQuery,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    if let Err(error) = validate_container_name(&name) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    let tail = match parse_tail(raw_query.0.as_deref()) {
        Ok(tail) => tail,
        Err(response) => return *response,
    };
    let backend = DockerBackend;
    match docker_call(
        backend.container_logs(host_agent::DOCKER_SOCKET_ENDPOINT, &name, tail),
        DOCKER_TIMEOUT,
    )
    .await
    {
        Ok(logs) => Json(DockerContainerLogs::from(logs)).into_response(),
        Err(response) => *response,
    }
}

pub(super) async fn docker_container_stats(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    if let Err(error) = validate_container_name(&name) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    let backend = DockerBackend;
    match docker_call(
        backend.container_stats(host_agent::DOCKER_SOCKET_ENDPOINT, &name),
        DOCKER_TIMEOUT,
    )
    .await
    {
        Ok(stats) => Json(DockerContainerStats::from(stats)).into_response(),
        Err(response) => *response,
    }
}

#[derive(Clone, Copy)]
enum ContainerAction {
    Start,
    Stop,
    Restart,
    Delete,
}

async fn container_action(
    state: AgentState,
    headers: HeaderMap,
    name: String,
    action: ContainerAction,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    if let Err(error) = validate_container_name(&name) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    let backend = DockerBackend;
    let endpoint = host_agent::DOCKER_SOCKET_ENDPOINT;
    let result = match action {
        ContainerAction::Start => {
            docker_call(backend.start_container(endpoint, &name), DOCKER_TIMEOUT).await
        }
        ContainerAction::Stop => {
            docker_call(backend.stop_container(endpoint, &name), DOCKER_TIMEOUT).await
        }
        ContainerAction::Restart => {
            docker_call(backend.restart_container(endpoint, &name), DOCKER_TIMEOUT).await
        }
        ContainerAction::Delete => {
            docker_call(backend.remove_container(endpoint, &name), DOCKER_TIMEOUT).await
        }
    };
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(response) => *response,
    }
}

pub(super) async fn docker_start(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    container_action(state, headers, name, ContainerAction::Start).await
}

pub(super) async fn docker_stop(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    container_action(state, headers, name, ContainerAction::Stop).await
}

pub(super) async fn docker_restart(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    container_action(state, headers, name, ContainerAction::Restart).await
}

pub(super) async fn docker_delete(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    container_action(state, headers, name, ContainerAction::Delete).await
}

pub(super) async fn docker_run(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<RunContainerRequest>, JsonRejection>,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let Json(request) = match payload {
        Ok(payload) => payload,
        Err(rejection) => {
            return error_response(StatusCode::BAD_REQUEST, rejection.body_text());
        }
    };
    if let Err(error) = validate_image_reference(&request.image) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    if let Err(error) = validate_container_name(&request.name) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    if request.container_port == 0 {
        return error_response(
            StatusCode::BAD_REQUEST,
            "container port must be between 1 and 65535",
        );
    }
    let backend = DockerBackend;
    match docker_call(
        backend.run_container(
            host_agent::DOCKER_SOCKET_ENDPOINT,
            &request.image,
            &request.name,
            request.container_port,
            &request.env,
        ),
        DOCKER_RUN_TIMEOUT,
    )
    .await
    {
        Ok(result) => (
            StatusCode::CREATED,
            Json(RunContainerResponse::from(result)),
        )
            .into_response(),
        Err(response) => *response,
    }
}

fn host_memory_total() -> Option<u64> {
    let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = contents
        .lines()
        .find(|line| line.starts_with("MemTotal:"))?;
    let kib = line.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(kib.saturating_mul(1024))
}

fn parse_percent(value: &str) -> Option<f64> {
    value.trim().trim_end_matches('%').parse().ok()
}

fn parse_memory_pair(value: &str) -> Option<(u64, u64)> {
    let (used, limit) = value.split_once('/')?;
    Some((parse_memory(used)?, parse_memory(limit)?))
}

fn parse_memory(value: &str) -> Option<u64> {
    let value = value.trim();
    let split = value
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(value.len());
    let number = value[..split].parse::<f64>().ok()?;
    let unit = value[split..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "b" | "" => 1.0,
        "kib" | "kb" => 1024.0,
        "mib" | "mb" => 1024.0 * 1024.0,
        "gib" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "tib" | "tb" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * multiplier) as u64)
}
