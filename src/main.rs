#![forbid(unsafe_code)]

mod cli;
mod cloudflare;
mod config;
mod utils;
mod web;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("hakobi=info")),
        )
        .with_target(false)
        .init();

    if let Err(error) = run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let store = cli.store()?;

    match cli.command {
        Some(Command::Setup { local }) => cli::setup(&store, local).await,
        Some(Command::Run) => cli::serve(&store).await,
        Some(Command::Passwd) => cli::passwd(&store).await,
        None => {
            if !store.exists() {
                cli::setup(&store, false).await?;
            }
            cli::serve(&store).await
        }
    }
}
