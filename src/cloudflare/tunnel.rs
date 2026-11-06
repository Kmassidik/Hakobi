use std::{process::ExitStatus, time::Duration};

use anyhow::{Context, Result};
use tokio::{
    process::{Child, Command},
    time::timeout,
};

pub struct TunnelProcess {
    child: Child,
}

impl TunnelProcess {
    pub fn spawn(token: &str) -> Result<Self> {
        let child = Command::new("cloudflared")
            .args(["tunnel", "--no-autoupdate", "run"])
            .env("TUNNEL_TOKEN", token)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .context("could not start cloudflared; install it or use the Nix/Docker environment")?;
        Ok(Self { child })
    }

    pub async fn wait(&mut self) -> Result<ExitStatus> {
        Ok(self.child.wait().await?)
    }

    pub async fn terminate(&mut self) -> Result<()> {
        if self.child.id().is_none() {
            return Ok(());
        }
        self.child.start_kill()?;
        timeout(Duration::from_secs(10), self.child.wait())
            .await
            .context("cloudflared did not stop within 10 seconds")??;
        Ok(())
    }
}
