//! Repository picker snapshots: shared memory, durable last-good data, and
//! single-flight background refreshes.

use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use hakobi::{catalog::Catalog, github::GithubClient, github::RepoSummary};
use serde::{Deserialize, Serialize};

use super::{super::AppState, authenticated, github::client_for};

const CACHE_TTL: Duration = Duration::from_secs(300);
const SNAPSHOT_PREFIX: &str = "github.repositories.";

/// GET /api/github/status — installation, identity, and live API health.
pub(crate) async fn status(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !authenticated(&state, &headers).await {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(catalog) = Catalog::open((*state.catalog_path).clone()) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let app_installed = catalog.github_app_installed().unwrap_or(false);
    let user = catalog
        .load_github_connection(&state.config.username)
        .ok()
        .flatten();
    let api_available = if app_installed && user.is_some() {
        match client_for(&state, &headers).await {
            Some(client) => client.check_api().await.is_ok(),
            None => false,
        }
    } else {
        false
    };
    Json(serde_json::json!({
        "app_installed": app_installed,
        "user_connected": user.is_some(),
        "github_login": user.map(|u| u.github_login),
        "api_available": api_available,
    }))
    .into_response()
}

#[derive(Serialize, Deserialize)]
struct Snapshot {
    fetched_at: u64,
    repos: Vec<RepoSummary>,
}

/// Returns fresh repositories immediately; stale snapshots refresh in background.
pub(crate) async fn repos(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let Some(client) = client_for(&state, &headers).await else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if let Some(json) = state.github_repo_cache.get(CACHE_TTL) {
        if let Ok(repos) = serde_json::from_str::<Vec<RepoSummary>>(&json) {
            return repository_response(repos, false);
        }
    }
    let key = format!("{SNAPSHOT_PREFIX}{}", client.installation_id());
    let saved = load_snapshot(&state.catalog_path, &key);
    if let Some(snapshot) = saved
        .as_ref()
        .filter(|snapshot| snapshot_is_fresh(snapshot))
    {
        cache_snapshot(&state, snapshot);
        return repository_response(snapshot.repos.clone(), false);
    }
    if let Some(snapshot) = saved {
        refresh_in_background(state, client, key);
        return repository_response(snapshot.repos, true);
    }
    match client.list_repos().await {
        Ok(repos) => {
            store_snapshot(&state, &key, &repos);
            repository_response(repos, false)
        }
        Err(error) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

fn repository_response(repos: Vec<RepoSummary>, stale: bool) -> Response {
    Json(serde_json::json!({ "repos": repos, "stale": stale })).into_response()
}

fn snapshot_is_fresh(snapshot: &Snapshot) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(snapshot.fetched_at) < CACHE_TTL.as_secs()
}

fn load_snapshot(catalog_path: &Path, key: &str) -> Option<Snapshot> {
    Catalog::open(catalog_path)
        .ok()?
        .settings_get(key)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
}

fn cache_snapshot(state: &AppState, snapshot: &Snapshot) {
    if let Ok(json) = serde_json::to_string(&snapshot.repos) {
        state.github_repo_cache.set(json);
    }
}

fn store_snapshot(state: &AppState, key: &str, repos: &[RepoSummary]) {
    let snapshot = Snapshot {
        fetched_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        repos: repos.to_vec(),
    };
    cache_snapshot(state, &snapshot);
    let Ok(json) = serde_json::to_string(&snapshot) else {
        return;
    };
    let result = Catalog::open(state.catalog_path.to_path_buf())
        .and_then(|catalog| catalog.settings_set(key, &json));
    if let Err(error) = result {
        tracing::warn!("could not persist GitHub repository snapshot: {error}");
    }
}

fn refresh_in_background(state: AppState, client: GithubClient, key: String) {
    if !state.github_repo_cache.begin_refresh() {
        return;
    }
    tokio::spawn(async move {
        match client.list_repos().await {
            Ok(repos) => store_snapshot(&state, &key, &repos),
            Err(error) => tracing::warn!("GitHub repository refresh failed: {error}"),
        }
        state.github_repo_cache.finish_refresh();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_snapshot_survives_catalog_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.db");
        let snapshot = Snapshot {
            fetched_at: 123,
            repos: vec![RepoSummary {
                full_name: "owner/repo".into(),
                default_branch: "main".into(),
                private: true,
            }],
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        Catalog::open(path.clone())
            .unwrap()
            .settings_set("github.repositories.42", &json)
            .unwrap();

        let restored = load_snapshot(&path, "github.repositories.42").unwrap();
        assert_eq!(restored.repos[0].full_name, "owner/repo");
        assert!(restored.repos[0].private);
    }

    #[test]
    fn stale_refresh_has_one_flight() {
        let cache = hakobi::github::RepositoryCache::new();
        assert!(cache.begin_refresh());
        assert!(!cache.begin_refresh());
        cache.finish_refresh();
        assert!(cache.begin_refresh());
    }
}
