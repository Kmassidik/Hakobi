//! Privileged host agent for Hakobi.
//!
//! This binary runs on the host, never inside the Hakobi dashboard container.
//! It exposes a tiny localhost-only HTTP API protected by a bearer token. Every
//! mutating endpoint maps to a fixed, embedded installer; request data is never
//! interpolated into a shell.

#![forbid(unsafe_code)]

use std::{
    collections::HashMap,
    env,
    future::Future,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{bail, Context, Result};
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use hakobi::{
    host_agent::{
        self, AgentErrorBody, DockerContainerInspect, DockerContainerLogs, DockerContainerRow,
        DockerContainerStats, DockerContainersResponse, DockerMetrics, DockerSnapshotResponse,
        DockerVersion, InstallJob, RunContainerRequest, RunContainerResponse,
    },
    providers::{
        validate_container_name, validate_image_reference, ContainerBackend, DockerBackend,
    },
};
use subtle::ConstantTimeEq;

use hakobi::agent_build;
use sysinfo::{Disks, Networks, System};
use tokio::{net::TcpListener, process::Command, sync::Mutex, time::timeout};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const MAX_REPORTED_OUTPUT: usize = 8 * 1024;
const MAX_REPORTED_ROWS: usize = 512;
const MAX_ERROR_DETAIL: usize = 500;
const DOCKER_TIMEOUT: Duration = Duration::from_secs(20);
const DOCKER_RUN_TIMEOUT: Duration = Duration::from_secs(120);
/// Log window served when the dashboard omits `tail`, matching the 100 lines
/// the detail page polls for.
const DEFAULT_TAIL_LINES: usize = 100;

#[derive(Clone)]
struct AgentState {
    token: Arc<str>,
    jobs: Arc<Mutex<HashMap<String, InstallJob>>>,
    system: Arc<Mutex<SystemSampler>>,
    build: agent_build::BuildState,
}

impl axum::extract::FromRef<AgentState> for agent_build::BuildState {
    fn from_ref(state: &AgentState) -> Self {
        state.build.clone()
    }
}

struct SystemSampler {
    system: System,
    networks: Networks,
    last_sample: Instant,
    cached: hakobi::host_agent::SystemMetrics,
}

/// How often the background loop refreshes counters. Fixed 2s window gives
/// stable CPU/network deltas like btop, independent of request timing.
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

impl SystemSampler {
    fn new() -> Self {
        let mut system = System::new_all();
        system.refresh_all();
        let mut networks = Networks::new_with_refreshed_list();
        // Warm the delta baseline with a short sleep so the first snapshot
        // has a real window instead of a near-zero one.
        std::thread::sleep(std::time::Duration::from_millis(200));
        system.refresh_all();
        networks.refresh(true);
        let mut sampler = Self {
            system,
            networks,
            last_sample: Instant::now(),
            cached: hakobi::host_agent::SystemMetrics::default(),
        };
        sampler.cached = sampler.measure();
        sampler
    }

    /// Measures counters over the window since the last measure call.
    fn measure(&mut self) -> hakobi::host_agent::SystemMetrics {
        let elapsed = self.last_sample.elapsed().as_secs_f64().max(0.001);
        self.system.refresh_all();
        self.networks.refresh(true);
        self.last_sample = Instant::now();
        let load = System::load_average();
        let cpus = self.system.cpus();
        let processes = self
            .system
            .processes()
            .values()
            .map(|process| hakobi::host_agent::SystemProcess {
                pid: process.pid().as_u32(),
                user: process
                    .user_id()
                    .map(|user| user.to_string())
                    .unwrap_or_default(),
                cpu_percent: process.cpu_usage(),
                memory_percent: process.memory() as f32 / self.system.total_memory().max(1) as f32
                    * 100.0,
                command: process.name().to_string_lossy().into_owned(),
            })
            .collect::<Vec<_>>();
        let mut processes = processes;
        processes.sort_by(|left, right| {
            right
                .cpu_percent
                .partial_cmp(&left.cpu_percent)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        processes.truncate(20);
        let disks = Disks::new_with_refreshed_list()
            .list()
            .iter()
            .filter(|disk| disk.total_space() > 0)
            .map(|disk| hakobi::host_agent::SystemDisk {
                name: disk.name().to_string_lossy().into_owned(),
                mount: disk.mount_point().to_string_lossy().into_owned(),
                total_bytes: disk.total_space(),
                available_bytes: disk.available_space(),
            })
            .collect();
        let networks = self
            .networks
            .iter()
            .map(|(name, network)| hakobi::host_agent::SystemNetwork {
                name: name.clone(),
                received_bytes_per_second: (network.received() as f64 / elapsed) as u64,
                transmitted_bytes_per_second: (network.transmitted() as f64 / elapsed) as u64,
                total_received_bytes: network.total_received(),
                total_transmitted_bytes: network.total_transmitted(),
            })
            .collect();
        hakobi::host_agent::SystemMetrics {
            hostname: System::host_name().unwrap_or_default(),
            os_name: System::name().unwrap_or_default(),
            os_version: System::os_version().unwrap_or_default(),
            kernel_version: System::kernel_version().unwrap_or_default(),
            cpu_model: cpus
                .first()
                .map(|cpu| cpu.brand().to_owned())
                .unwrap_or_default(),
            cpu_count: cpus.len(),
            cpu_usage_percent: self.system.global_cpu_usage(),
            cpu_per_core: cpus.iter().map(|cpu| cpu.cpu_usage()).collect(),
            load_1: load.one,
            load_5: load.five,
            load_15: load.fifteen,
            uptime_seconds: System::uptime(),
            memory_total_bytes: self.system.total_memory(),
            memory_used_bytes: self.system.used_memory(),
            memory_available_bytes: self.system.available_memory(),
            swap_total_bytes: self.system.total_swap(),
            swap_used_bytes: self.system.used_swap(),
            process_count: self.system.processes().len(),
            processes,
            disks,
            networks,
            sampled_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_default(),
        }
    }

    /// Returns the most recent measurement; never blocks on counters.
    fn snapshot(&self) -> hakobi::host_agent::SystemMetrics {
        self.cached.clone()
    }

    /// Background loop: keeps the cached snapshot current on a fixed window.
    async fn run_refresh_loop(sampler: Arc<Mutex<Self>>) {
        loop {
            tokio::time::sleep(SAMPLE_INTERVAL).await;
            if let Ok(mut sampler) = sampler.try_lock() {
                sampler.cached = sampler.measure();
            }
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("hakobi_agent=info")),
        )
        .with_target(false)
        .init();
    if let Err(error) = run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let token = load_token()?;
    if token.len() < 24 {
        bail!("HAKOBI_AGENT_TOKEN must be at least 24 characters");
    }
    let address = env::var("HAKOBI_AGENT_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:8787".to_owned())
        .parse::<SocketAddr>()
        .context("invalid HAKOBI_AGENT_ADDR")?;
    let listener = TcpListener::bind(address)
        .await
        .with_context(|| format!("binding host agent on {address}"))?;
    tracing::info!(%address, "hakobi host agent listening");
    let state = AgentState {
        token: Arc::from(token),
        jobs: Arc::new(Mutex::new(HashMap::new())),
        system: {
            let sampler = Arc::new(Mutex::new(SystemSampler::new()));
            tokio::spawn(SystemSampler::run_refresh_loop(Arc::clone(&sampler)));
            sampler
        },
        build: agent_build::BuildState {
            runs_root: std::path::PathBuf::from(
                env::var("HAKOBI_AGENT_BUILDS").unwrap_or_else(|_| "/tmp/hakobi-builds".to_owned()),
            ),
            slots: Arc::new(tokio::sync::Semaphore::new(
                agent_build::MAX_CONCURRENT_BUILDS,
            )),
        },
    };
    axum::serve(listener, app(state))
        .await
        .context("host agent server")
}

/// Builds the agent router.
///
/// Every route except `/healthz` requires the bearer token. The Docker routes
/// are read and operate endpoints for the fixed host socket; none of them
/// proxies raw Docker API calls or accepts a user-supplied endpoint.
fn app(state: AgentState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/readiness", get(readiness))
        .route("/v1/docker/metrics", get(docker_metrics))
        .route("/v1/system/metrics", get(system_metrics))
        .route("/v1/docker/version", get(docker_version))
        .route("/v1/docker/containers", get(docker_containers))
        .route(
            "/v1/docker/containers/{name}/inspect",
            get(docker_container_inspect),
        )
        .route(
            "/v1/docker/containers/{name}/logs",
            get(docker_container_logs),
        )
        .route(
            "/v1/docker/containers/{name}/stats",
            get(docker_container_stats),
        )
        .route("/v1/docker/snapshot", get(docker_snapshot))
        .route("/v1/docker/containers/run", post(docker_run))
        .route("/build/dockerfile", post(agent_build::build_dockerfile))
        .route("/build/compose", post(agent_build::build_compose))
        .route("/build/nixpacks", post(agent_build::build_nixpacks))
        .route("/v1/docker/containers/{name}/start", post(docker_start))
        .route("/v1/docker/containers/{name}/stop", post(docker_stop))
        .route("/v1/docker/containers/{name}/restart", post(docker_restart))
        .route("/v1/docker/containers/{name}/delete", post(docker_delete))
        .route("/v1/install/status", get(install_status))
        .route("/v1/install/{tool}", post(install))
        .with_state(state)
}

fn load_token() -> Result<String> {
    if let Ok(token) = env::var("HAKOBI_AGENT_TOKEN") {
        return Ok(token);
    }
    let path = env::var("HAKOBI_AGENT_TOKEN_FILE")
        .unwrap_or_else(|_| "/etc/hakobi/agent.token".to_owned());
    let token = std::fs::read_to_string(&path)
        .with_context(|| format!("reading host agent token from {path}"))?;
    Ok(token.trim().to_owned())
}

fn authorized(headers: &HeaderMap, state: &AgentState) -> bool {
    let Some(value) = headers.get(AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    token.as_bytes().ct_eq(state.token.as_bytes()).into()
}

async fn health() -> impl IntoResponse {
    StatusCode::OK
}

async fn readiness(State(state): State<AgentState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    match tokio::task::spawn_blocking(hakobi::install::detect).await {
        Ok(readiness) => Json(readiness).into_response(),
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

async fn install(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(tool): Path<String>,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let Some(script) = installer(&tool) else {
        return error_response(StatusCode::NOT_FOUND, "unknown tool");
    };
    {
        let mut jobs = state.jobs.lock().await;
        if jobs.get(&tool).is_some_and(|job| job.running) {
            return error_response(StatusCode::CONFLICT, "install already running");
        }
        jobs.insert(
            tool.clone(),
            InstallJob {
                running: true,
                success: false,
                output: String::new(),
            },
        );
    }
    let state_for_job = state.clone();
    let tool_for_job = tool.clone();
    tokio::spawn(async move {
        let result = run_installer(script).await;
        let (success, output) = match result {
            Ok(output) => {
                tracing::info!(tool = %tool_for_job, output = %output, "host installer finished");
                (true, output)
            }
            Err(error) => {
                let output = error.to_string();
                tracing::error!(tool = %tool_for_job, error = %output, "host installer failed");
                (false, output)
            }
        };
        if let Some(job) = state_for_job.jobs.lock().await.get_mut(&tool_for_job) {
            job.running = false;
            job.success = success;
            job.output = truncate(&output);
        }
    });
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "status": "started", "tool": tool })),
    )
        .into_response()
}

async fn run_installer(script: &'static str) -> Result<String> {
    let mut child = Command::new("/bin/sh");
    child
        .arg("-c")
        .arg(script)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .kill_on_drop(true);
    let output = timeout(INSTALL_TIMEOUT, child.output())
        .await
        .context("host installer exceeded its 15 minute limit")?
        .context("running host installer")?;
    let stdout = truncate(&String::from_utf8_lossy(&output.stdout));
    let stderr = truncate(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        bail!(
            "installer exited with {}: {}{}",
            output.status,
            stdout,
            if stderr.is_empty() {
                String::new()
            } else {
                format!("\n{stderr}")
            }
        );
    }
    Ok(if stdout.is_empty() { stderr } else { stdout })
}

async fn system_metrics(State(state): State<AgentState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let metrics = state.system.lock().await.snapshot();
    Json(metrics).into_response()
}

async fn docker_metrics(State(state): State<AgentState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let mut child = Command::new("docker");
    child
        .args(["stats", "--no-stream", "--format", "{{json .}}"])
        .kill_on_drop(true);
    let output = match timeout(Duration::from_secs(20), child.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => return error_response(StatusCode::BAD_GATEWAY, error.to_string()),
        Err(_) => {
            return error_response(
                StatusCode::GATEWAY_TIMEOUT,
                "docker stats exceeded its 20 second limit",
            );
        }
    };
    if !output.status.success() {
        return error_response(
            StatusCode::BAD_GATEWAY,
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
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
    Json(metrics).into_response()
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

async fn docker_version(State(state): State<AgentState>, headers: HeaderMap) -> Response {
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

async fn docker_containers(State(state): State<AgentState>, headers: HeaderMap) -> Response {
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

async fn docker_snapshot(State(state): State<AgentState>, headers: HeaderMap) -> Response {
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
fn parse_tail(query: Option<&str>) -> std::result::Result<usize, Box<Response>> {
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

async fn docker_container_inspect(
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

async fn docker_container_logs(
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

async fn docker_container_stats(
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

async fn docker_start(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    container_action(state, headers, name, ContainerAction::Start).await
}

async fn docker_stop(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    container_action(state, headers, name, ContainerAction::Stop).await
}

async fn docker_restart(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    container_action(state, headers, name, ContainerAction::Restart).await
}

async fn docker_delete(
    State(state): State<AgentState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Response {
    container_action(state, headers, name, ContainerAction::Delete).await
}

async fn docker_run(
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

async fn install_status(State(state): State<AgentState>, headers: HeaderMap) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let jobs = state.jobs.lock().await;
    Json(jobs.clone()).into_response()
}

fn unauthorized() -> Response {
    error_response(StatusCode::UNAUTHORIZED, "unauthorized")
}

fn error_response(status: StatusCode, error: impl Into<String>) -> Response {
    (
        status,
        Json(AgentErrorBody {
            error: error.into(),
        }),
    )
        .into_response()
}

fn truncate(value: &str) -> String {
    truncate_to(value, MAX_REPORTED_OUTPUT)
}

/// Truncates `value` to at most `limit` bytes on a character boundary.
fn truncate_to(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}

fn installer(tool: &str) -> Option<&'static str> {
    match tool {
        "docker" => Some(
            r#"set -eu
curl -fsSL https://get.docker.com -o /tmp/hakobi-docker-install.sh
sh /tmp/hakobi-docker-install.sh
systemctl enable --now docker
rm -f /tmp/hakobi-docker-install.sh
"#,
        ),
        "compose" => Some(
            r#"set -eu
if command -v docker >/dev/null 2>&1 && docker compose version >/dev/null 2>&1; then
  exit 0
fi
if command -v apt-get >/dev/null 2>&1; then
  apt-get update
  DEBIAN_FRONTEND=noninteractive apt-get install -y docker-compose-plugin
elif command -v dnf >/dev/null 2>&1; then
  dnf install -y docker-compose-plugin
else
  echo "install the Docker Compose plugin for this distribution" >&2
  exit 1
fi
"#,
        ),
        "kubectl" => Some(
            r#"set -eu
case "$(uname -m)" in
  x86_64) arch=amd64 ;;
  aarch64|arm64) arch=arm64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
version="$(curl -fsSL --max-time 30 --retry 2 https://dl.k8s.io/release/stable.txt)"
base="https://dl.k8s.io/release/${version}/bin/linux/${arch}/kubectl"
curl -fsSL --max-time 300 --retry 2 --retry-delay 2 -o /tmp/hakobi-kubectl "$base"
curl -fsSL --max-time 30 --retry 2 -o /tmp/hakobi-kubectl.sha256 "$base.sha256"
printf '%s  %s\n' "$(cat /tmp/hakobi-kubectl.sha256)" /tmp/hakobi-kubectl | sha256sum -c -
install -m 0755 /tmp/hakobi-kubectl /usr/local/bin/kubectl
rm -f /tmp/hakobi-kubectl /tmp/hakobi-kubectl.sha256
kubectl version --client
"#,
        ),
        "nix" => Some(
            r#"set -eu
installer="$(mktemp)"
trap 'rm -f "$installer"' EXIT
curl -fsSL --max-time 300 --retry 2 --retry-delay 3 https://install.determinate.systems/nix -o "$installer"
sh "$installer" install --no-confirm
"#,
        ),
        "nixpacks" => Some(
            r#"set -eu
case "$(uname -m)" in
  x86_64) arch=x86_64 ;;
  aarch64|arm64) arch=aarch64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
version="${NIXPACKS_VERSION:-v1.41.0}"
archive="/tmp/hakobi-nixpacks.tar.gz"
work="$(mktemp -d)"
trap 'rm -rf "$work" "$archive"' EXIT
curl -fsSL --max-time 300 --retry 2 --retry-delay 3 -o "$archive" "https://github.com/railwayapp/nixpacks/releases/download/${version}/nixpacks-${version}-${arch}-unknown-linux-musl.tar.gz"
tar -xzf "$archive" -C "$work"
install -m 0755 "$work/nixpacks" /usr/local/bin/nixpacks
nixpacks --version
"#,
        ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{header::CONTENT_TYPE, Method, Request},
    };
    use tower::ServiceExt;

    use super::*;

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
}
