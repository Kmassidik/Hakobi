use std::{collections::HashMap, time::Duration};

use hakobi::host_agent::{AgentEndpoint, AgentErrorBody, AgentHost, InstallJob, SystemMetrics};
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

    pub(super) fn from_host(host: AgentHost) -> Option<Self> {
        Self::from_endpoint(AgentEndpoint::from_host(host))
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

    pub(super) async fn install(&self, tool: &str) -> anyhow::Result<()> {
        let response = self
            .client
            .post(format!("{}/v1/install/{tool}", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
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
        Ok(())
    }

    pub(super) async fn system_metrics(&self) -> anyhow::Result<SystemMetrics> {
        let response = self
            .client
            .get(format!("{}/v1/system/metrics", self.endpoint.url()))
            .bearer_auth(self.endpoint.token())
            .timeout(Duration::from_secs(10))
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
        Ok(response.json::<SystemMetrics>().await?)
    }

    pub(super) async fn install_status(&self) -> anyhow::Result<HashMap<String, InstallJob>> {
        let response = self
            .client
            .get(format!("{}/v1/install/status", self.endpoint.url()))
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
        Ok(response.json::<HashMap<String, InstallJob>>().await?)
    }
}
