//! Authenticated build-terminal partial and its live SSE event stream.

use std::{collections::VecDeque, convert::Infallible, time::Duration};

use anyhow::Result;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use futures_util::stream;
use hakobi::catalog::{Deployment, DeploymentLog};
use serde_json::json;

use super::{authenticated, html_response, internal_error, page_csrf, with_catalog};
use crate::web::{templates, AppState};

const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(super) async fn logs_for_deployment(
    state: &AppState,
    deployment: Option<&Deployment>,
) -> Vec<DeploymentLog> {
    let Some(deployment) = deployment else {
        return Vec::new();
    };
    let deployment_id = deployment.id.clone();
    with_catalog(state, move |catalog| {
        catalog.list_deployment_logs(&deployment_id)
    })
    .await
    .unwrap_or_default()
}

pub(crate) async fn deployment_terminal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(deployment_id): Path<String>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    let deployment = match owned_deployment(&state, &deployment_id).await {
        Ok(Some(deployment)) => deployment,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let logs = match with_catalog(&state, move |catalog| {
        catalog.list_deployment_logs(&deployment_id)
    })
    .await
    {
        Ok(logs) => logs,
        Err(error) => return internal_error(error),
    };
    let (csrf_token, cookies) = page_csrf(&state, &headers);
    html_response(
        StatusCode::OK,
        templates::build_terminal(&deployment, &logs, &csrf_token, true),
        cookies,
    )
}

pub(crate) async fn deployment_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(deployment_id): Path<String>,
) -> Response {
    if !authenticated(&state, &headers).await {
        return Redirect::to("/login").into_response();
    }
    match owned_deployment(&state, &deployment_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    }
    let events = EventState::new(state, deployment_id);
    let body = Body::from_stream(stream::unfold(events, |mut events| async move {
        next_event(&mut events)
            .await
            .map(|event| (Ok::<_, Infallible>(event), events))
    }));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-accel-buffering", "no")
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn owned_deployment(state: &AppState, deployment_id: &str) -> Result<Option<Deployment>> {
    let deployment_id = deployment_id.to_owned();
    with_catalog(state, move |catalog| {
        let Some(deployment) = catalog.get_deployment(&deployment_id)? else {
            return Ok(None);
        };
        if catalog.get_app(&deployment.app_id)?.is_none() {
            return Ok(None);
        }
        Ok(Some(deployment))
    })
    .await
}

async fn event_snapshot(
    state: &AppState,
    deployment_id: &str,
    after_id: i64,
) -> Result<Option<(Deployment, Vec<DeploymentLog>)>> {
    let deployment_id = deployment_id.to_owned();
    with_catalog(state, move |catalog| {
        let Some(deployment) = catalog.get_deployment(&deployment_id)? else {
            return Ok(None);
        };
        if catalog.get_app(&deployment.app_id)?.is_none() {
            return Ok(None);
        }
        let logs = catalog.list_deployment_logs_after(&deployment_id, after_id)?;
        Ok(Some((deployment, logs)))
    })
    .await
}

struct EventState {
    state: AppState,
    deployment_id: String,
    after_id: i64,
    status: Option<(String, String, Option<String>)>,
    pending: VecDeque<String>,
    finished: bool,
}

impl EventState {
    fn new(state: AppState, deployment_id: String) -> Self {
        Self {
            state,
            deployment_id,
            after_id: 0,
            status: None,
            pending: VecDeque::new(),
            finished: false,
        }
    }
}

async fn next_event(events: &mut EventState) -> Option<String> {
    loop {
        if let Some(event) = events.pending.pop_front() {
            return Some(event);
        }
        if events.finished {
            return None;
        }
        let snapshot = event_snapshot(&events.state, &events.deployment_id, events.after_id).await;
        let (deployment, logs) = (match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(error = %error, "deployment event stream failed");
                return None;
            }
        })?;
        queue_snapshot(events, deployment, logs);
        if events.pending.is_empty() {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }
}

fn queue_snapshot(events: &mut EventState, deployment: Deployment, logs: Vec<DeploymentLog>) {
    let status = (
        deployment.status.clone(),
        deployment.started_at.clone(),
        deployment.completed_at.clone(),
    );
    if events.status.as_ref() != Some(&status) {
        events.pending.push_back(status_event(&deployment));
        events.status = Some(status);
    }
    for log in logs {
        events.after_id = log.id;
        events.pending.push_back(log_event(&log));
    }
    if is_terminal(&deployment.status) {
        events.finished = true;
    }
}

fn status_event(deployment: &Deployment) -> String {
    let data = json!({
        "status": deployment.status,
        "started_at": deployment.started_at,
        "completed_at": deployment.completed_at,
    });
    format!("event: status\ndata: {data}\n\n")
}

fn log_event(log: &DeploymentLog) -> String {
    let data = json!({
        "timestamp": log.timestamp,
        "level": log.level,
        "message": log.message,
    });
    format!("id: {}\nevent: log\ndata: {data}\n\n", log.id)
}

fn is_terminal(status: &str) -> bool {
    matches!(status, "running" | "failed" | "cancelled" | "rolled_back")
}
