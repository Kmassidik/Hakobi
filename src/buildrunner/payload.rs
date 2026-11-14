use anyhow::{bail, Context, Result};
use serde::Serialize;
use url::Url;

use super::client::BuildRequest;

const MAX_GIT_URL_LEN: usize = 2_048;
pub(super) const MAX_BRANCH_LEN: usize = 255;
const MAX_IMAGE_TAG_LEN: usize = 255;
const MAX_PATH_LEN: usize = 1_024;
const MAX_TOKEN_LEN: usize = 8_192;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BuildKind {
    Compose,
    Dockerfile,
    Nixpacks,
}

impl BuildKind {
    pub(super) fn endpoint(self) -> &'static str {
        match self {
            Self::Compose => "/build/compose",
            Self::Dockerfile => "/build/dockerfile",
            Self::Nixpacks => "/build/nixpacks",
        }
    }
}

#[derive(Serialize)]
pub(super) struct BuildPayload<'a> {
    git_url: &'a str,
    branch: &'a str,
    image_tag: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dockerfile_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_dir: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<&'a str>,
}

impl<'a> BuildPayload<'a> {
    pub(super) fn for_request(kind: BuildKind, request: &'a BuildRequest) -> Result<Self> {
        validate_request_common(request)?;
        let (dockerfile_path, context_dir) = match kind {
            BuildKind::Compose | BuildKind::Dockerfile => {
                if kind == BuildKind::Compose && request.dockerfile_path.is_some() {
                    bail!("compose builds do not accept dockerfile_path");
                }
                if let Some(path) = request.dockerfile_path.as_deref() {
                    validate_repo_path("dockerfile_path", path, false)?;
                }
                if let Some(directory) = request.context_dir.as_deref() {
                    validate_repo_path("context_dir", directory, true)?;
                }
                (
                    request.dockerfile_path.as_deref(),
                    request.context_dir.as_deref(),
                )
            }
            BuildKind::Nixpacks => {
                if request.dockerfile_path.is_some() || request.context_dir.is_some() {
                    bail!("nixpacks builds do not accept dockerfile_path or context_dir");
                }
                (None, None)
            }
        };
        Ok(Self {
            git_url: &request.git_url,
            branch: &request.branch,
            image_tag: &request.image_tag,
            dockerfile_path,
            context_dir,
            token: request.token.as_deref(),
        })
    }
}

pub(super) fn validate_base_url(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        bail!("build runner URL must not be empty");
    }
    if value.chars().any(char::is_control) {
        bail!("build runner URL must not contain control characters");
    }
    let url = Url::parse(value).context("build runner URL is not a valid URL")?;
    match url.scheme() {
        "http" | "https" => {}
        scheme => bail!("build runner URL must use http or https, not {scheme}"),
    }
    if url.host_str().is_none() {
        bail!("build runner URL must include a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("build runner URL must not contain credentials");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("build runner URL must not contain a query or fragment");
    }
    Ok(value.trim_end_matches('/').to_owned())
}

pub(super) fn validate_opaque_token(field: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{field} must not be empty when provided");
    }
    if value.len() > MAX_TOKEN_LEN {
        bail!("{field} must be at most {MAX_TOKEN_LEN} bytes");
    }
    if !value.is_ascii() {
        bail!("{field} must be ASCII");
    }
    if value
        .chars()
        .any(|character| character.is_control() || character.is_whitespace())
    {
        bail!("{field} must not contain whitespace or control characters");
    }
    Ok(())
}

fn validate_request_common(request: &BuildRequest) -> Result<()> {
    validate_git_url(&request.git_url)?;
    validate_branch(&request.branch)?;
    validate_image_tag(&request.image_tag)?;
    if let Some(token) = request.token.as_deref() {
        validate_opaque_token("token", token)?;
        if !is_https_url(&request.git_url) {
            bail!("token is only supported for https git URLs");
        }
    }
    Ok(())
}

fn validate_field(field: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() {
        bail!("{field} must not be empty");
    }
    if value.len() > max {
        bail!("{field} must be at most {max} bytes");
    }
    if value.chars().any(char::is_control) {
        bail!("{field} must not contain control characters");
    }
    Ok(())
}

fn validate_git_url(value: &str) -> Result<()> {
    validate_field("git_url", value, MAX_GIT_URL_LEN)?;
    if value.chars().any(char::is_whitespace) {
        bail!("git_url must not contain whitespace");
    }
    if value.starts_with('-') {
        bail!("git_url must not start with '-'");
    }
    // Check the raw value: `Url::parse` silently collapses `..` segments.
    if value.split('/').any(|segment| segment == "..") {
        bail!("git_url must not contain '..' path segments");
    }

    if let Some((scheme, _)) = value.split_once("://") {
        return validate_url_git(value, scheme);
    }
    validate_scp_git(value)
}

fn validate_url_git(value: &str, scheme: &str) -> Result<()> {
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("ssh") {
        bail!("git_url must use https:// or ssh://");
    }
    let url = Url::parse(value).context("git_url is not a valid URL")?;
    if url.host_str().is_none() {
        bail!("git_url must include a host");
    }
    if url.password().is_some() {
        bail!("git_url must not embed a password or token");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("git_url must not contain a query or fragment");
    }
    if url.username().starts_with('-') {
        bail!("git_url has an invalid user name");
    }
    if url.path().is_empty() || url.path() == "/" {
        bail!("git_url must include a repository path");
    }
    Ok(())
}

