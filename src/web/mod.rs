mod agent_client;
pub mod auth;
mod deployment;
mod handlers;
mod templates;
#[cfg(test)]
mod tests {
    // Contents of tests.rs are included verbatim; the file itself opens and
    // closes the module braces.
    include!("tests.rs");
}
#[cfg(test)]
mod tests_deployment;
#[cfg(test)]
mod tests_metrics;
#[cfg(test)]
mod tests_pipeline_support;
#[cfg(test)]
mod tests_webhook;
mod webhook;

use hakobi::buildrunner::BuildRunnerClient;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    http::{
        header::{CONTENT_SECURITY_POLICY, REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS},
        HeaderValue, StatusCode,
    },
    routing::{get, post},
    Router,
};
use http::HeaderName;
use tokio::sync::Semaphore;
use tower_http::{
    catch_panic::CatchPanicLayer, limit::RequestBodyLimitLayer, set_header::SetResponseHeaderLayer,
    timeout::TimeoutLayer, trace::TraceLayer,
};

use self::auth::{LoginLimiter, Sessions};
use crate::config::PublicConfig;
use hakobi::{
    catalog::Catalog,
    providers::{ContainerBackend, RoutingBackend},
};

#[derive(Clone)]
pub(super) struct AppState {
    pub config: Arc<PublicConfig>,
    pub sessions: Arc<Sessions>,
    pub limiter: Arc<LoginLimiter>,
    pub password_work: Arc<Semaphore>,
    pub catalog_path: Arc<PathBuf>,
    pub backend: Arc<dyn ContainerBackend>,
    pub build_runner: Option<BuildRunnerClient>,
    pub deployments: deployment::DeploymentGuard,
    /// Shared 1.5s cache of latest live samples so concurrent dashboard
    /// components share one metrics-db read and fall back to the last-good
    /// reading instead of rendering zeros when the database is empty.
    pub live_cache: Arc<std::sync::Mutex<LiveSampleCache>>,
}

/// Last-good live samples with a short shared TTL.
#[derive(Default)]
pub(super) struct LiveSampleCache {
    pub fetched_at: Option<std::time::Instant>,
    pub samples: Vec<hakobi::metrics::LatestLive>,
}

impl LiveSampleCache {
    const TTL: std::time::Duration = std::time::Duration::from_millis(1500);

    /// Returns cached samples when fresh; otherwise reloads from the metrics
    /// database and keeps the previous reading as last-good fallback.
    fn get_or_load(&mut self, catalog_path: &Path) -> Vec<hakobi::metrics::LatestLive> {
        if let Some(fetched_at) = self.fetched_at {
            if fetched_at.elapsed() < Self::TTL && !self.samples.is_empty() {
                return self.samples.clone();
            }
        }
        let metrics_path = hakobi::metrics::metrics_database_path(catalog_path);
        let fresh = hakobi::metrics::MetricsDb::open(metrics_path)
            .and_then(|db| db.latest_live_samples())
            .unwrap_or_default();
        if !fresh.is_empty() {
            self.samples = fresh.clone();
            self.fetched_at = Some(std::time::Instant::now());
        }
        // Empty fresh read keeps the last-good samples (stale but honest);
        // only starts empty when nothing was ever sampled.
        self.samples.clone()
    }
}

/// Builds the production router.
///
/// Providers with an `agent://` endpoint are served by the host agent when
/// `HAKOBI_AGENT_URL` and `HAKOBI_AGENT_TOKEN` are configured; every other
/// endpoint keeps using the direct Docker backend.
pub fn router(config: PublicConfig, catalog_path: PathBuf) -> Router {
    let backend = match Catalog::open(&catalog_path) {
        Ok(catalog) => Arc::new(RoutingBackend::from_env_with_catalog(Arc::new(Mutex::new(
            catalog,
        )))),
        Err(error) => {
            tracing::error!(error = %error, "catalog unavailable for host-agent routing");
            Arc::new(RoutingBackend::from_env())
        }
    };
    router_with_backend(config, catalog_path, backend)
}

pub fn router_with_backend(
    config: PublicConfig,
    catalog_path: PathBuf,
    backend: Arc<dyn ContainerBackend>,
) -> Router {
    let build_runner = match BuildRunnerClient::from_env() {
        Ok(client) => client,
        Err(error) => {
            tracing::warn!(error = %error, "build runner disabled: invalid configuration");
            None
        }
    };
    router_with_pipeline(config, catalog_path, backend, build_runner)
        .expect("session secret load cannot fail for a writable data dir")
}

