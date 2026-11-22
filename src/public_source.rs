//! Credential-free discovery of example environment keys in public Git hosts.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use url::Url;

use crate::{
    buildrunner::{validate_branch, validate_public_git_url},
    github::{example_file_path, parse_example_environment},
};

const MAX_EXAMPLE_BYTES: usize = 64 * 1024;

pub fn validate_app_path(path: &str) -> Result<()> {
    anyhow::ensure!(
        path.is_empty()
            || (!path.starts_with('/')
                && !path.ends_with('/')
                && path
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")),
        "invalid app path"
    );
    Ok(())
}

fn raw_url(repository: &str, branch: &str, path: &str, filename: &str) -> Result<Url> {
    validate_public_git_url(repository)?;
    validate_branch(branch)?;
    validate_app_path(path)?;
    let repository_url = Url::parse(repository)?;
    let host = repository_url
        .host_str()
        .context("repository host missing")?;
    let prefix = match host {
        "github.com" | "gitlab.com" | "codeberg.org" => host,
        _ => bail!("example discovery supports public GitHub, GitLab, and Codeberg repositories"),
    };
    let mut segments: Vec<&str> = repository_url
        .path_segments()
        .context("repository path missing")?
        .filter(|segment| !segment.is_empty())
        .collect();
    if let Some(last) = segments.last_mut() {
        *last = last.strip_suffix(".git").unwrap_or(last);
    }
    let mut url = match prefix {
        "github.com" => Url::parse("https://raw.githubusercontent.com")?,
        "gitlab.com" => Url::parse("https://gitlab.com")?,
        _ => Url::parse("https://codeberg.org")?,
    };
    {
        let mut output = url.path_segments_mut().expect("HTTP base URL");
        output.extend(segments);
        match prefix {
            "gitlab.com" => {
                output.extend(["-", "raw", branch]);
            }
            "codeberg.org" => {
                output.extend(["raw", "branch", branch]);
            }
            _ => {
                output.push(branch);
            }
        }
        let file_path = example_file_path(path, filename);
        output.extend(file_path.split('/'));
    }
    Ok(url)
}

pub fn validate_discovery_input(repository: &str, branch: &str, path: &str) -> Result<()> {
    let path = if path == "." { "" } else { path };
    raw_url(repository, branch, path, ".env.example").map(|_| ())
}

async fn fetch_optional(client: &reqwest::Client, url: Url) -> Result<Option<String>> {
    let response = client
        .get(url)
        .send()
        .await
        .context("example file request")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        bail!("public host returned {}", response.status());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk?);
        if bytes.len() > MAX_EXAMPLE_BYTES {
            bail!("example environment file exceeds 64 KiB");
        }
    }
    Ok(Some(
        String::from_utf8(bytes).context("example file is not UTF-8")?,
    ))
}

pub async fn fetch_example_environment(
    repository: &str,
    branch: &str,
    path: &str,
) -> Result<Option<BTreeMap<String, String>>> {
    let path = if path == "." { "" } else { path };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("hakobi-public-source-discovery")
        .build()?;
    for filename in [".env.example", ".env.sample"] {
        let url = raw_url(repository, branch, path, filename)?;
        if let Some(text) = fetch_optional(&client, url).await? {
            return Ok(Some(parse_example_environment(&text)));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_raw_urls_for_supported_public_hosts() {
        assert_eq!(
            raw_url(
                "https://github.com/decolua/9router.git",
                "master",
                "",
                ".env.example"
            )
            .unwrap()
            .as_str(),
            "https://raw.githubusercontent.com/decolua/9router/master/.env.example"
        );
        assert_eq!(
            raw_url(
                "https://gitlab.com/acme/app",
                "main",
                "apps/web",
                ".env.sample"
            )
            .unwrap()
            .as_str(),
            "https://gitlab.com/acme/app/-/raw/main/apps/web/.env.sample"
        );
        assert_eq!(
            raw_url("https://codeberg.org/acme/app", "main", "", ".env.example")
                .unwrap()
                .as_str(),
            "https://codeberg.org/acme/app/raw/branch/main/.env.example"
        );
    }

    #[test]
    fn rejects_untrusted_hosts_and_parent_paths() {
        assert!(raw_url("https://127.0.0.1/acme/app", "main", "", ".env.example").is_err());
        assert!(raw_url(
            "https://github.com/acme/app",
            "main",
            "../secret",
            ".env.example"
        )
        .is_err());
    }

    #[test]
    fn reported_nine_router_example_imports_fifteen_blank_keys() {
        let text = "JWT_SECRET=x\nINITIAL_PASSWORD=x\nDATA_DIR=x\nPORT=x\nNODE_ENV=x\nAPI_KEY_SECRET=x\nMACHINE_ID_SALT=x\nENABLE_REQUEST_LOGS=x\nOBSERVABILITY_ENABLED=x\nAUTH_COOKIE_SECURE=x\nREQUIRE_API_KEY=x\nBASE_URL=x\nCLOUD_URL=x\nNEXT_PUBLIC_BASE_URL=x\nNEXT_PUBLIC_CLOUD_URL=x\n# IGNORED=x\n";
        let environment = parse_example_environment(text);
        assert_eq!(environment.len(), 15);
        assert!(environment.contains_key("JWT_SECRET"));
        assert!(environment.contains_key("NEXT_PUBLIC_CLOUD_URL"));
        assert!(!environment.contains_key("IGNORED"));
        assert!(environment.values().all(String::is_empty));
    }
}
