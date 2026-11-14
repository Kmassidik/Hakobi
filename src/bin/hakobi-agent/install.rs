use std::time::Duration;

use anyhow::{bail, Context, Result};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::host_agent::InstallJob;
use tokio::{process::Command, time::timeout};

use super::{authorized, error_response, truncate_to, unauthorized, AgentState};

const INSTALL_TIMEOUT: Duration = Duration::from_secs(900);
const MAX_REPORTED_OUTPUT: usize = 8 * 1024;

pub(super) async fn install(
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
    spawn_install(state.clone(), tool.clone(), script);
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "status": "started", "tool": tool })),
    )
        .into_response()
}

fn spawn_install(state: AgentState, tool: String, script: &'static str) {
    tokio::spawn(async move {
        let result = run_installer(script).await;
        let (success, output) = match result {
            Ok(output) => {
                tracing::info!(tool = %tool, output = %output, "host installer finished");
                (true, output)
            }
            Err(error) => {
                let output = error.to_string();
                tracing::error!(tool = %tool, error = %output, "host installer failed");
                (false, output)
            }
        };
        if let Some(job) = state.jobs.lock().await.get_mut(&tool) {
            job.running = false;
            job.success = success;
            job.output = truncate(&output);
        }
    });
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

pub(super) async fn install_status(
    State(state): State<AgentState>,
    headers: HeaderMap,
) -> Response {
    if !authorized(&headers, &state) {
        return unauthorized();
    }
    let jobs = state.jobs.lock().await;
    Json(jobs.clone()).into_response()
}

fn truncate(value: &str) -> String {
    truncate_to(value, MAX_REPORTED_OUTPUT)
}

pub(super) fn installer(tool: &str) -> Option<&'static str> {
    match tool {
        "docker" => Some(DOCKER_INSTALLER),
        "compose" => Some(COMPOSE_INSTALLER),
        "kubectl" => Some(KUBECTL_INSTALLER),
        "nix" => Some(NIX_INSTALLER),
        "nixpacks" => Some(NIXPACKS_INSTALLER),
        _ => None,
    }
}

const DOCKER_INSTALLER: &str = r#"set -eu
curl -fsSL https://get.docker.com -o /tmp/hakobi-docker-install.sh
sh /tmp/hakobi-docker-install.sh
systemctl enable --now docker
rm -f /tmp/hakobi-docker-install.sh
"#;

const COMPOSE_INSTALLER: &str = r#"set -eu
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
"#;

const KUBECTL_INSTALLER: &str = r#"set -eu
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
"#;

const NIX_INSTALLER: &str = r#"set -eu
installer="$(mktemp)"
trap 'rm -f "$installer"' EXIT
curl -fsSL --max-time 300 --retry 2 --retry-delay 3 https://install.determinate.systems/nix -o "$installer"
sh "$installer" install --no-confirm
"#;

const NIXPACKS_INSTALLER: &str = r#"set -eu
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
"#;
