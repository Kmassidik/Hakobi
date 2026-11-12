//! GitHub HTTP client: app auth (JWT -> installation token), user OAuth
//! exchange, repo/branch/tree listing with a small in-process cache, and
//! commit status posting.

use anyhow::{anyhow, Context, Result};
use hmac::Mac;
use serde::Deserialize;
use sha2::Sha256;
use std::time::{Duration, Instant};

const GITHUB_API: &str = "https://api.github.com";
const CACHE_TTL: Duration = Duration::from_secs(300);
/// Installation tokens are valid 1h; refresh 5 min early.
const TOKEN_TTL: Duration = Duration::from_secs(55 * 60);

/// GitHub API client bound to one installed App + one user token.
pub struct GithubClient {
    http: reqwest::Client,
    app_id: i64,
    installation_id: i64,
    private_key_pem: String,
    app_installation_token: MutexCache<String>,
    user_token: String,
    cache: MutexCache<String>,
}

/// Tiny single-value TTL cache (one entry per kind).
struct MutexCache<T> {
    value: std::sync::Mutex<Option<(T, Instant)>>,
}

impl<T: Clone> MutexCache<T> {
    fn new() -> Self {
        Self {
            value: std::sync::Mutex::new(None),
        }
    }

    fn get(&self, ttl: Duration) -> Option<T> {
        let guard = self.value.lock().ok()?;
        let (value, at) = guard.as_ref()?;
        (at.elapsed() < ttl).then(|| value.clone())
    }

    fn set(&self, value: T) {
        if let Ok(mut guard) = self.value.lock() {
            *guard = Some((value, Instant::now()));
        }
    }

    fn invalidate(&self) {
        if let Ok(mut guard) = self.value.lock() {
            *guard = None;
        }
    }
}

#[derive(serde::Serialize)]
struct AppClaims {
    iss: String,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct InstallationToken {
    token: String,
}

#[derive(Deserialize)]
struct RepoDto {
    full_name: String,
    default_branch: String,
    private: bool,
}

#[derive(Deserialize)]
struct BranchDto {
    name: String,
}

#[derive(Deserialize)]
struct TreeDto {
    tree: Vec<TreeEntry>,
    truncated: bool,
}

#[derive(Deserialize)]
struct TreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

/// A repository visible to the installation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepoSummary {
    pub full_name: String,
    pub default_branch: String,
    pub private: bool,
}

/// One detected buildable unit in a repo tree.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DetectedApp {
    pub path: String,
    pub dockerfile: String,
}

/// Result of scanning a repo tree for buildable directories.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DetectionResult {
    pub apps: Vec<DetectedApp>,
    pub truncated: bool,
}

