use anyhow::Context as _;
use hakobi::install::Readiness;

/// Loads tool readiness from the host agent (falls back to local detect).
pub(super) async fn load_readiness() -> anyhow::Result<Readiness> {
    let agent = crate::web::agent_client::HostAgent::from_env();
    if let Some(agent) = agent {
        if let Ok(readiness) = agent.readiness().await {
            return Ok(readiness);
        }
    }
    tokio::task::spawn_blocking(hakobi::install::detect)
        .await
        .context("host readiness probe")
}
