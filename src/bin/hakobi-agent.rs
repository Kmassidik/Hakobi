//! Privileged host agent for Hakobi.
//!
//! This binary runs on the host, never inside the Hakobi dashboard container.
//! It exposes a tiny localhost-only HTTP API protected by a bearer token. Every
//! mutating endpoint maps to a fixed, embedded installer; request data is never
//! interpolated into a shell.

#![forbid(unsafe_code)]

use std::{collections::HashMap, env, net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{bail, Context, Result};
use axum::{
    extract::{Path, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, process::Command, sync::Mutex, time::timeout};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const MAX_REPORTED_OUTPUT: usize = 8 * 1024;

#[derive(Clone)]
struct AgentState {
    token: Arc<str>,
    jobs: Arc<Mutex<HashMap<String, InstallJob>>>,
}

#[derive(Clone, Serialize)]
struct InstallJob {
    running: bool,
    success: bool,
    output: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Default, Serialize)]
struct DockerMetrics {
    containers: usize,
    cpu_percent: f64,
    memory_used_bytes: u64,
    memory_limit_bytes: u64,
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
    };
    let app = Router::new()
        .route("/healthz", get(health))
        .route("/v1/readiness", get(readiness))
        .route("/v1/docker/metrics", get(docker_metrics))
        .route("/v1/install/status", get(install_status))
        .route("/v1/install/{tool}", post(install))
        .with_state(state);
    axum::serve(listener, app)
        .await
        .context("host agent server")
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
        Json(ErrorResponse {
            error: error.into(),
        }),
    )
        .into_response()
}

fn truncate(value: &str) -> String {
    if value.len() <= MAX_REPORTED_OUTPUT {
        return value.to_owned();
    }
    let mut end = MAX_REPORTED_OUTPUT;
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
