use axum::{
    http::{HeaderMap, StatusCode},
    response::Response,
};
use hakobi::catalog::ProviderConnection;

use super::{internal_error, redirect_with_notice, templates, truncate, AppState};

pub(super) async fn run_new_container(
    state: &AppState,
    headers: &HeaderMap,
    provider: &ProviderConnection,
    providers: &[ProviderConnection],
    values: &templates::ContainerFormValues,
    container_port: u16,
) -> Response {
    let result = state
        .backend
        .run_container(
            &provider.endpoint,
            &values.image,
            &values.name,
            Some(container_port),
            None,
            &std::collections::HashMap::new(),
            hakobi::providers::ResourceLimits::default(),
            None,
            &[],
        )
        .await;
    match result {
        Ok(run) => {
            let Some(host_port) = run.host_port else {
                return internal_error(anyhow::anyhow!(
                    "published container returned no host port"
                ));
            };
            let notice = format!(
                "Container {} is running on host port {}.",
                run.name, host_port
            );
            redirect_with_notice("/containers", &notice)
        }
        Err(error) => {
            tracing::warn!(error = %error, "container run failed");
            super::render_container_form(
                state,
                headers,
                providers,
                values,
                Some(&truncate(&error.to_string(), 500)),
                StatusCode::BAD_GATEWAY,
            )
        }
    }
}