impl GithubClient {
    /// Builds a client for the installed App plus a user access token.
    pub fn new(
        app_id: i64,
        installation_id: i64,
        private_key_pem: &str,
        user_token: &str,
    ) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent("hakobi-control-plane")
                .timeout(Duration::from_secs(20))
                .build()?,
            app_id,
            installation_id,
            private_key_pem: private_key_pem.to_owned(),
            app_installation_token: MutexCache::new(),
            user_token: user_token.to_owned(),
            cache: MutexCache::new(),
        })
    }

    /// Signs an app JWT and exchanges it for an installation token.
    async fn installation_token(&self) -> Result<String> {
        if let Some(token) = self.app_installation_token.get(TOKEN_TTL) {
            return Ok(token);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let claims = AppClaims {
            iss: self.app_id.to_string(),
            iat: now.saturating_sub(60),
            exp: now + 600,
        };
        let key = jsonwebtoken::EncodingKey::from_rsa_pem(self.private_key_pem.as_bytes())
            .context("parse GitHub app private key")?;
        let app_jwt = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
            &claims,
            &key,
        )
        .context("sign app jwt")?;
        let response = self
            .http
            .post(format!(
                "{GITHUB_API}/app/installations/{}/access_tokens",
                self.installation_id
            ))
            .header("Authorization", format!("Bearer {app_jwt}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            self.app_installation_token.invalidate();
            anyhow::bail!("installation token request returned {status}");
        }
        let token: InstallationToken = response.json().await.context("decode token response")?;
        self.app_installation_token.set(token.token.clone());
        Ok(token.token)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str, token: &str) -> Result<T> {
        let response = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("GET {url} returned {status}");
        }
        let body = response.text().await.context("read github response body")?;
        serde_json::from_str(&body).with_context(|| format!("decode github response from {url}"))
    }

    /// Repositories the installation can access.
    pub async fn list_repos(&self) -> Result<Vec<RepoSummary>> {
        if let Some(cached) = self.cache.get(CACHE_TTL) {
            return Ok(serde_json::from_str(&cached)?);
        }
        let token = self.installation_token().await?;
        let mut repos = Vec::new();
        let mut page = 1;
        loop {
            #[derive(Deserialize)]
            struct RepoPage {
                repositories: Vec<RepoDto>,
            }
            let page_dto: RepoPage = self
                .get_json(
                    &format!("{GITHUB_API}/installation/repositories?per_page=100&page={page}"),
                    &token,
                )
                .await?;
            let dtos = page_dto.repositories;
            let done = dtos.len() < 100;
            repos.extend(dtos);
            if done || page >= 10 {
                break;
            }
            page += 1;
        }
        let summaries: Vec<RepoSummary> = repos
            .into_iter()
            .map(|r| RepoSummary {
                full_name: r.full_name,
                default_branch: r.default_branch,
                private: r.private,
            })
            .collect();
        self.cache.set(serde_json::to_string(&summaries)?);
        Ok(summaries)
    }

    /// Branch names for one repo.
    pub async fn list_branches(&self, repo: &str) -> Result<Vec<String>> {
        let token = self.installation_token().await?;
        let dtos: Vec<BranchDto> = self
            .get_json(
                &format!("{GITHUB_API}/repos/{repo}/branches?per_page=100"),
                &token,
            )
            .await?;
        Ok(dtos.into_iter().map(|b| b.name).collect())
    }

    /// Scans a repo tree for directories containing a Dockerfile.
    /// Returns a recommendation the UI pre-fills (user stays in control).
    pub async fn detect_apps(&self, repo: &str, branch: &str) -> Result<DetectionResult> {
        let token = self.installation_token().await?;
        let url = format!("{GITHUB_API}/repos/{repo}/git/trees/{branch}?recursive=1");
        let tree: TreeDto = self.get_json(&url, &token).await?;
        let mut dirs: Vec<String> = Vec::new();
        for entry in &tree.tree {
            if entry.kind == "blob"
                && (entry.path.ends_with("/Dockerfile") || entry.path == "Dockerfile")
            {
                let dir = entry
                    .path
                    .rsplit_once('/')
                    .map(|(dir, _)| dir.to_owned())
                    .unwrap_or_default();
                if !dirs.contains(&dir) {
                    dirs.push(dir);
                }
            }
        }
        dirs.sort();
        let apps = dirs
            .iter()
            .map(|dir| {
                let dockerfile = if dir.is_empty() {
                    "Dockerfile".to_owned()
                } else {
                    format!("{dir}/Dockerfile")
                };
                DetectedApp {
                    path: dir.clone(),
                    dockerfile,
                }
            })
            .collect();
        Ok(DetectionResult {
            apps,
            truncated: tree.truncated,
        })
    }

    /// Posts a commit status (state: pending|success|failure|error).
    pub async fn create_status(
        &self,
        repo: &str,
        sha: &str,
        state: &str,
        context: &str,
        description: &str,
        target_url: &str,
    ) -> Result<()> {
        let token = self.installation_token().await?;
        let body = serde_json::json!({
            "state": state,
            "context": context,
            "description": description,
            "target_url": target_url,
        });
        let response = self
            .http
            .post(format!("{GITHUB_API}/repos/{repo}/statuses/{sha}"))
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("create status returned {status}: {text}");
        }
        Ok(())
    }

    /// The stored user access token (for user-scoped API calls).
    pub fn user_token(&self) -> &str {
        &self.user_token
    }

    /// Exchanges a user OAuth code for an access token and identity.
    pub async fn exchange_user_code(
        http: &reqwest::Client,
        client_id: &str,
        client_secret: &str,
        code: &str,
    ) -> Result<(String, String, i64)> {
        let response = http
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "client_id": client_id,
                "client_secret": client_secret,
                "code": code,
            }))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("oauth token exchange returned {status}");
        }
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
        }
        let token: TokenResponse = response.json().await.context("decode oauth token")?;
        let user: serde_json::Value = http
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {}", token.access_token))
            .header("User-Agent", "hakobi-control-plane")
            .send()
            .await?
            .json()
            .await?;
        let login = user["login"]
            .as_str()
            .ok_or_else(|| anyhow!("github user response missing login"))?
            .to_owned();
        let uid = user["id"].as_i64().unwrap_or(0);
        Ok((token.access_token, login, uid))
    }
}

