//! Input validation shared by the Docker backends.
//!
//! Container names and image references are checked before they reach the
//! Docker API so path fragments or shell shapes never make it into a request.

use std::net::{IpAddr, Ipv4Addr};

use anyhow::{bail, Context, Result};
use bollard::{Docker, API_DEFAULT_VERSION};
use url::Url;

const CONNECT_TIMEOUT_SECS: u64 = 8;
const MAX_IMAGE_LEN: usize = 255;
const MAX_NAME_LEN: usize = 128;

/// Validates a Docker endpoint for direct connections.
///
/// Accepts only `unix:///absolute/socket` paths and `tcp://host:port`
/// addresses; the cloud metadata address is rejected outright.
pub fn validate_endpoint(endpoint: &str) -> Result<()> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() || endpoint.len() > 512 {
        bail!("provider endpoint must be 1–512 characters");
    }
    if endpoint.chars().any(char::is_control) {
        bail!("provider endpoint must not contain control characters");
    }
    if let Some(path) = endpoint.strip_prefix("unix://") {
        if !path.starts_with('/') || path.contains("..") || path == "/" {
            bail!("Unix provider endpoint must be an absolute socket path");
        }
        return Ok(());
    }
    if let Some(address) = endpoint.strip_prefix("tcp://") {
        let url = Url::parse(&format!("http://{address}/"))
            .context("Docker TCP endpoint must be host:port")?;
        let host = url.host_str().context("Docker TCP endpoint needs a host")?;
        if url.port().is_none() {
            bail!("Docker TCP endpoint must include a port");
        }
        if matches!(url.path(), "" | "/") && url.query().is_none() && url.username().is_empty() {
            if let Ok(address) = host.parse::<IpAddr>() {
                if address == IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)) {
                    bail!("metadata addresses are not allowed");
                }
            }
            return Ok(());
        }
    }
    bail!("use a unix:///path or tcp://host:port Docker endpoint")
}

/// Connects to the Docker daemon behind `endpoint`, which must be a
/// `unix:///absolute/path` socket or a `tcp://host:port` address.
pub(crate) fn connect(endpoint: &str) -> Result<Docker> {
    validate_endpoint(endpoint)?;
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

/// Validates a Docker container name before it reaches the Docker API.
///
/// Mirrors the rules applied by the execution layer: alphanumeric first
/// character, then letters, digits, `.`, `_` and `-` only.
/// Validates a Docker container name before it reaches the Docker API.
///
/// Mirrors the rules applied by the execution layer: alphanumeric first
/// character, then letters, digits, `.`, `_` and `-` only.
pub fn validate_container_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        bail!("container name must be 1–{MAX_NAME_LEN} characters");
    }
    match name.chars().next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => bail!("container name must start with a letter or digit"),
    }
    if !name
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        bail!("container name may only contain letters, digits, '.', '_' and '-'");
    }
    Ok(())
}

/// Validates an image reference that must already exist locally.
///
/// The reference is never pulled, so this only rejects shapes Docker would
/// refuse or that could smuggle arguments into a registry URL.
/// Validates an image reference that must already exist locally.
///
/// The reference is never pulled, so this only rejects shapes Docker would
/// refuse or that could smuggle arguments into a registry URL.
pub fn validate_image_reference(image: &str) -> Result<()> {
    if image.is_empty() || image.len() > MAX_IMAGE_LEN {
        bail!("image reference must be 1–{MAX_IMAGE_LEN} characters");
    }
    if image
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        bail!("image reference must not contain whitespace or control characters");
    }
    match image.chars().next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => bail!("image reference must start with a letter or digit"),
    }
    if !image.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/' | ':' | '@')
    }) {
        bail!("image reference contains unsupported characters");
    }
    if image.contains("..") || image.contains("//") || image.contains("@@") {
        bail!("image reference contains an invalid separator sequence");
    }
    Ok(())
}
