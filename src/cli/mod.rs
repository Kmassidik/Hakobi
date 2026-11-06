use std::{future::pending, path::PathBuf, process::ExitStatus};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input, Password};
use tokio::{net::TcpListener, task};
use zeroize::Zeroizing;

use crate::{
    cloudflare::{tunnel::TunnelProcess, CloudflareClient, CreatedTunnel},
    config::{
        listen_port, local_base_url, validate_cloudflare_id, validate_password,
        validate_public_base_url, validate_secret, validate_username, CloudflareConfig, Config,
        ConfigStore,
    },
    web::{self, auth},
};

#[derive(Debug, Parser)]
#[command(name = "hakobi", version, about = "Self-hosted DevOps control plane")]
pub struct Cli {
    /// Override the directory containing config.toml.
    #[arg(long, global = true, value_name = "PATH")]
    data_dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Configure credentials and, by default, a public Cloudflare Tunnel.
    Setup {
        /// Configure a loopback-only server without Cloudflare.
        #[arg(long)]
        local: bool,
    },
    /// Run an existing configuration.
    Run,
    /// Change the dashboard password.
    Passwd,
}

pub async fn passwd(store: &ConfigStore) -> Result<()> {
    let mut config = store.load()?;
    let password = Zeroizing::new(
        Password::new()
            .with_prompt("New password")
            .with_confirmation("Confirm new password", "Passwords do not match")
            .validate_with(|value: &String| validate_password(value))
            .interact()?,
    );
    let password_for_hash = password.clone();
    config.auth.password_hash =
        task::spawn_blocking(move || auth::hash_password(&password_for_hash))
            .await
            .context("password hashing task failed")??;
    store.replace(&config)?;
    println!("Password updated. Restart Hakobi to apply it and end active sessions.");
    Ok(())
}

impl Cli {
    pub fn store(&self) -> Result<ConfigStore> {
        ConfigStore::discover(self.data_dir.clone())
    }
}

pub async fn setup(store: &ConfigStore, local: bool) -> Result<()> {
    if store.exists() {
        bail!(
            "{} already exists; refusing to replace credentials or DNS settings",
            store.path().display()
        );
    }

    let listen_port = listen_port()?;
    println!("Hakobi first-run setup\n");
    let username: String = Input::new()
        .with_prompt("Username")
        .default("admin".into())
        .validate_with(|value: &String| validate_username(value))
        .interact_text()?;
    let password = Zeroizing::new(
        Password::new()
            .with_prompt("Password")
            .with_confirmation("Confirm password", "Passwords do not match")
            .validate_with(|value: &String| validate_password(value))
            .interact()?,
    );
    let password_for_hash = password.clone();
    let password_hash = task::spawn_blocking(move || auth::hash_password(&password_for_hash))
        .await
        .context("password hashing task failed")??;

    if local {
        let local_url = local_base_url(listen_port);
        let config = Config::new(username, password_hash, local_url.clone(), None)?;
        store.save_new(&config)?;
        println!("\nLocal setup complete. Open {local_url} after running `hakobi run`.");
        return Ok(());
    }

    let base_url: String = Input::new()
        .with_prompt("Public base URL (for example, https://hakobi.example.com)")
        .validate_with(|value: &String| validate_public_base_url(value))
        .interact_text()?;
    let account_id: String = Input::new()
        .with_prompt("Cloudflare account ID")
        .validate_with(|value: &String| {
            validate_cloudflare_id(value, "account ID").map_err(|error| error.to_string())
        })
        .interact_text()?;
    let zone_id: String = Input::new()
        .with_prompt("Cloudflare zone ID")
        .validate_with(|value: &String| {
            validate_cloudflare_id(value, "zone ID").map_err(|error| error.to_string())
        })
        .interact_text()?;
    let api_token = Zeroizing::new(
        Password::new()
            .with_prompt("Cloudflare API token")
            .validate_with(|value: &String| {
                validate_secret(value, "Cloudflare API token").map_err(|error| error.to_string())
            })
            .interact()?,
    );

    let config = Config::new(username, password_hash, base_url.clone(), None)?;
    let hostname = config.hostname()?.to_owned();
    println!("\nHakobi will create a tunnel and a proxied DNS record for {hostname}.");
    println!("An existing DNS record with the same name will not be overwritten.");
    if !Confirm::new()
        .with_prompt("Continue?")
        .default(true)
        .interact()?
    {
        bail!("setup cancelled");
    }

    let client = CloudflareClient::new(api_token.to_string())?;
    let tunnel_name = format!("hakobi-{hostname}");
    let tunnel = client.create_tunnel(&account_id, &tunnel_name).await?;
    let mut dns_record_id = None;

    let provisioning = async {
        client
            .configure_tunnel(&account_id, tunnel.id, &hostname, listen_port)
            .await?;
        let record = client
            .create_dns_record(&zone_id, &hostname, tunnel.id)
            .await?;
        dns_record_id = Some(record.id);
        Ok(())
    }
    .await;

    if let Err(error) = provisioning {
        rollback(
            &client,
            &account_id,
            &zone_id,
            &tunnel,
            &hostname,
            dns_record_id.as_deref(),
        )
        .await;
        return Err(error);
    }

    let config = Config::new(
        config.auth.username.clone(),
        config.auth.password_hash.clone(),
        base_url.clone(),
        Some(CloudflareConfig {
            account_id: account_id.clone(),
            zone_id: zone_id.clone(),
            tunnel_id: tunnel.id.to_string(),
            tunnel_token: tunnel.token.clone(),
        }),
    )?;
    if let Err(error) = store.save_new(&config) {
        rollback(
            &client,
            &account_id,
            &zone_id,
            &tunnel,
            &hostname,
            dns_record_id.as_deref(),
        )
        .await;
        return Err(error);
    }

    println!(
        "\nSetup complete. Hakobi will be available at {}.",
        config.base_url
    );
    println!("The Cloudflare API token was not saved.");
    Ok(())
}

