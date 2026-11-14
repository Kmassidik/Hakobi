use super::MIN_TOKEN_LEN;

/// Connection settings for the host agent.
///
/// The token is never printed: [`std::fmt::Debug`] redacts it so request logs
/// and error reports stay free of credentials.
#[derive(Clone)]
pub struct AgentEndpoint {
    url: String,
    token: String,
}

impl AgentEndpoint {
    /// Builds validated settings, or `None` when the URL or token is unusable.
    pub fn new(url: impl Into<String>, token: impl Into<String>) -> Option<Self> {
        let url = url.into();
        let url = url.trim().trim_end_matches('/');
        let token = token.into();
        let token = token.trim();
        if token.len() < MIN_TOKEN_LEN || !is_agent_url(url) {
            return None;
        }
        Some(Self {
            url: url.to_owned(),
            token: token.to_owned(),
        })
    }

    /// Reads `HAKOBI_AGENT_URL` and `HAKOBI_AGENT_TOKEN` from the environment.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("HAKOBI_AGENT_URL").ok()?;
        let token = std::env::var("HAKOBI_AGENT_TOKEN").ok()?;
        Self::new(url, token)
    }

    /// Base URL of the agent, without a trailing slash.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Bearer token used for every agent request.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Builds an `AgentEndpoint` from an [`AgentHost`].
    pub fn from_host(host: crate::host_agent::AgentHost) -> Self {
        Self {
            url: host.url().to_owned(),
            token: host.token().to_owned(),
        }
    }
}

impl std::fmt::Debug for AgentEndpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentEndpoint")
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// A named agent connection resolved from the catalog.
///
/// Unlike [`AgentEndpoint`], this carries a per-host ID and is used for
/// `agent://{host_id}` provider resolution. The token is never printed:
/// [`std::fmt::Debug`] redacts it so request logs stay free of credentials.
#[derive(Clone)]
pub struct AgentHost {
    host_id: String,
    url: String,
    token: String,
}

impl AgentHost {
    /// Builds an agent host from raw parts.
    pub fn new(host_id: String, url: String, token: String) -> Option<Self> {
        if !is_agent_url(&url) {
            return None;
        }
        Some(Self {
            host_id,
            url: url.trim_end_matches('/').to_owned(),
            token,
        })
    }

    /// Per-host ID used as the `agent://{host_id}` endpoint.
    pub fn host_id(&self) -> &str {
        &self.host_id
    }

    /// Base URL of the agent, without a trailing slash.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Bearer token used for every agent request.
    pub fn token(&self) -> &str {
        &self.token
    }
}

impl std::fmt::Debug for AgentHost {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentHost")
            .field("host_id", &self.host_id)
            .field("url", &self.url)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Accepts only plain loopback-style HTTP(S) base URLs.
fn is_agent_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return false;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return false;
    }
    matches!(parsed.path(), "" | "/")
}
