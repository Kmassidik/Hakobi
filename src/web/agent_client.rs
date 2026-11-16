use std::{collections::BTreeMap, time::Duration};

use hakobi::host_agent::{AgentEndpoint, AgentErrorBody, InstallJob};
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
        let response = agent_response(response).await?;
        Ok(response.json::<Readiness>().await?)
    }

    pub(super) async fn install(&self, tool: &str) -> anyhow::Result<()> {
        let response = self
            .client
            .post(format!("{}/v1/install/{tool}", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
            .timeout(Duration::from_secs(15))
            .send()
            .await?;
        agent_response(response).await?;
        Ok(())
    }

    pub(super) async fn install_status(&self) -> anyhow::Result<BTreeMap<String, InstallJob>> {
        let response = self
            .client
            .get(format!("{}/v1/install/status", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        let response = agent_response(response).await?;
        Ok(response.json::<BTreeMap<String, InstallJob>>().await?)
    }
}

async fn agent_response(response: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let message = response
        .json::<AgentErrorBody>()
        .await
        .map(|error| error.error)
        .unwrap_or_else(|_| format!("host agent returned {status}"));
    anyhow::bail!("{message}")
}
