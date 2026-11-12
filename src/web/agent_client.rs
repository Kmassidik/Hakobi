use std::time::Duration;

use hakobi::host_agent::{AgentEndpoint, AgentErrorBody};
use hakobi::install::Readiness;

#[derive(Clone)]
pub(super) struct HostAgent {
    endpoint: AgentEndpoint,
    client: reqwest::Client,
}

impl HostAgent {
    pub(super) fn from_env() -> Option<Self> {
        let endpoint = AgentEndpoint::from_env()?;
        Self::from_endpoint(endpoint)
    }

    fn from_endpoint(endpoint: AgentEndpoint) -> Option<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(900))
            .build()
            .ok()?;
        Some(Self { endpoint, client })
    }

    pub(super) async fn readiness(&self) -> anyhow::Result<Readiness> {
        let response = self
            .client
            .get(format!("{}/v1/readiness", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let message = response
                .json::<AgentErrorBody>()
                .await
                .map(|error| error.error)
                .unwrap_or_else(|_| format!("host agent returned {status}"));
            anyhow::bail!("{message}");
        }
        Ok(response.json::<Readiness>().await?)
    }
}