async fn rollback(
    client: &CloudflareClient,
    account_id: &str,
    zone_id: &str,
    tunnel: &CreatedTunnel,
    hostname: &str,
    dns_record_id: Option<&str>,
) {
    let record_ids = match dns_record_id {
        Some(record_id) => vec![record_id.to_owned()],
        None => match client.find_dns_records(zone_id, hostname).await {
            Ok(records) => {
                let expected_content = format!("{}.cfargotunnel.com", tunnel.id);
                records
                    .into_iter()
                    .filter(|record| record.content == expected_content)
                    .map(|record| record.id)
                    .collect()
            }
            Err(error) => {
                eprintln!(
                    "warning: could not reconcile DNS after the failed setup; leaving the tunnel in place for manual cleanup: {error:#}"
                );
                return;
            }
        },
    };

    for record_id in record_ids {
        if let Err(error) = client.delete_dns_record(zone_id, &record_id).await {
            eprintln!(
                "warning: could not roll back the DNS record; leaving the tunnel in place for manual cleanup: {error:#}"
            );
            return;
        }
    }
    if let Err(error) = client.delete_tunnel(account_id, tunnel.id).await {
        eprintln!("warning: could not roll back the Cloudflare tunnel: {error:#}");
    }
}

pub async fn serve(store: &ConfigStore) -> Result<()> {
    let config = store.load()?;
    let public = config.public();
    let base_url = config.base_url.clone();
    let mut tunnel = match &config.cloudflare {
        Some(cloudflare) => Some(TunnelProcess::spawn(&cloudflare.tunnel_token)?),
        None => None,
    };

    let listen_addr = format!("127.0.0.1:{}", listen_port()?);
    let listener = TcpListener::bind(&listen_addr)
        .await
        .with_context(|| format!("could not bind {listen_addr}"))?;
    let app = web::router(public, store.data_dir().join("catalog.sqlite3"));
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = axum::serve(listener, app).with_graceful_shutdown(async {
        let _ = shutdown_rx.await;
    });
    let server = std::future::IntoFuture::into_future(server);
    tokio::pin!(server);

    println!("Hakobi is available at {base_url}");
    let outcome = tokio::select! {
        result = &mut server => result.context("HTTP server failed"),
        status = wait_for_tunnel(&mut tunnel) => {
            match status {
                Ok(status) => Err(anyhow::anyhow!("cloudflared exited unexpectedly with {status}")),
                Err(error) => Err(error.context("cloudflared failed")),
            }
        }
        _ = shutdown_signal() => {
            let _ = shutdown_tx.send(());
            (&mut server).await.context("HTTP server shutdown failed")
        }
    };

    if let Some(tunnel) = &mut tunnel {
        if let Err(error) = tunnel.terminate().await {
            eprintln!("warning: could not stop cloudflared cleanly: {error:#}");
        }
    }
    outcome
}

async fn wait_for_tunnel(tunnel: &mut Option<TunnelProcess>) -> Result<ExitStatus> {
    match tunnel {
        Some(tunnel) => tunnel.wait().await,
        None => pending().await,
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        } else {
            pending::<()>().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
