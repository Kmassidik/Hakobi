pub mod apps_route;
pub mod tunnel;

use std::time::Duration;

use anyhow::{bail, Context, Result};
use http::{Method, StatusCode};
use reqwest::{Client, RequestBuilder};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;
use zeroize::Zeroize;

const API_ROOT: &str = "https://api.cloudflare.com/client/v4";

#[derive(Clone)]
pub struct CloudflareClient {
    client: Client,
    api_token: String,
}

#[derive(Debug)]
pub struct CreatedTunnel {
    pub id: Uuid,
    pub token: String,
}

impl Drop for CloudflareClient {
    fn drop(&mut self) {
        self.api_token.zeroize();
    }
}

impl Drop for CreatedTunnel {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

/// One ingress rule in a remotely-managed tunnel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressRule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    pub service: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_request: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct TunnelConfig {
    #[serde(default)]
    config: Option<Config>,
}

#[derive(Debug, Deserialize)]
struct Config {
    #[serde(default)]
    ingress: Vec<IngressRule>,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    success: bool,
    #[serde(default)]
    errors: Vec<ApiError>,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    code: u64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct TunnelResponse {
    id: Uuid,
    token: String,
}

#[derive(Debug, Deserialize)]
pub struct DnsRecord {
    pub id: String,
    pub content: String,
}

impl CloudflareClient {
    pub fn new(api_token: String) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("hakobi/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { client, api_token })
    }

    pub async fn create_tunnel(&self, account_id: &str, name: &str) -> Result<CreatedTunnel> {
        #[derive(Serialize)]
        struct CreateTunnel<'a> {
            name: &'a str,
            config_src: &'static str,
        }

        let response: TunnelResponse = self
            .send(
                self.request(Method::POST, &format!("/accounts/{account_id}/cfd_tunnel"))
                    .json(&CreateTunnel {
                        name,
                        config_src: "cloudflare",
                    }),
            )
            .await?;
        Ok(CreatedTunnel {
            id: response.id,
            token: response.token,
        })
    }

    pub async fn configure_tunnel(
        &self,
        account_id: &str,
        tunnel_id: Uuid,
        hostname: &str,
        listen_port: u16,
    ) -> Result<()> {
        let origin = format!("http://127.0.0.1:{listen_port}");
        let body = json!({
            "config": {
                "ingress": [
                    {
                        "hostname": hostname,
                        "service": origin,
                        "originRequest": {}
                    },
                    { "service": "http_status:404" }
                ]
            }
        });
        self.send::<Value>(
            self.request(
                Method::PUT,
                &format!("/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations"),
            )
            .json(&body),
        )
        .await?;
        Ok(())
    }

    pub async fn create_dns_record(
        &self,
        zone_id: &str,
        hostname: &str,
        tunnel_id: Uuid,
    ) -> Result<DnsRecord> {
        #[derive(Serialize)]
        #[serde(rename_all = "UPPERCASE")]
        enum RecordType {
            Cname,
        }

        #[derive(Serialize)]
        struct CreateRecord<'a> {
            r#type: RecordType,
            name: &'a str,
            content: String,
            proxied: bool,
            comment: &'static str,
        }

