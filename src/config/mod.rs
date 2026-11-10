use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const DEFAULT_PORT: u16 = 3000;
const CONFIG_VERSION: u8 = 1;

pub fn listen_port() -> Result<u16> {
    let value = match env::var("PORT") {
        Ok(value) => value,
        Err(env::VarError::NotPresent) => return Ok(DEFAULT_PORT),
        Err(env::VarError::NotUnicode(_)) => bail!("PORT must be valid UTF-8"),
    };
    let port = value
        .trim()
        .parse::<u16>()
        .with_context(|| format!("invalid PORT value: {value}"))?;
    if port == 0 {
        bail!("PORT must be between 1 and 65535");
    }
    Ok(port)
}

pub fn local_base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

#[derive(Clone, Debug, Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u8,
    pub auth: AuthConfig,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloudflare: Option<CloudflareConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct AuthConfig {
    pub username: String,
    pub password_hash: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct CloudflareConfig {
    pub account_id: String,
    pub zone_id: String,
    pub tunnel_id: String,
    pub tunnel_token: String,
}

#[derive(Clone)]
pub struct PublicConfig {
    pub username: String,
    pub password_hash: String,
    pub base_url: String,
    pub secure_cookies: bool,
}

impl Config {
    pub fn new(
        username: String,
        password_hash: String,
        base_url: String,
        cloudflare: Option<CloudflareConfig>,
    ) -> Result<Self> {
        let base_url =
            canonical_base_url(&base_url, cloudflare.is_none()).map_err(anyhow::Error::msg)?;
        let config = Self {
            version: CONFIG_VERSION,
            auth: AuthConfig {
                username,
                password_hash,
            },
            base_url,
            cloudflare,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn public(&self) -> PublicConfig {
        PublicConfig {
            username: self.auth.username.clone(),
            password_hash: self.auth.password_hash.clone(),
            base_url: self.base_url.clone(),
            secure_cookies: self.base_url.starts_with("https://"),
        }
    }

    pub fn hostname(&self) -> Result<String> {
        let url = Url::parse(&self.base_url)?;
        url.host_str()
            .map(str::to_owned)
            .context("base URL has no hostname")
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != CONFIG_VERSION {
            bail!("unsupported configuration version {}", self.version);
        }
        validate_username(&self.auth.username).map_err(anyhow::Error::msg)?;
        if !self.auth.password_hash.starts_with("$argon2id$") {
            bail!("password hash must use Argon2id");
        }
        let canonical = canonical_base_url(&self.base_url, self.cloudflare.is_none())
            .map_err(anyhow::Error::msg)?;
        if self.base_url != canonical {
            bail!("base URL must use canonical origin form: {canonical}");
        }

        if let Some(cloudflare) = &self.cloudflare {
            validate_cloudflare_id(&cloudflare.account_id, "account ID")?;
            validate_cloudflare_id(&cloudflare.zone_id, "zone ID")?;
            uuid::Uuid::parse_str(&cloudflare.tunnel_id)
                .context("Cloudflare tunnel ID is not a UUID")?;
            validate_secret(&cloudflare.tunnel_token, "Cloudflare tunnel token")?;
        }
        Ok(())
    }
}

impl PublicConfig {
    pub fn session_cookie_name(&self) -> &'static str {
        if self.secure_cookies {
            "__Host-hakobi_session"
        } else {
            "hakobi_session"
        }
    }

    pub fn csrf_cookie_name(&self) -> &'static str {
        if self.secure_cookies {
            "__Host-hakobi_csrf"
        } else {
            "hakobi_csrf"
        }
    }
}

#[derive(Clone, Debug)]
pub struct ConfigStore {
    data_dir: PathBuf,
}

impl ConfigStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    pub fn discover(override_dir: Option<PathBuf>) -> Result<Self> {
        if let Some(dir) = override_dir {
            return Ok(Self::new(dir));
        }
        if let Some(dir) = std::env::var_os("HAKOBI_DATA_DIR") {
            return Ok(Self::new(PathBuf::from(dir)));
        }
        let dirs = ProjectDirs::from("dev", "Hakobi", "Hakobi")
            .context("could not determine the application data directory")?;
        Ok(Self::new(dirs.data_local_dir().to_path_buf()))
    }

    pub fn exists(&self) -> bool {
        self.path().is_file()
    }

    pub fn path(&self) -> PathBuf {
        self.data_dir.join("config.toml")
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn load(&self) -> Result<Config> {
        let path = self.path();
        reject_insecure_permissions(&path)?;
        let mut source = fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let config = toml::from_str(&source);
        source.zeroize();
        let config: Config = config.context("invalid configuration")?;
        config.validate()?;
        Ok(config)
    }

    pub fn save_new(&self, config: &Config) -> Result<()> {
        self.save(config, true)
    }

    pub fn replace(&self, config: &Config) -> Result<()> {
        self.save(config, false)
    }

    fn save(&self, config: &Config, no_clobber: bool) -> Result<()> {
        config.validate()?;
        fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("could not create {}", self.data_dir.display()))?;
        set_private_dir_permissions(&self.data_dir)?;

        let path = self.path();
        if no_clobber && path.exists() {
            bail!(
                "{} already exists; refusing to overwrite it",
                path.display()
            );
        }

        let mut serialized = Zeroizing::new(toml::to_string_pretty(config)?);
        let result = (|| -> Result<()> {
            let mut file = tempfile::NamedTempFile::new_in(&self.data_dir)?;
            file.write_all(serialized.as_bytes())?;
            file.as_file().sync_all()?;
            if no_clobber {
                file.persist_noclobber(&path).map_err(|error| error.error)?;
            } else {
                file.persist(&path).map_err(|error| error.error)?;
            }
            Ok(())
        })();
        serialized.zeroize();
        result.with_context(|| format!("could not save {}", path.display()))
    }
}

