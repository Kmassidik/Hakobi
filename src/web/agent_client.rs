use std::{collections::HashMap, time::Duration};

use hakobi::install::Readiness;
use serde::Deserialize;

#[derive(Clone)]
pub(super) struct HostAgent {
    url: String,
    token: String,
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct AgentError {
    error: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct InstallJob {
    pub running: bool,
    pub success: bool,
    pub output: String,
}

impl HostAgent {
    pub(super) fn from_env() -> Option<Self> {
        let url = std::env::var("HAKOBI_AGENT_URL").ok()?;
        let token = std::env::var("HAKOBI_AGENT_TOKEN").ok()?;
        if url.trim().is_empty() || token.trim().len() < 24 {
            return None;
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(900))
            .build()
            .ok()?;
        Some(Self {
            url: url.trim_end_matches('/').to_owned(),
            token,
            client,
        })
    }

    pub(super) async fn readiness(&self) -> anyhow::Result<Readiness> {
        let response = self
            .client
            .get(format!("{}/v1/readiness", self.url))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let message = response
                .json::<AgentError>()
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
            .post(format!("{}/v1/install/{tool}", self.url))
            .bearer_auth(&self.token)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let message = response
                .json::<AgentError>()
                .await
                .map(|error| error.error)
                .unwrap_or_else(|_| format!("host agent returned {status}"));
            anyhow::bail!("{message}");
        }
        Ok(())
    }

    pub(super) async fn install_status(&self) -> anyhow::Result<HashMap<String, InstallJob>> {
        let response = self
            .client
            .get(format!("{}/v1/install/status", self.url))
            .bearer_auth(&self.token)
            .timeout(Duration::from_secs(5))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let message = response
                .json::<AgentError>()
                .await
                .map(|error| error.error)
                .unwrap_or_else(|_| format!("host agent returned {status}"));
            anyhow::bail!("{message}");
        }
        Ok(response.json::<HashMap<String, InstallJob>>().await?)
    }
}
