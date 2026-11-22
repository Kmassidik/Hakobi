//! Bounded, credential-free discovery in public Git repositories.

use std::time::Duration;

use anyhow::Result;
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use hakobi::{
    buildrunner::{validate_branch, validate_public_git_url},
    host_agent::{PublicSourceScanRequest, PublicSourceScanResponse},
};
use tokio::time::timeout;

use super::{authorized, error_response, unauthorized, AgentState};

#[path = "scanner/detect.rs"]
mod detect;
#[path = "scanner/git.rs"]
mod git;

const SCAN_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) async fn scan_public_source(
    State(state): State<AgentState>,
    headers: HeaderMap,
    payload: std::result::Result<Json<PublicSourceScanRequest>, JsonRejection>,
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
    if let Err(error) = validate_request(&request) {
        return error_response(StatusCode::BAD_REQUEST, error.to_string());
    }
    match timeout(SCAN_TIMEOUT, scan(request)).await {
        Ok(Ok(response)) => Json(response).into_response(),
        Ok(Err(error)) => error_response(StatusCode::BAD_GATEWAY, error.to_string()),
        Err(_) => error_response(
            StatusCode::GATEWAY_TIMEOUT,
            "public source scan exceeded 60 seconds",
        ),
    }
}

fn validate_request(request: &PublicSourceScanRequest) -> Result<()> {
    validate_public_git_url(&request.git_url)?;
    if let Some(branch) = request.branch.as_deref() {
        validate_branch(branch)?;
    }
    Ok(())
}

async fn scan(request: PublicSourceScanRequest) -> Result<PublicSourceScanResponse> {
    let workspace = tempfile::Builder::new()
        .prefix("hakobi-public-scan-")
        .tempdir()?;
    let refs = git::discover(&request.git_url, workspace.path()).await?;
    let selected = request
        .branch
        .as_deref()
        .unwrap_or(&refs.default_branch)
        .to_owned();
    let checkout = workspace.path().join("checkout");
    git::clone(&request.git_url, &selected, &checkout, workspace.path()).await?;
    let apps = detect::apps(&checkout)?;
    Ok(PublicSourceScanResponse {
        default_branch: refs.default_branch,
        branches: refs.branches,
        tags: refs.tags,
        apps,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    fn git(cwd: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(cwd)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    #[tokio::test]
    async fn local_git_discovery_and_shallow_clone_feed_detection() {
        let workspace = tempfile::tempdir().unwrap();
        let source = workspace.path().join("source");
        fs::create_dir(&source).unwrap();
        git(
            workspace.path(),
            &["init", "--quiet", "--initial-branch=main", "source"],
        );
        git(&source, &["config", "user.email", "scanner@example.test"]);
        git(&source, &["config", "user.name", "Scanner Test"]);
        fs::write(source.join("Dockerfile"), "FROM scratch\nEXPOSE 8080\n").unwrap();
        fs::write(source.join(".env.example"), "PUBLIC_KEY=\n").unwrap();
        git(&source, &["add", "Dockerfile", ".env.example"]);
        git(&source, &["commit", "--quiet", "-m", "initial"]);
        git(&source, &["tag", "v1.0.0"]);
        let url = source.to_str().unwrap();
        let refs = super::git::discover(url, workspace.path()).await.unwrap();
        assert_eq!(refs.default_branch, "main");
        assert_eq!(refs.branches, ["main"]);
        assert_eq!(refs.tags, ["v1.0.0"]);
        let checkout = workspace.path().join("checkout");
        super::git::clone(url, "main", &checkout, workspace.path())
            .await
            .unwrap();
        let apps = super::detect::apps(&checkout).unwrap();
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].container_port, Some(8080));
        assert!(apps[0].environment.contains_key("PUBLIC_KEY"));
    }
}
