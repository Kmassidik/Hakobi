//! App runtime lifecycle actions and live container usage samples.

use axum::{
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use serde::Deserialize;

use super::{internal_error, post_is_allowed, redirect_with_notice, with_catalog, AppState};
use crate::web::deployment::container_name_for;
use hakobi::host_agent::DockerContainerStats;

#[derive(Debug, Deserialize)]
pub(crate) struct RuntimeForm {
    csrf_token: String,
}

#[derive(Clone, Copy)]
enum RuntimeAction {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
}

impl RuntimeAction {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "start" => Some(Self::Start),
            "stop" => Some(Self::Stop),
            "restart" => Some(Self::Restart),
            "pause" => Some(Self::Pause),
            "unpause" => Some(Self::Unpause),
            _ => None,
        }
    }

    fn state(self) -> &'static str {
        match self {
            Self::Stop => "stopped",
            Self::Pause => "paused",
            _ => "running",
        }
    }

    fn notice(self) -> &'static str {
        match self {
            Self::Start => "App started.",
            Self::Stop => "App stopped.",
            Self::Restart => "App restarted.",
            Self::Pause => "App paused.",
            Self::Unpause => "App resumed.",
        }
    }
}

pub(crate) async fn app_runtime_partial(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return super::partials::partial_unauthorized();
    }
    let app = match super::app_deploy::load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let (csrf_token, _) = super::page_csrf(&state, &headers);
    super::html_response(
        StatusCode::OK,
        super::templates::app_runtime_card(&app, &csrf_token),
        Vec::new(),
    )
}

pub(crate) async fn app_runtime_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((id, action)): Path<(String, String)>,
    Form(form): Form<RuntimeForm>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return axum::response::Redirect::to("/login").into_response();
    }
    if !post_is_allowed(&state, &headers, &form.csrf_token) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(action) = RuntimeAction::parse(&action) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let app = match super::app_deploy::load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let endpoint = format!("agent://{}", app.host_id);
    let app_id = app.id.clone();
    let processes = with_catalog(&state, move |catalog| catalog.list_app_processes(&app_id)).await;
    let processes = match processes {
        Ok(processes) => processes,
        Err(error) => return internal_error(error),
    };
    let name = container_name_for(&app.id);
    let mut names = vec![name];
    names.extend(
        processes
            .into_iter()
            .map(|process| hakobi::catalog::worker_container_name(&app.id, &process.name)),
    );
    for name in names {
        if let Err(error) = execute_action(&state, &endpoint, &name, action).await {
            return internal_error(error);
        }
    }
    let app_id = app.id.clone();
    let status = action.state();
    if let Err(error) = with_catalog(&state, move |catalog| {
        catalog.set_app_runtime_status(&app_id, Some(status))
    })
    .await
    {
        return internal_error(error);
    }
    redirect_with_notice(&format!("/apps/{id}"), action.notice())
}

async fn execute_action(
    state: &AppState,
    endpoint: &str,
    name: &str,
    action: RuntimeAction,
) -> anyhow::Result<()> {
    match action {
        RuntimeAction::Start => state.backend.start_container(endpoint, name).await,
        RuntimeAction::Stop => state.backend.stop_container(endpoint, name).await,
        RuntimeAction::Restart => state.backend.restart_container(endpoint, name).await,
        RuntimeAction::Pause => state.backend.pause_container(endpoint, name).await,
        RuntimeAction::Unpause => state.backend.unpause_container(endpoint, name).await,
    }
}

pub(crate) async fn app_usage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !super::authenticated(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let app = match super::app_deploy::load_app(&state, &id).await {
        Ok(Some(app)) => app,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => return internal_error(error),
    };
    let endpoint = format!("agent://{}", app.host_id);
    let name = container_name_for(&app.id);
    match state.backend.container_stats(&endpoint, &name).await {
        Ok(stats) => Json(DockerContainerStats::from(stats)).into_response(),
        Err(error) => internal_error(error),
    }
}