/// Builds the router with an explicit build runner client; tests inject a
/// scripted runner here instead of relying on process environment variables.
pub(crate) fn router_with_pipeline(
    config: PublicConfig,
    catalog_path: PathBuf,
    backend: Arc<dyn ContainerBackend>,
    build_runner: Option<BuildRunnerClient>,
) -> anyhow::Result<Router> {
    let secure = config.secure_cookies;
    let state = AppState {
        config: Arc::new(config),
        sessions: Arc::new(Sessions::load(
            catalog_path.parent().unwrap_or(Path::new(".")),
        )?),
        limiter: Arc::new(LoginLimiter::default()),
        password_work: Arc::new(Semaphore::new(2)),
        catalog_path: Arc::new(catalog_path),
        backend,
        build_runner,
        deployments: deployment::DeploymentGuard::new(),
        live_cache: Arc::new(std::sync::Mutex::new(LiveSampleCache::default())),
    };

    Ok(Router::new()
        .route("/", get(handlers::dashboard_redirect))
        .route("/dashboard", get(handlers::dashboard))
        .route("/containers", get(handlers::containers))
        .route(
            "/containers/{provider}/{name}",
            get(handlers::container_detail),
        )
        .route(
            "/containers/{provider}/{name}/stats",
            get(handlers::container_detail_stats_partial),
        )
        .route(
            "/containers/{provider}/{name}/logs",
            get(handlers::container_detail_logs_partial),
        )
        .route("/partials/metrics", get(handlers::metrics_partial))
        .route("/partials/charts", get(handlers::charts_partial))
        .route("/partials/readiness", get(handlers::readiness_partial))
        .route(
            "/containers/new",
            get(handlers::container_new).post(handlers::container_create),
        )
        .route(
            "/containers/{provider}/{name}/start",
            post(handlers::container_start),
        )
        .route(
            "/containers/{provider}/{name}/stop",
            post(handlers::container_stop),
        )
        .route(
            "/containers/{provider}/{name}/restart",
            post(handlers::container_restart),
        )
        .route(
            "/containers/{provider}/{name}/delete",
            post(handlers::container_delete),
        )
        .route("/apps", get(handlers::app_list).post(handlers::app_create))
        .route("/apps/new", get(handlers::app_new))
        .route("/apps/{id}", get(handlers::app_detail))
        .route(
            "/apps/{id}/edit",
            get(handlers::app_edit_page).post(handlers::app_edit_submit),
        )
        .route("/apps/{id}/delete", post(handlers::app_delete))
        .route("/apps/{id}/deploy", post(handlers::app_deploy))
        .route("/apps/{id}/rollback", post(handlers::app_rollback))
        .route("/apps/{id}/deployments", get(handlers::app_deployments))
        .route("/apps/{id}/logs", get(handlers::app_logs))
        .route("/apps/{id}/status", get(handlers::app_status_partial))
        .route("/webhook/{app_id}/{secret}", post(webhook::webhook_receive))
        .route("/webhooks/github", post(handlers::github_webhook))
        .route("/webhooks/github/callback", get(handlers::github_manifest_callback))
        .route("/api/github/status", get(handlers::github_status))
        .route("/api/github/repos", get(handlers::github_repos))
        .route("/api/github/branches", get(handlers::github_branches_api))
        .route("/api/github/detect", get(handlers::github_detect))
        .route("/settings", get(handlers::settings_page))
        .route("/settings/github", get(handlers::github_settings_page))
        .route("/settings/github/connect", post(handlers::github_connect_start))
        .route("/settings/github/recheck", post(handlers::github_recheck_installation))
        .route("/settings/github/install", post(handlers::github_install_start))
        .route("/settings/github/manifest-callback", get(handlers::github_manifest_post_install))
        .route("/settings/github/callback", get(handlers::github_connect_callback))
        .route("/settings/github/delete", post(handlers::github_disconnect))
        .route("/login",
            get(handlers::login_page).post(handlers::login),
        )
        .route("/logout", post(handlers::logout))
        .route("/healthz", get(|| async { "OK" }))
        .route("/partials/status", get(handlers::status))
        .route("/assets/{*path}", get(handlers::asset))
        .route("/favicon.ico", get(handlers::asset_favicon))
        .fallback(|| async { StatusCode::NOT_FOUND })
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(16 * 1024))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(20),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'self'; script-src 'self' https://static.cloudflareinsights.com 'sha256-6K7TdWRsrATvM1Ny7ztRvNVS5Gpo1MkMNVxEwPPEzog='; style-src 'self' 'sha256-faU7yAF8NxuMTNEwVmBz+VcYeIoBQ2EMHW3WaVxCvnk='; img-src 'self' data:; connect-src 'self' https://cloudflareinsights.com; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self' https://github.com; frame-ancestors 'none'",
            ),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("cross-origin-opener-policy"),
            HeaderValue::from_static("same-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("cross-origin-resource-policy"),
            HeaderValue::from_static("same-origin"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("strict-transport-security"),
            if secure {
                HeaderValue::from_static("max-age=31536000; includeSubDomains")
            } else {
                HeaderValue::from_static("max-age=0")
            },
        ))
        .layer(CatchPanicLayer::new())
        .layer(TraceLayer::new_for_http()))
}