fn validate_scp_git(value: &str) -> Result<()> {
    // `user@host:path` (scp-like) is the other transport the runner accepts.
    let remote_error = "git_url must be an https, ssh://, or user@host:path remote";
    let (authority, path) = value.split_once(':').context(remote_error)?;
    let (user, host) = authority.split_once('@').context(remote_error)?;
    if user.is_empty() || host.is_empty() || path.is_empty() {
        bail!(remote_error);
    }
    if !user
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
    {
        bail!("git_url has an invalid user name");
    }
    if !host
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-'))
        || host.starts_with('-')
        || host.starts_with('.')
        || host.ends_with('.')
        || host.ends_with('-')
    {
        bail!("git_url has an invalid host");
    }
    if path.split('/').any(|segment| segment == "..") {
        bail!("git_url must not contain '..' path segments");
    }
    if path.contains('?') || path.contains('#') {
        bail!("git_url must not contain a query or fragment");
    }
    Ok(())
}

fn validate_branch(value: &str) -> Result<()> {
    validate_field("branch", value, MAX_BRANCH_LEN)?;
    if value.chars().any(char::is_whitespace) {
        bail!("branch must not contain whitespace");
    }
    if value.starts_with('-') {
        bail!("branch must not start with '-'");
    }
    if value == "@" {
        bail!("branch must not be '@'");
    }
    if value.starts_with('/') || value.ends_with('/') || value.contains("//") {
        bail!("branch must not start or end with '/', or contain '//'");
    }
    if value.starts_with('.') || value.ends_with('.') {
        bail!("branch must not start or end with '.'");
    }
    if value.contains("..") {
        bail!("branch must not contain '..'");
    }
    if value.contains("@{") {
        bail!("branch must not contain '@{{'");
    }
    if value
        .chars()
        .any(|character| matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
    {
        bail!("branch contains a character Git refs cannot use");
    }
    for component in value.split('/') {
        if component.is_empty() {
            bail!("branch contains an empty path component");
        }
        if component.starts_with('.') {
            bail!("branch has a path component starting with '.'");
        }
        if component.ends_with(".lock") {
            bail!("branch has a path component ending with '.lock'");
        }
    }
    Ok(())
}

fn validate_image_tag(value: &str) -> Result<()> {
    validate_field("image_tag", value, MAX_IMAGE_TAG_LEN)?;
    if value.chars().any(char::is_whitespace) {
        bail!("image_tag must not contain whitespace");
    }
    if value.contains('@') {
        bail!("image_tag must not contain a digest");
    }
    if value.starts_with('/') || value.ends_with('/') {
        bail!("image_tag must not start or end with '/'");
    }
    if !value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
    {
        bail!("image_tag must start with a letter or digit");
    }
    if !value.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/' | ':')
    }) {
        bail!("image_tag contains unsupported characters");
    }
    if value.contains("..") || value.contains("//") {
        bail!("image_tag contains an invalid separator sequence");
    }

    // A colon may only appear in the registry (first component, as a numeric
    // port) or in the final component (as the tag).
    let components: Vec<&str> = value.split('/').collect();
    validate_image_tag_colons(&components)?;
    validate_image_tag_name(components[components.len() - 1])
}

fn validate_image_tag_colons(components: &[&str]) -> Result<()> {
    let last_index = components.len() - 1;
    for (index, component) in components.iter().enumerate() {
        let Some((head, tail)) = component.split_once(':') else {
            continue;
        };
        if index == 0 && last_index > 0 {
            if head.is_empty()
                || tail.is_empty()
                || !tail.chars().all(|character| character.is_ascii_digit())
            {
                bail!("image_tag has an invalid registry port");
            }
            continue;
        }
        if index != last_index {
            bail!("image_tag has an invalid ':' placement");
        }
    }
    Ok(())
}

fn validate_image_tag_name(final_component: &str) -> Result<()> {
    let Some((name, tag)) = final_component.split_once(':') else {
        return Ok(());
    };
    if name.is_empty() {
        bail!("image_tag has an empty repository name");
    }
    if tag.is_empty() {
        bail!("image_tag has an empty tag");
    }
    if tag.len() > 128 {
        bail!("image_tag tag is too long");
    }
    if !tag
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        bail!("image_tag has an invalid tag");
    }
    if !tag
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-'))
    {
        bail!("image_tag has an invalid tag");
    }
    Ok(())
}

fn validate_repo_path(field: &str, value: &str, allow_root: bool) -> Result<()> {
    validate_field(field, value, MAX_PATH_LEN)?;
    if value.chars().any(char::is_whitespace) {
        bail!("{field} must not contain whitespace");
    }
    if value.contains('\\') {
        bail!("{field} must use '/' separators");
    }
    if value.starts_with('/') || value.starts_with('~') {
        bail!("{field} must be relative to the repository root");
    }
    if value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        bail!("{field} must not be an absolute Windows path");
    }
    if value.contains("//") {
        bail!("{field} must not contain empty path components");
    }
    if value.split('/').any(|component| component == "..") {
        bail!("{field} must not contain '..' path segments");
    }
    let is_root = value
        .split('/')
        .all(|component| component.is_empty() || component == ".");
    if is_root && !allow_root {
        bail!("{field} must point to a file, not the repository root");
    }
    Ok(())
}

fn is_https_url(value: &str) -> bool {
    value
        .as_bytes()
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"https://"))
}