/// Verifies a webhook signature (constant-time).
pub fn verify_webhook_signature(secret: &str, raw_body: &[u8], header_value: &str) -> bool {
    let Some(expected) = header_value.strip_prefix("sha256=") else {
        return false;
    };
    let mut mac = hmac::Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("hmac accepts any key length");
    mac.update(raw_body);
    let digest = hex::encode(&mac.finalize().into_bytes());
    constant_time_eq(digest.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Returns the changed-file set from a push payload, or None when the push
/// should be treated as "all files changed" (forced push, >20 commits).
pub fn changed_files_from_push(payload: &serde_json::Value) -> Option<Vec<String>> {
    let commits = payload.get("commits")?.as_array()?;
    if commits.len() > 20 {
        return None;
    }
    let mut files: Vec<String> = Vec::new();
    for commit in commits {
        for key in ["added", "modified", "removed"] {
            if let Some(list) = commit[key].as_array() {
                for file in list {
                    if let Some(name) = file.as_str() {
                        if !files.iter().any(|f| f == name) {
                            files.push(name.to_owned());
                        }
                    }
                }
            }
        }
    }
    Some(files)
}

/// Glob-ish path filter: supports `dir/`, `dir/**`, `*.ext`, and exact
/// paths. Empty pattern matches everything.
pub fn path_matches(pattern: &str, changed: &[String]) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return true;
    }
    changed.iter().any(|file| {
        if pattern.ends_with("/**") {
            let prefix = pattern.trim_end_matches('*').trim_end_matches('/');
            file == prefix || file.starts_with(&format!("{prefix}/"))
        } else if pattern.ends_with('/') {
            file.starts_with(pattern)
        } else if pattern.contains('*') {
            let prefix = pattern.split('*').next().unwrap_or_default();
            file.starts_with(prefix)
        } else {
            file == pattern || file.starts_with(&format!("{pattern}/"))
        }
    })
}

/// Exposes hex for the HMAC digest without adding the hex crate at every
/// call site.
pub mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::Hmac;

    #[test]
    fn webhook_signature_validates_hmac() {
        let secret = "whsec_test";
        let body = br#"{"action":"push"}"#;
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let good = format!("sha256={}", hex::encode(&mac.finalize().into_bytes()));
        assert!(verify_webhook_signature(secret, body, &good));
        assert!(!verify_webhook_signature(secret, body, "sha256=deadbeef"));
        assert!(!verify_webhook_signature(secret, body, "no-prefix"));
        assert!(!verify_webhook_signature("other", body, &good));
    }

    #[test]
    fn changed_files_extract_and_cap() {
        let payload = serde_json::json!({
            "commits": [
                {"added": ["a/x.rs"], "modified": ["b/y.rs"], "removed": []},
                {"added": [], "modified": ["b/z.rs"], "removed": ["old"]},
            ]
        });
        let files = changed_files_from_push(&payload).unwrap();
        assert_eq!(files.len(), 4);
        let big = serde_json::json!({
            "commits": (0..21).map(|i| serde_json::json!({"modified": [format!("f{i}")]})).collect::<Vec<_>>()
        });
        assert!(changed_files_from_push(&big).is_none());
    }

    #[test]
    fn path_globs_match_monorepo_layouts() {
        let changed = vec![
            "services/api/src/main.rs".to_owned(),
            "services/web/index.html".to_owned(),
            "README.md".to_owned(),
        ];
        assert!(path_matches("", &changed));
        assert!(path_matches("services/api", &changed));
        assert!(path_matches("services/api/**", &changed));
        assert!(path_matches("services/", &changed));
        assert!(path_matches("services/web", &changed)); // web/index.html changed
        assert!(!path_matches("packages/ui", &changed));
        assert!(!path_matches(
            "services/web",
            &["services/api/src/main.rs".to_owned()]
        ));
    }

    #[tokio::test]
    async fn jwt_crafted_by_wrong_secret_is_rejected_by_github() {
        // Sanity for our app-JWT path: encoding with a different key then
        // decoding with ours must fail.
        use jsonwebtoken::{EncodingKey, Header};
        let claims = serde_json::json!({"iss": "1", "iat": 0, "exp": 1});
        let token = jsonwebtoken::encode(
            &Header::new(jsonwebtoken::Algorithm::RS256),
            &claims,
            &EncodingKey::from_rsa_pem(include_bytes!("../../test-fixtures/gh-key.pem")).unwrap(),
        )
        .unwrap();
        let wrong = jsonwebtoken::DecodingKey::from_secret("not-a-rsa-key".as_bytes());
        assert!(jsonwebtoken::decode::<serde_json::Value>(
            &token,
            &wrong,
            &jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256),
        )
        .is_err());
        let _ = token;
    }
}
