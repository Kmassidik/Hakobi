//! Privileged host agent for Hakobi.
//!
//! This binary runs on the host, never inside the Hakobi dashboard container.
//! It exposes a tiny localhost-only HTTP API protected by a bearer token. Every
//! mutating endpoint maps to a fixed, embedded installer; request data is never
//! interpolated into a shell.

#![forbid(unsafe_code)]

#[path = "hakobi-agent/docker.rs"]
mod docker;
#[path = "hakobi-agent/install.rs"]
mod install;
#[path = "hakobi-agent/routes.rs"]
mod routes;
#[path = "hakobi-agent/sampler.rs"]
mod sampler;
#[cfg(test)]
#[path = "hakobi-agent/tests.rs"]
mod tests;

use std::{collections::HashMap, env, net::SocketAddr, sync::Arc};

use anyhow::{bail, Context, Result};
use axum::{
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::{agent_build, host_agent::AgentErrorBody, host_agent::InstallJob};
use subtle::ConstantTimeEq;
use tokio::{net::TcpListener, sync::Mutex};

use routes::app;
use sampler::SystemSampler;

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
