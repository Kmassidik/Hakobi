//! CP9: GitHub integration storage — app credentials, user connections,
//! and webhook delivery dedupe, all encrypted with the existing vault.

use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;

/// Admin-level GitHub App credentials (single row, id = "app").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubApp {
    pub app_id: i64,
    pub client_id: String,
    pub installation_id: i64,
    pub installed_at: String,
    /// Decrypted secrets; never stored or logged as plaintext outside
    /// this struct instance.
    pub client_secret: String,
    pub webhook_secret: String,
    pub private_key_pem: String,
}

/// One Hakobi user's GitHub identity and OAuth access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubConnection {
    pub user_id: String,
    pub github_login: String,
    pub github_user_id: i64,
    pub access_token: String,
}

impl Catalog {
    /// Stores (or replaces) the GitHub App credentials, encrypted.
    pub fn save_github_app(
        &self,
        app_id: i64,
        client_id: &str,
        client_secret: &str,
        webhook_secret: &str,
        private_key_pem: &str,
        installation_id: i64,
    ) -> Result<()> {
        self.connection.execute(
            "INSERT INTO github_app (id, app_id, client_id, client_secret, webhook_secret, \
             private_key_pem, installation_id, installed_at) \
             VALUES ('app', ?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ON CONFLICT(id) DO UPDATE SET app_id=excluded.app_id, client_id=excluded.client_id, \
             client_secret=excluded.client_secret, webhook_secret=excluded.webhook_secret, \
             private_key_pem=excluded.private_key_pem, installation_id=excluded.installation_id, \
             installed_at=excluded.installed_at",
            params![
                app_id,
                client_id,
                self.encrypt_secret(client_secret)?,
                self.encrypt_secret(webhook_secret)?,
                self.encrypt_secret(private_key_pem)?,
                installation_id,
            ],
        )?;
        Ok(())
    }

    /// Loads and decrypts the GitHub App credentials, if installed.
    pub fn load_github_app(&self) -> Result<Option<GithubApp>> {
        let row = self
            .connection
            .query_row(
                "SELECT app_id, client_id, client_secret, webhook_secret, private_key_pem, \
                 installation_id, installed_at FROM github_app WHERE id = 'app'",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()
            .context("load github app")?;
        let Some((
            app_id,
            client_id,
            client_secret,
            webhook_secret,
            private_key_pem,
            installation_id,
            installed_at,
        )) = row
        else {
            return Ok(None);
        };
        Ok(Some(GithubApp {
            app_id,
            client_id,
            client_secret: self.decrypt_secret(&client_secret)?,
            webhook_secret: self.decrypt_secret(&webhook_secret)?,
            private_key_pem: self.decrypt_secret(&private_key_pem)?,
            installation_id,
            installed_at,
        }))
    }

    /// Whether the admin completed the App installation.
    pub fn github_app_installed(&self) -> Result<bool> {
        let installed: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM github_app WHERE id = 'app')",
            [],
            |row| row.get(0),
        )?;
        Ok(installed)
    }

    /// Upserts a user's GitHub identity and access token (encrypted).
    pub fn save_github_connection(
        &self,
        user_id: &str,
        github_login: &str,
        github_user_id: i64,
        access_token: &str,
    ) -> Result<()> {
        let id = format!("user:{user_id}");
        self.connection.execute(
            "INSERT INTO github_connections (id, user_id, github_login, github_user_id, \
             access_token, token_expires_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), \
             strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) \
             ON CONFLICT(id) DO UPDATE SET github_login=excluded.github_login, \
             github_user_id=excluded.github_user_id, access_token=excluded.access_token, \
             token_expires_at=excluded.token_expires_at",
            params![
                id,
                user_id,
                github_login,
                github_user_id,
                self.encrypt_secret(access_token)?,
            ],
        )?;
        Ok(())
    }

    /// Loads a user's GitHub connection (decrypted token), if connected.
    pub fn load_github_connection(&self, user_id: &str) -> Result<Option<GithubConnection>> {
        let id = format!("user:{user_id}");
        let row = self
            .connection
            .query_row(
                "SELECT github_login, github_user_id, access_token FROM github_connections \
                 WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .context("load github connection")?;
        let Some((login, uid, token)) = row else {
            return Ok(None);
        };
        Ok(Some(GithubConnection {
            user_id: user_id.to_owned(),
            github_login: login,
            github_user_id: uid,
            access_token: self.decrypt_secret(&token)?,
        }))
    }

    /// Removes a user's GitHub connection.
    pub fn delete_github_connection(&self, user_id: &str) -> Result<()> {
        let id = format!("user:{user_id}");
        self.connection
            .execute("DELETE FROM github_connections WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Records a webhook delivery id; returns false when already seen
    /// (replay).
    pub fn record_webhook_delivery(&self, delivery_id: &str) -> Result<bool> {
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO webhook_deliveries (delivery_id, received_at) \
             VALUES (?1, ?2)",
            params![delivery_id, sql_now()],
        )?;
        Ok(inserted == 1)
    }
}

/// ISO-8601 UTC timestamp matching the catalog format.
fn sql_now() -> String {
    // Computed here because rusqlite params cannot call strftime.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Convert epoch to ISO-8601 without external crates (calendar math).
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (h, m, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{sec:02}Z")
}

/// Days-since-epoch to civil date (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
