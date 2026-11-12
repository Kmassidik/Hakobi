//! HTTP build-runner endpoints for the host agent.
//!
//! `POST /build/{kind}` clones the app repository at the requested branch,
//! runs `docker build` with the requested tag, and reports the outcome. The
//! control plane's `BuildRunnerClient` is the only intended caller.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use anyhow::{bail, Result};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MAX_LOG_TAIL: usize = 4000;
/// Build directories live under the agent's runs dir; capped so a runaway
/// checkout cannot fill the disk.
pub const MAX_CONCURRENT_BUILDS: usize = 2;

#[derive(Debug, Deserialize)]
pub struct BuildJob {
    pub git_url: String,
    pub branch: String,
    pub image_tag: String,
    #[serde(default)]
    pub dockerfile_path: Option<String>,
    #[serde(default)]
    pub context_dir: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct BuildOutcome {
    pub status: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub log_tail: String,
    pub truncated: bool,
    pub detail: Option<String>,
}

fn truncate_tail(log: &str) -> (String, bool) {
    if log.len() <= MAX_LOG_TAIL {
        return (log.to_owned(), false);
    }
    let start = log.len() - MAX_LOG_TAIL;
    // Skip to the next newline so the tail starts on a whole line.
    let offset = log[start..].find('\n').map(|i| start + i + 1).unwrap_or(start);
    (log[offset..].to_owned(), true)
}

fn outcome(status: &str, exit_code: i32, started: Instant, log: &str, detail: Option<String>) -> BuildOutcome {
    let (log_tail, truncated) = truncate_tail(log);
    BuildOutcome {
        status: status.to_owned(),
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
        log_tail,
        truncated,
        detail,
    }
}

fn error_outcome(started: Instant, error: anyhow::Error) -> BuildOutcome {
    outcome("failed", 1, started, &format!("{error:#}"), Some(error.to_string()))
}

/// Sanitizes the repo URL into a short directory name.
fn workspace_dir(runs_root: &Path, git_url: &str, branch: &str) -> PathBuf {
    let digest = Sha256::digest(format!("{git_url}@{branch}").as_bytes());
    runs_root.join(format!("build-{}", hex::encode(&digest[..8])))
}

fn run_capture(cwd: &Path, program: &str, args: &[&str]) -> (i32, String) {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output();
    match output {
        Ok(output) => {
            let mut log = String::from_utf8_lossy(&output.stdout).to_string();
            log.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.code().unwrap_or(1), log)
        }
        Err(error) => (1, format!("{program} failed to start: {error}")),
    }
}

fn validate_inputs(job: &BuildJob) -> Result<()> {
    if !job.git_url.starts_with("https://") {
        bail!("git_url must be an https URL");
    }
    if job.branch.is_empty() || job.branch.len() > 200 {
        bail!("branch must be 1-200 characters");
    }
    if job.image_tag.is_empty() || job.image_tag.contains(char::is_whitespace) {
        bail!("image_tag must be a non-empty docker tag");
    }
    for path in [job.dockerfile_path.as_deref(), job.context_dir.as_deref()]
        .into_iter()
        .flatten()
    {
        if path.starts_with('/') || path.split('/').any(|part| part == "..") {
            bail!("repository paths must be relative");
        }
    }
    Ok(())
}

/// Runs one build job end to end: clone → docker build.
pub fn run_build(runs_root: &Path, job: &BuildJob) -> BuildOutcome {
    let started = Instant::now();
    if let Err(error) = validate_inputs(job) {
        return error_outcome(started, error);
    }
    let workdir = workspace_dir(runs_root, &job.git_url, &job.branch);

    // Reset any previous checkout so the clone is deterministic.
    let _ = std::fs::remove_dir_all(&workdir);
    if let Err(error) = std::fs::create_dir_all(&workdir) {
        return error_outcome(started, anyhow::anyhow!(error));
    }

    // Private repos: embed the token as a basic-auth user (git accepts
    // x-access-token:<token> for GitHub App installation tokens).
    let clone_url = job
        .token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| {
            job.git_url
                .replacen("https://", &format!("https://x-access-token:{token}@"), 1)
        })
        .unwrap_or_else(|| job.git_url.clone());
    let (code, log) = run_capture(&workdir, "git", &["clone", "--depth", "1", "--branch", &job.branch, &clone_url, "repo"]);
    let mut log = log;
    if code != 0 {
        return outcome("failed", code, started, &log, Some("git clone failed".to_owned()));
    }

    let repo = workdir.join("repo");
    let dockerfile = job.dockerfile_path.as_deref().unwrap_or("Dockerfile");
    let context = job.context_dir.as_deref().unwrap_or(".");
    let context_path = repo.join(context);
    let (code, build_log) = run_capture(
        &context_path,
        "docker",
        &["build", "-f", dockerfile, "-t", &job.image_tag, "."],
    );
    log.push_str(&build_log);
    if code != 0 {
        return outcome("failed", code, started, &log, Some("docker build failed".to_owned()));
    }
    outcome("succeeded", 0, started, &log, None)
}

/// Axum handler state for build routes.
#[derive(Clone)]
pub struct BuildState {
    pub runs_root: PathBuf,
    pub slots: std::sync::Arc<tokio::sync::Semaphore>,
}

pub async fn build_dockerfile(
    axum::extract::State(state): axum::extract::State<BuildState>,
    axum::Json(payload): axum::Json<BuildJob>,
) -> Response {
    handle_build(state, payload).await
}

pub async fn build_compose(
    axum::extract::State(state): axum::extract::State<BuildState>,
    axum::Json(payload): axum::Json<BuildJob>,
) -> Response {
    handle_build(state, payload).await
}

pub async fn build_nixpacks(
    axum::extract::State(state): axum::extract::State<BuildState>,
    axum::Json(payload): axum::Json<BuildJob>,
) -> Response {
    handle_build(state, payload).await
}

async fn handle_build(state: BuildState, job: BuildJob) -> Response {
    // Bound concurrency so parallel pushes cannot thrash the host.
    let _slot = match state.slots.acquire().await {
        Ok(slot) => slot,
        Err(_) => {
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "build queue saturated",
            )
                .into_response()
        }
    };
    let runs_root = state.runs_root.clone();
    // Blocking work: move off the async runtime.
    let outcome = tokio::task::spawn_blocking(move || run_build(&runs_root, &job))
        .await
        .unwrap_or_else(|error| {
            error_outcome(Instant::now(), anyhow::anyhow!("build task failed: {error}"))
        });
    axum::Json(outcome).into_response()
}
