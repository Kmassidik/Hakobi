use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;

use super::Catalog;

/// Length of a ChaCha20-Poly1305 key and nonce in bytes.
pub(super) const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

impl Catalog {
    pub fn encrypt_secret(&self, secret: &str) -> Result<String> {
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key));
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), secret.as_bytes())
            .map_err(|_| anyhow!("encrypt provider secret"))?;
        let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        payload.extend_from_slice(&nonce_bytes);
        payload.extend_from_slice(&ciphertext);
        Ok(BASE64.encode(payload))
    }

    pub fn decrypt_secret(&self, encoded: &str) -> Result<String> {
        let payload = BASE64
            .decode(encoded)
            .context("decode provider secret encoding")?;
        if payload.len() < NONCE_LEN {
            bail!("provider secret ciphertext is truncated");
        }
        let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&self.key));
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| anyhow!("decrypt provider secret"))?;
        String::from_utf8(plaintext).context("provider secret is not valid UTF-8")
    }
}

pub(super) fn load_or_create_key(database: &Path) -> Result<[u8; KEY_LEN]> {
    let path = key_path(database);
    match fs::read(&path) {
        Ok(bytes) => key_from_bytes(&path, &bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => create_key(&path),
        Err(error) => Err(error).with_context(|| format!("read catalog key {}", path.display())),
    }
}

fn create_key(path: &Path) -> Result<[u8; KEY_LEN]> {
    let mut key = [0u8; KEY_LEN];
    rand::rngs::OsRng.fill_bytes(&mut key);
    match create_key_file(path, &key) {
        Ok(()) => Ok(key),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let bytes =
                fs::read(path).with_context(|| format!("read catalog key {}", path.display()))?;
            key_from_bytes(path, &bytes)
        }
        Err(error) => Err(error).with_context(|| format!("create catalog key {}", path.display())),
    }
}

fn key_path(database: &Path) -> PathBuf {
    database.with_extension("key")
}

fn key_from_bytes(path: &Path, bytes: &[u8]) -> Result<[u8; KEY_LEN]> {
    if bytes.len() != KEY_LEN {
        bail!(
            "catalog key {} must be {KEY_LEN} bytes, found {}",
            path.display(),
            bytes.len()
        );
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(bytes);
    Ok(key)
}

fn create_key_file(path: &Path, key: &[u8; KEY_LEN]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(key)?;
    file.sync_all()?;
    Ok(())
}
