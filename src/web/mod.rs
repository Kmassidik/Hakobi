pub mod auth;
mod handlers;
mod stacks_admin;
mod templates;
#[cfg(test)]
mod tests;

use std::{path::PathBuf, sync::Arc, time::Duration};

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
use hakobi::providers::{ContainerBackend, DockerBackend};

#[derive(Clone)]
pub(super) struct AppState {
    pub config: Arc<PublicConfig>,
    pub sessions: Arc<Sessions>,
    pub limiter: Arc<LoginLimiter>,
    pub password_work: Arc<Semaphore>,
    pub catalog_path: Arc<PathBuf>,
    pub backend: Arc<dyn ContainerBackend>,
}

pub fn router(config: PublicConfig, catalog_path: PathBuf) -> Router {
    router_with_backend(config, catalog_path, Arc::new(DockerBackend))
}

pub fn router_with_backend(
    config: PublicConfig,
    catalog_path: PathBuf,
    backend: Arc<dyn ContainerBackend>,
) -> Router {
    let secure = config.secure_cookies;
    let state = AppState {
        config: Arc::new(config),
        sessions: Arc::new(Sessions::default()),
        limiter: Arc::new(LoginLimiter::default()),
        password_work: Arc::new(Semaphore::new(2)),
        catalog_path: Arc::new(catalog_path),
        backend,
    };

    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/providers", get(handlers::providers).post(handlers::add_provider))
        .route("/providers/{id}/test", post(handlers::test_provider))
        .route("/providers/{id}/toggle", post(handlers::toggle_provider))
        .route("/providers/{id}/delete", post(handlers::delete_provider))
        .route("/containers", get(handlers::containers))
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
        .route("/stacks", get(handlers::stacks))
        .route(
            "/stacks/new",
            get(stacks_admin::stack_new).post(stacks_admin::stack_create),
        )
        .route(
            "/stacks/{id}/edit",
            get(stacks_admin::stack_edit).post(stacks_admin::stack_update),
        )
        .route(
            "/stacks/{id}/delete",
            post(stacks_admin::stack_delete),
        )
        .route("/stacks/runtime/{provider}/{name}", get(handlers::stack_detail))
        .route("/infrastructure", get(handlers::infrastructure))
        .route(
            "/login",
            get(handlers::login_page).post(handlers::login),
        )
        .route("/logout", post(handlers::logout))
        .route("/healthz", get(|| async { "OK" }))
        .route("/partials/status", get(handlers::status))
        .route("/assets/{*path}", get(handlers::asset))
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
                "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
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
        .layer(TraceLayer::new_for_http())
}
