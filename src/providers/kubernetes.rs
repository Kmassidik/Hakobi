//! Kubernetes provider backed by the `kubectl` binary.
//!
//! Every cluster operation is a direct `kubectl` invocation (argv only, never a
//! shell) carrying an explicit `--kubeconfig` and `--context`, so the caller's
//! ambient kubectl configuration is never consulted. Kubeconfig bytes are never
//! read, logged, or copied outside the file itself; only its path is passed on.

mod command;
mod rows;
#[cfg(test)]
mod tests;

pub use rows::{ConnectionInfo, DeploymentRow, NamespaceRow, PodRow, ServicePort, ServiceRow};

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use tokio::process::Command;

use command::{
    apply_args, delete_args, get_args, manifest_file, run_command, validate_context,
    validate_kubeconfig, validate_namespace, validate_segment, version_args, APPLY_TIMEOUT,
    QUERY_TIMEOUT,
};
use rows::{
    connection_info, deployments_from_json, namespaces_from_json, pods_from_json,
    services_from_json,
};

/// A kubeconfig + context pair identifying exactly one cluster.
#[derive(Debug, Clone)]
pub struct KubernetesProvider {
    kubeconfig: PathBuf,
    context: String,
}

impl KubernetesProvider {
    /// Builds a provider after verifying the kubeconfig is a readable file and
    /// the context is plausible. No cluster contact happens here.
    pub fn new(kubeconfig: impl Into<PathBuf>, context: impl Into<String>) -> Result<Self> {
        let provider = Self {
            kubeconfig: kubeconfig.into(),
            context: context.into(),
        };
        provider.validate()?;
        Ok(provider)
    }

    pub fn kubeconfig(&self) -> &Path {
        &self.kubeconfig
    }

    pub fn context(&self) -> &str {
        &self.context
    }

    /// Validates the stored connection settings again, useful before a long
    /// running operation in case the kubeconfig was removed meanwhile.
    pub fn validate(&self) -> Result<()> {
        validate_context(&self.context)?;
        validate_kubeconfig(&self.kubeconfig)
    }

    /// Runs `kubectl version --output=json` and reports client/server versions.
    pub async fn test_connection(&self) -> Result<ConnectionInfo> {
        let stdout = self.run("version", version_args()).await?;
        connection_info(&stdout, &self.context)
    }

    pub async fn list_namespaces(&self) -> Result<Vec<NamespaceRow>> {
        let stdout = self
            .run("get namespaces", get_args("namespaces", None))
            .await?;
        namespaces_from_json(&stdout)
    }

    pub async fn list_deployments(&self, namespace: &str) -> Result<Vec<DeploymentRow>> {
        validate_namespace(namespace)?;
        let stdout = self
            .run("get deployments", get_args("deployments", Some(namespace)))
            .await?;
        deployments_from_json(&stdout)
    }

    pub async fn list_services(&self, namespace: &str) -> Result<Vec<ServiceRow>> {
        validate_namespace(namespace)?;
        let stdout = self
            .run("get services", get_args("services", Some(namespace)))
            .await?;
        services_from_json(&stdout)
    }

    pub async fn list_pods(&self, namespace: &str) -> Result<Vec<PodRow>> {
        validate_namespace(namespace)?;
        let stdout = self
            .run("get pods", get_args("pods", Some(namespace)))
            .await?;
        pods_from_json(&stdout)
    }

    /// Applies a manifest bundle. The YAML is written to a private (0600)
    /// temporary file which is removed once kubectl exits.
    pub async fn apply(&self, namespace: &str, yaml: &str) -> Result<()> {
        validate_namespace(namespace)?;
        if yaml.trim().is_empty() {
            bail!("manifest must not be empty");
        }
        let manifest = manifest_file(yaml)?;
        let args = apply_args(namespace, manifest.path());
        let mut command = self.command();
        command.args(&args);
        run_command(command, "apply", APPLY_TIMEOUT).await?;
        Ok(())
    }

    /// Deletes one named resource of an explicit kind inside a namespace.
    pub async fn delete(&self, namespace: &str, resource: &str, name: &str) -> Result<()> {
        validate_namespace(namespace)?;
        validate_segment("resource", resource)?;
        validate_segment("name", name)?;
        let label = format!("delete {resource}");
        self.run(&label, delete_args(namespace, resource, name))
            .await?;
        Ok(())
    }

    fn command(&self) -> Command {
        command::command(&self.kubeconfig, &self.context)
    }

    async fn run(&self, label: &str, args: Vec<std::ffi::OsString>) -> Result<Vec<u8>> {
        let mut command = self.command();
        command.args(&args);
        run_command(command, label, QUERY_TIMEOUT).await
    }
}
