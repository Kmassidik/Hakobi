use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use std::collections::HashSet;
use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::{config::PublicConfig, utils::random::random_token};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use http::HeaderMap;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

const SESSION_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);
const MAX_REVOKED: usize = 4096;
const LOGIN_WINDOW: Duration = Duration::from_secs(5 * 60);
const MAX_LOGIN_FAILURES: u8 = 8;
const MAX_LOGIN_KEYS: usize = 4096;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|error| anyhow::anyhow!("could not hash password: {error}"))?
        .to_string())
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    let Ok(hash) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &hash)
        .is_ok()
}

/// JWT session claims. Stateless: any container instance validates any
/// token, so sessions survive deploys without a shared store.
#[derive(Debug, Serialize, Deserialize)]
struct SessionClaims {
    /// Issued-at, seconds since the Unix epoch.
    iat: u64,
    /// Expiry, seconds since the Unix epoch.
    exp: u64,
    /// Random session id; also seeds the revocation denylist.
    jti: String,
}

/// Signs and validates session JWTs. The secret persists in the data dir so
/// tokens remain valid across container restarts and deploys.
pub struct Sessions {
    encoding: jsonwebtoken::EncodingKey,
    decoding: jsonwebtoken::DecodingKey,
    /// Revoked session ids (logout). In-memory: clearing it on restart only
    /// shortens revoked sessions, never extends valid ones.
    revoked: Mutex<HashSet<[u8; 32]>>,
}

impl Sessions {
    /// Loads the signing secret from `<data_dir>/session.secret`, creating a
    /// random one on first run. The file must have `0600` permissions.
    pub fn load(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join("session.secret");
        let secret = match std::fs::read_to_string(&path) {
            Ok(existing) if existing.len() >= 32 => existing,
            _ => {
                let generated = random_token();
                std::fs::write(&path, &generated)
                    .with_context(|| format!("write {}", path.display()))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                        .with_context(|| format!("chmod {}", path.display()))?;
                }
                generated
            }
        };
        Ok(Self {
            encoding: jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
            decoding: jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
            revoked: Mutex::new(HashSet::new()),
        })
    }

    /// Issues a signed session token valid for [`SESSION_LIFETIME`].
    pub async fn create(&self) -> String {
        let now = EpochMillis::now_secs();
        let claims = SessionClaims {
            iat: now,
            exp: now + SESSION_LIFETIME.as_secs(),
            jti: random_token(),
        };
        jsonwebtoken::encode(&jsonwebtoken::Header::default(), &claims, &self.encoding)
            .expect("session jwt encode with valid claims cannot fail")
    }

    /// Validates signature, expiry, and the revocation denylist.
    pub async fn is_valid(&self, token: Option<&str>) -> bool {
        let Some(token) = token else {
            return false;
        };
        let Ok(claims) = jsonwebtoken::decode::<SessionClaims>(
            token,
            &self.decoding,
            &jsonwebtoken::Validation::default(),
        ) else {
            return false;
        };
        let id = token_key(&claims.claims.jti);
        let revoked = self.revoked.lock().await;
        !revoked.contains(&id)
    }

    /// Adds the session id to the revocation denylist (logout).
    pub async fn revoke(&self, token: Option<&str>) {
        let Some(token) = token else {
            return;
        };
        let Ok(data) = jsonwebtoken::decode::<SessionClaims>(
            token,
            &self.decoding,
            &jsonwebtoken::Validation::default(),
        ) else {
            return;
        };
        let id = token_key(&data.claims.jti);
        let mut revoked = self.revoked.lock().await;
        revoked.insert(id);
        if revoked.len() > MAX_REVOKED {
            revoked.clear();
        }
    }
}

/// Seconds since the Unix epoch.
struct EpochMillis;

impl EpochMillis {
    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    }
}

