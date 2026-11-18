//! HTTP build-runner endpoints for the host agent.
//!
//! `POST /build/{kind}` clones the app repository at the requested branch,
//! runs `docker build` with the requested tag, and reports the outcome. The
//! control plane's `BuildRunnerClient` is the only intended caller.

use std::{
    convert::Infallible,
    path::{Path, PathBuf},
    time::Instant,
};

use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

mod process;
mod runner;
#[cfg(test)]
mod tests;

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

#[derive(Debug, Serialize)]
pub struct BuildOutcome {
    pub status: String,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub log_tail: String,
    pub truncated: bool,
    pub detail: Option<String>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RunnerEvent<'a> {
    Log { stage: &'a str, message: &'a str },
    Complete { outcome: &'a BuildOutcome },
}

fn encode_log_event(stage: &str, message: &str) -> String {
    encode_event(&RunnerEvent::Log { stage, message })
}

fn encode_complete_event(outcome: &BuildOutcome) -> String {
    encode_event(&RunnerEvent::Complete { outcome })
}

fn encode_event(event: &RunnerEvent<'_>) -> String {
    let mut encoded = serde_json::to_string(event).expect("build event serialization cannot fail");
    encoded.push('\n');
    encoded
}

/// Sanitizes the repo URL into a short directory name.
fn workspace_dir(runs_root: &Path, git_url: &str, branch: &str) -> PathBuf {
    let digest = Sha256::digest(format!("{git_url}@{branch}").as_bytes());
    runs_root.join(format!("build-{}", hex::encode(&digest[..8])))
}

/// Runs one build job end to end: clone → docker build.
pub fn run_build(runs_root: &Path, job: &BuildJob) -> BuildOutcome {
    runner::run(runs_root, job, None)
}

fn run_build_stream(runs_root: &Path, job: &BuildJob, sender: &UnboundedSender<String>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::run(runs_root, job, Some(sender))
    }));
    let outcome = result.unwrap_or_else(|_| BuildOutcome {
        status: "failed".to_owned(),
        exit_code: 1,
        duration_ms: 0,
        log_tail: String::new(),
        truncated: false,
        detail: Some("build task panicked".to_owned()),
    });
    let _ = sender.send(encode_complete_event(&outcome));
}

/// Axum handler state for build routes.
#[derive(Clone)]
pub struct BuildState {
    pub runs_root: PathBuf,
    pub slots: std::sync::Arc<tokio::sync::Semaphore>,
}

pub async fn build_dockerfile(
    axum::extract::State(state): axum::extract::State<BuildState>,
    headers: HeaderMap,
    axum::Json(payload): axum::Json<BuildJob>,
) -> Response {
    handle_build(state, &headers, payload).await
}

pub async fn build_compose(
    axum::extract::State(state): axum::extract::State<BuildState>,
    headers: HeaderMap,
    axum::Json(payload): axum::Json<BuildJob>,
) -> Response {
    handle_build(state, &headers, payload).await
}

pub async fn build_nixpacks(
    axum::extract::State(state): axum::extract::State<BuildState>,
    headers: HeaderMap,
    axum::Json(payload): axum::Json<BuildJob>,
) -> Response {
    handle_build(state, &headers, payload).await
}

async fn handle_build(state: BuildState, headers: &HeaderMap, job: BuildJob) -> Response {
    // Bound concurrency so parallel pushes cannot thrash the host.
    let slot = match state.slots.clone().acquire_owned().await {
        Ok(slot) => slot,
        Err(_) => {
            return (StatusCode::SERVICE_UNAVAILABLE, "build queue saturated").into_response();
        }
    };
    if accepts_ndjson(headers) {
        return streaming_response(state.runs_root, job, slot);
    }
    json_response(state.runs_root, job, slot).await
}

async fn json_response(
    runs_root: PathBuf,
    job: BuildJob,
    slot: tokio::sync::OwnedSemaphorePermit,
) -> Response {
    let outcome = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        run_build(&runs_root, &job)
    })
    .await
    .unwrap_or_else(|error| failed_join_outcome(error.to_string()));
    axum::Json(outcome).into_response()
}

fn streaming_response(
    runs_root: PathBuf,
    job: BuildJob,
    slot: tokio::sync::OwnedSemaphorePermit,
) -> Response {
    let (sender, receiver) = unbounded_channel();
    tokio::task::spawn_blocking(move || {
        let _slot = slot;
        run_build_stream(&runs_root, &job, &sender);
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(receiver_stream(receiver)))
        .expect("valid NDJSON response headers")
}

fn receiver_stream(
    receiver: UnboundedReceiver<String>,
) -> impl futures_util::Stream<Item = Result<String, Infallible>> {
    futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (Ok(item), receiver))
    })
}

fn accepts_ndjson(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .any(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/x-ndjson"))
        })
}

fn failed_join_outcome(error: String) -> BuildOutcome {
    let started = Instant::now();
    BuildOutcome {
        status: "failed".to_owned(),
        exit_code: 1,
        duration_ms: started.elapsed().as_millis() as u64,
        log_tail: format!("build task failed: {error}"),
        truncated: false,
        detail: Some(format!("build task failed: {error}")),
    }
}
