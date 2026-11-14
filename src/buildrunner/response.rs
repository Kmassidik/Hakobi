use anyhow::{anyhow, Context, Result};
use reqwest::{Response, StatusCode};
use serde::{Deserialize, Deserializer};

const MAX_ERROR_BODY: usize = 8 * 1024;
pub(super) const ERROR_EXCERPT_BYTES: usize = 512;

/// The runner reports a null `exit_code` when the process never exited
/// (typically a timeout). The public contract uses `i32`, so map that to a
/// sentinel that cannot be a POSIX exit status.
pub(super) const NO_EXIT_CODE: i32 = -1;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BuildResponse {
    pub status: String,
    #[serde(
        default = "missing_exit_code",
        deserialize_with = "deserialize_exit_code"
    )]
    pub exit_code: i32,
    pub duration_ms: u64,
    pub log_tail: String,
    pub truncated: bool,
    #[serde(default)]
    pub detail: Option<String>,
}

pub(super) fn parse_build_response(body: &[u8]) -> Result<BuildResponse> {
    serde_json::from_slice(body).context("build runner returned an invalid JSON response")
}

fn missing_exit_code() -> i32 {
    NO_EXIT_CODE
}

fn deserialize_exit_code<'de, D>(deserializer: D) -> std::result::Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<i32>::deserialize(deserializer)?;
    Ok(value.unwrap_or(NO_EXIT_CODE))
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

pub(super) async fn http_error(response: Response) -> anyhow::Error {
    let status = response.status();
    let body = read_bounded(response, MAX_ERROR_BODY)
        .await
        .unwrap_or_default();
    anyhow!("{}", describe_http_error(status, &body))
}

async fn read_bounded(mut response: Response, limit: usize) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while body.len() < limit {
        let chunk = response
            .chunk()
            .await
            .context("could not read the build runner error response")?;
        let Some(chunk) = chunk else {
            break;
        };
        if chunk.is_empty() {
            break;
        }
        let take = (limit - body.len()).min(chunk.len());
        body.extend_from_slice(&chunk[..take]);
        if take < chunk.len() {
            break;
        }
    }
    Ok(body)
}

pub(super) fn describe_http_error(status: StatusCode, body: &[u8]) -> String {
    if let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(body) {
        let code = sanitize_excerpt(&envelope.error.code, 64);
        let message = sanitize_excerpt(&envelope.error.message, ERROR_EXCERPT_BYTES);
        if !message.is_empty() {
            return if code.is_empty() {
                format!("build runner returned HTTP {status}: {message}")
            } else {
                format!("build runner returned HTTP {status} ({code}): {message}")
            };
        }
    }

    let excerpt = sanitize_excerpt(&String::from_utf8_lossy(body), ERROR_EXCERPT_BYTES);
    if excerpt.is_empty() {
        format!("build runner returned HTTP {status}")
    } else {
        format!("build runner returned HTTP {status}: {excerpt}")
    }
}

fn sanitize_excerpt(value: &str, max_bytes: usize) -> String {
    let mut result = String::new();
    for character in value.chars() {
        if result.len() + character.len_utf8() > max_bytes {
            break;
        }
        if character.is_control() {
            result.push(' ');
        } else {
            result.push(character);
        }
    }
    result.trim().to_owned()
}
