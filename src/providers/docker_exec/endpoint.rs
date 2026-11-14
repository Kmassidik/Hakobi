use anyhow::{Context, Result};
use bollard::{Docker, API_DEFAULT_VERSION};

const CONNECT_TIMEOUT_SECS: u64 = 8;

/// Connects to the Docker daemon behind `endpoint`, which must be a
/// `unix:///absolute/path` socket or a `tcp://host:port` address.
pub(super) fn connect(endpoint: &str) -> Result<Docker> {
    let endpoint = endpoint.trim();
    crate::providers::docker::validate_endpoint(endpoint)?;
    if let Some(path) = endpoint.strip_prefix("unix://") {
        return Docker::connect_with_unix(path, CONNECT_TIMEOUT_SECS, API_DEFAULT_VERSION)
            .context("connect to Docker Unix socket");
    }
    let host = endpoint
        .strip_prefix("tcp://")
        .context("invalid Docker endpoint")?;
    Docker::connect_with_http(
        &format!("http://{host}"),
        CONNECT_TIMEOUT_SECS,
        API_DEFAULT_VERSION,
    )
    .context("connect to Docker TCP endpoint")
}