fn token_key(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[derive(Debug)]
struct Failures {
    first: Instant,
    count: u8,
}

#[derive(Debug, Default)]
pub struct LoginLimiter {
    inner: Mutex<HashMap<String, Failures>>,
}

impl LoginLimiter {
    pub async fn allows(&self, key: &str) -> bool {
        let limiter = self.inner.lock().await;
        limiter.get(key).is_none_or(|failure| {
            failure.first.elapsed() >= LOGIN_WINDOW || failure.count < MAX_LOGIN_FAILURES
        })
    }

    pub async fn record_failure(&self, key: &str) {
        let now = Instant::now();
        let mut limiter = self.inner.lock().await;
        if limiter.len() >= MAX_LOGIN_KEYS {
            limiter.retain(|_, failure| failure.first.elapsed() < LOGIN_WINDOW);
        }
        let has_capacity = limiter.len() < MAX_LOGIN_KEYS;
        match limiter.get_mut(key) {
            Some(failure) if failure.first.elapsed() < LOGIN_WINDOW => {
                failure.count = failure.count.saturating_add(1);
            }
            Some(failure) => {
                failure.first = now;
                failure.count = 1;
            }
            None if has_capacity => {
                limiter.insert(
                    key.to_owned(),
                    Failures {
                        first: now,
                        count: 1,
                    },
                );
            }
            None => {}
        }
    }

    pub async fn clear(&self, key: &str) {
        self.inner.lock().await.remove(key);
    }
}

pub fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(http::header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then_some(value))
}

pub fn create_cookie(config: &PublicConfig, name: &str, value: &str, max_age: u64) -> String {
    let secure = if config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    // SameSite=Lax: cross-site top-level GET navigations (GitHub manifest/OAuth
    // callbacks) must carry the session cookie. Lax still withholds cookies on
    // cross-site POSTs, so CSRF protection is unaffected; forms also carry a
    // CSRF token.
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}")
}

pub fn clear_cookie(config: &PublicConfig, name: &str) -> String {
    create_cookie(config, name, "", 0)
}

pub fn csrf_is_valid(headers: &HeaderMap, config: &PublicConfig, submitted: &str) -> bool {
    let Some(expected) = cookie_value(headers, config.csrf_cookie_name()) else {
        return false;
    };
    if expected.len() != 43 || submitted.len() != 43 {
        return false;
    }
    expected.as_bytes().ct_eq(submitted.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn revoked_sessions_stop_working() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = Sessions::load(dir.path()).unwrap();
        let token = sessions.create().await;
        assert!(sessions.is_valid(Some(&token)).await);
        sessions.revoke(Some(&token)).await;
        assert!(!sessions.is_valid(Some(&token)).await);
    }

    #[tokio::test]
    async fn sessions_survive_process_restart_with_persisted_secret() {
        // Deploy simulation: same data dir, new Sessions instance (fresh
        // process). The browser cookie must still validate.
        let dir = tempfile::tempdir().unwrap();
        let token = {
            let sessions = Sessions::load(dir.path()).unwrap();
            sessions.create().await
        };
        let restarted = Sessions::load(dir.path()).unwrap();
        assert!(restarted.is_valid(Some(&token)).await);
        // A token minted by a different secret must be rejected.
        let other = tempfile::tempdir().unwrap();
        let foreign = Sessions::load(other.path()).unwrap();
        let foreign_token = foreign.create().await;
        assert!(!restarted.is_valid(Some(&foreign_token)).await);
    }

    #[tokio::test]
    async fn expired_sessions_stop_working() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = Sessions::load(dir.path()).unwrap();
        // Craft an already-expired token with the same key.
        use jsonwebtoken::{EncodingKey, Validation};
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let claims = serde_json::json!({
            "iat": now - 7200,
            "exp": now - 3600,
            "jti": "expired",
        });
        let secret = std::fs::read_to_string(dir.path().join("session.secret")).unwrap();
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        assert!(!sessions.is_valid(Some(&token)).await);
        // Default validation enforces exp; double-check explicitly.
        assert!(jsonwebtoken::decode::<SessionClaims>(
            &token,
            &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
            &Validation::default(),
        )
        .is_err());
    }

    #[test]
    fn password_hashing_accepts_only_the_original_password() {
        let hash = hash_password("a sufficiently long password").unwrap();
        assert!(verify_password(&hash, "a sufficiently long password"));
        assert!(!verify_password(&hash, "a different password"));
    }
}