        self.send(
            self.request(Method::POST, &format!("/zones/{zone_id}/dns_records"))
                .json(&CreateRecord {
                    r#type: RecordType::Cname,
                    name: hostname,
                    content: format!("{tunnel_id}.cfargotunnel.com"),
                    proxied: true,
                    comment: "Managed by Hakobi",
                }),
        )
        .await
    }

    pub async fn find_dns_records(&self, zone_id: &str, hostname: &str) -> Result<Vec<DnsRecord>> {
        self.send(
            self.request(Method::GET, &format!("/zones/{zone_id}/dns_records"))
                .query(&[("name", hostname)]),
        )
        .await
    }

    pub async fn delete_dns_record(&self, zone_id: &str, record_id: &str) -> Result<()> {
        self.send::<Value>(self.request(
            Method::DELETE,
            &format!("/zones/{zone_id}/dns_records/{record_id}"),
        ))
        .await?;
        Ok(())
    }

    pub async fn delete_tunnel(&self, account_id: &str, tunnel_id: Uuid) -> Result<()> {
        self.send::<Value>(self.request(
            Method::DELETE,
            &format!("/accounts/{account_id}/cfd_tunnel/{tunnel_id}"),
        ))
        .await?;
        Ok(())
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(method, format!("{API_ROOT}{path}"))
            .bearer_auth(&self.api_token)
    }

    async fn send_raw(&self, request: RequestBuilder) -> Result<(StatusCode, String)> {
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await?;
        Ok((status, text))
    }

    async fn send<T: DeserializeOwned>(&self, request: RequestBuilder) -> Result<T> {
        let response = request
            .send()
            .await
            .context("Cloudflare API request failed")?;
        let status = response.status();
        let envelope: Envelope<T> = response
            .json()
            .await
            .context("Cloudflare API returned an invalid response")?;
        if status != StatusCode::OK && !status.is_success() || !envelope.success {
            let details = envelope
                .errors
                .iter()
                .map(|error| format!("{} ({})", error.message, error.code))
                .collect::<Vec<_>>()
                .join("; ");
            if details.is_empty() {
                bail!("Cloudflare API returned HTTP {status}");
            }
            bail!("Cloudflare API returned HTTP {status}: {details}");
        }
        envelope.result.context("Cloudflare API returned no result")
    }

    /// Fetches the tunnel's current ingress rules.
    pub async fn get_tunnel_ingress(
        &self,
        account_id: &str,
        tunnel_id: Uuid,
    ) -> Result<Vec<IngressRule>> {
        let response: TunnelConfig = self
            .send(self.request(
                Method::GET,
                &format!("/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations"),
            ))
            .await?;
        Ok(response
            .config
            .map(|config| config.ingress)
            .unwrap_or_default())
    }

    /// Adds (or replaces) an ingress rule mapping `hostname` to the given
    /// host port, preserving all other rules and the catch-all.
    pub async fn upsert_ingress_route(
        &self,
        account_id: &str,
        tunnel_id: Uuid,
        hostname: &str,
        listen_port: u16,
    ) -> Result<()> {
        let mut rules = self.get_tunnel_ingress(account_id, tunnel_id).await?;
        let service = format!("http://127.0.0.1:{listen_port}");
        if let Some(existing) = rules
            .iter_mut()
            .find(|rule| rule.hostname.as_deref() == Some(hostname))
        {
            existing.service = service;
        } else {
            // Insert before the catch-all (last rule).
            let insert_at = rules.len().saturating_sub(1);
            rules.insert(
                insert_at,
                IngressRule {
                    hostname: Some(hostname.to_owned()),
                    service,
                    origin_request: None,
                },
            );
        }
        self.put_ingress(account_id, tunnel_id, rules).await
    }

    /// Removes the ingress rule for `hostname` if present.
    pub async fn remove_ingress_route(
        &self,
        account_id: &str,
        tunnel_id: Uuid,
        hostname: &str,
    ) -> Result<()> {
        let mut rules = self.get_tunnel_ingress(account_id, tunnel_id).await?;
        let before = rules.len();
        rules.retain(|rule| rule.hostname.as_deref() != Some(hostname));
        if rules.len() == before {
            return Ok(());
        }
        self.put_ingress(account_id, tunnel_id, rules).await
    }

    async fn put_ingress(
        &self,
        account_id: &str,
        tunnel_id: Uuid,
        rules: Vec<IngressRule>,
    ) -> Result<()> {
        let body = json!({ "config": { "ingress": rules } });
        let (status, text) = self
            .send_raw(
                self.request(
                    Method::PUT,
                    &format!("/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations"),
                )
                .json(&body),
            )
            .await?;
        if !status.is_success() {
            bail!("tunnel ingress PUT failed: {status} {text}");
        }
        Ok(())
    }
}
