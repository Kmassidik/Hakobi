use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

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
const MAX_SESSIONS: usize = 1024;
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

#[derive(Debug)]
struct Session {
    expires_at: Instant,
}

#[derive(Debug, Default)]
pub struct Sessions {
    inner: Mutex<HashMap<[u8; 32], Session>>,
}

impl Sessions {
    pub async fn create(&self) -> String {
        let token = random_token();
        let key = token_key(&token);
        let now = Instant::now();
        let mut sessions = self.inner.lock().await;
        sessions.retain(|_, session| session.expires_at > now);
        if sessions.len() >= MAX_SESSIONS {
            if let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, session)| session.expires_at)
                .map(|(key, _)| *key)
            {
                sessions.remove(&oldest);
            }
        }
        sessions.insert(
            key,
            Session {
                expires_at: now + SESSION_LIFETIME,
            },
        );
        token
    }

    pub async fn is_valid(&self, token: Option<&str>) -> bool {
        let Some(token) = token else {
            return false;
        };
        let sessions = self.inner.lock().await;
        sessions
            .get(&token_key(token))
            .is_some_and(|session| session.expires_at > Instant::now())
    }

    pub async fn revoke(&self, token: Option<&str>) {
        if let Some(token) = token {
            self.inner.lock().await.remove(&token_key(token));
        }
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
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}")
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
        let sessions = Sessions::default();
        let token = sessions.create().await;
        assert!(sessions.is_valid(Some(&token)).await);
        sessions.revoke(Some(&token)).await;
        assert!(!sessions.is_valid(Some(&token)).await);
    }

    #[test]
    fn password_hashing_accepts_only_the_original_password() {
        let hash = hash_password("a sufficiently long password").unwrap();
        assert!(verify_password(&hash, "a sufficiently long password"));
        assert!(!verify_password(&hash, "a different password"));
    }
}