pub fn validate_username(username: &str) -> std::result::Result<(), String> {
    let valid = !username.is_empty()
        && username.len() <= 32
        && username
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphanumeric())
        && username.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-')
        });
    if valid {
        Ok(())
    } else {
        Err("use 1–32 ASCII letters, digits, dots, dashes, or underscores; start with a letter or digit".into())
    }
}

pub fn validate_password(password: &str) -> std::result::Result<(), String> {
    if password.len() < 12 {
        Err("use at least 12 bytes".into())
    } else if password.len() > 128 {
        Err("use at most 128 bytes".into())
    } else {
        Ok(())
    }
}

pub fn validate_cloudflare_id(id: &str, label: &str) -> Result<()> {
    if id.len() != 32 || !id.chars().all(|character| character.is_ascii_hexdigit()) {
        bail!("Cloudflare {label} must contain exactly 32 hexadecimal characters");
    }
    Ok(())
}

pub fn validate_secret(secret: &str, label: &str) -> Result<()> {
    if secret.is_empty()
        || secret.len() > 4096
        || secret.chars().any(|character| character.is_whitespace())
    {
        bail!("{label} must be non-empty, contain no whitespace, and be at most 4096 bytes");
    }
    Ok(())
}

pub fn validate_public_base_url(value: &str) -> std::result::Result<(), String> {
    validate_base_url(value, false)
}

fn canonical_base_url(value: &str, allow_local: bool) -> std::result::Result<String, String> {
    validate_base_url(value, allow_local)?;
    let url = Url::parse(value).map_err(|_| "invalid base URL".to_string())?;
    Ok(url.origin().ascii_serialization())
}

fn validate_base_url(value: &str, allow_local: bool) -> std::result::Result<(), String> {
    let url = Url::parse(value)
        .map_err(|_| "enter an absolute URL such as https://hakobi.example.com".to_string())?;
    if url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("the base URL must not contain credentials, a path, query, or fragment".into());
    }
    if url.host_str().is_none() {
        return Err("the base URL must contain a hostname".into());
    }

    match url.scheme() {
        "https" if url.port().is_none() || url.port() == Some(443) => Ok(()),
        "http" if allow_local && is_loopback(&url) => Ok(()),
        "https" => Err("the HTTPS base URL must not use a custom port".into()),
        _ => Err("use an HTTPS base URL; HTTP is allowed only for local setup".into()),
    }
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(domain)) => domain == "localhost",
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

#[cfg(unix)]
fn reject_insecure_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)
        .with_context(|| format!("could not inspect {}", path.display()))?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        bail!(
            "{} is accessible by other users; run chmod 600 on it",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn reject_insecure_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloudflare() -> CloudflareConfig {
        CloudflareConfig {
            account_id: "a".repeat(32),
            zone_id: "b".repeat(32),
            tunnel_id: uuid::Uuid::nil().to_string(),
            tunnel_token: "tunnel-token".into(),
        }
    }

    #[test]
    fn public_urls_require_https_without_paths() {
        assert!(validate_public_base_url("https://hakobi.example.com").is_ok());
        assert!(validate_public_base_url("http://hakobi.example.com").is_err());
        assert!(validate_public_base_url("https://hakobi.example.com/admin").is_err());
        assert!(validate_public_base_url("https://user@hakobi.example.com").is_err());
    }

    #[test]
    fn local_urls_only_accept_loopback() {
        assert!(validate_base_url(&local_base_url(DEFAULT_PORT), true).is_ok());
        assert!(validate_base_url("http://192.168.1.2:3000", true).is_err());
    }

    #[test]
    fn configured_origins_are_canonicalized() {
        let config = Config::new(
            "admin".into(),
            "$argon2id$v=19$m=19456,t=2,p=1$abcdefghijklmnop$abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLM".into(),
            "https://hakobi.example.com:443/".into(),
            Some(cloudflare()),
        )
        .unwrap();
        assert_eq!(config.base_url, "https://hakobi.example.com");
    }

    #[test]
    fn concurrent_saves_never_replace_the_first_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().to_path_buf());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut handles = Vec::new();
        for username in ["first", "second"] {
            let store = store.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                let config = Config::new(
                    username.into(),
                    "$argon2id$v=19$m=19456,t=2,p=1$abcdefghijklmnop$abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLM".into(),
                    local_base_url(DEFAULT_PORT),
                    None,
                )
                .unwrap();
                barrier.wait();
                store.save_new(&config)
            }));
        }
        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert!(["first", "second"].contains(&store.load().unwrap().auth.username.as_str()));
    }

    #[test]
    fn saved_configuration_is_private_and_loadable() {
        let directory = tempfile::tempdir().unwrap();
        let store = ConfigStore::new(directory.path().to_path_buf());
        let config = Config::new(
            "admin".into(),
            "$argon2id$v=19$m=19456,t=2,p=1$abcdefghijklmnop$abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLM".into(),
            "https://hakobi.example.com".into(),
            Some(cloudflare()),
        )
        .unwrap();
        store.save_new(&config).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(store.load().unwrap().auth.username, "admin");
        assert!(store.save_new(&config).is_err());

        let mut updated = config.clone();
        updated.auth.username = "operator".into();
        store.replace(&updated).unwrap();
        assert_eq!(store.load().unwrap().auth.username, "operator");
    }
}
