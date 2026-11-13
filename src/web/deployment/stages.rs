//! Catalog plumbing shared by the deployment pipeline stages: status
//! transitions, log lines, and the versioned image tag scheme.

use anyhow::Result;

use super::{run_catalog_op, ExecutorDeps};

/// Deployment statuses as stored in the catalog.
pub(super) const STAGE_BUILDING: &str = "building";
pub(super) const STAGE_DEPLOYING: &str = "deploying";
pub(super) const STAGE_RUNNING: &str = "running";
pub(super) const STAGE_FAILED: &str = "failed";

/// Everything the build stage needs, resolved in one catalog pass.
pub(super) struct BuildContext {
    pub build_type: String,
    pub git_url: String,
    pub branch: String,
    pub dockerfile_path: Option<String>,
    pub context_dir: Option<String>,
    pub image_tag: String,
    /// Container port the app listens on (host port maps to this).
    pub container_port: u16,
    /// Raw JSON object of container environment variables.
    pub env_json: String,
}

/// Appends a log line to a deployment.
pub(super) async fn log(
    deps: &ExecutorDeps,
    deployment_id: &str,
    level: &str,
    message: &str,
) -> Result<()> {
    let path = deps.catalog_path.clone();
    let deployment_id = deployment_id.to_owned();
    let level = level.to_owned();
    let message = message.to_owned();
    run_catalog_op(path, move |catalog| {
        catalog.add_deployment_log(&deployment_id, &level, &message)?;
        Ok(())
    })
}

/// Records build runner output on the deployment record.
pub(super) async fn record_build_output(
    deps: &ExecutorDeps,
    deployment_id: &str,
    output: &str,
) -> Result<()> {
    let path = deps.catalog_path.clone();
    let deployment_id = deployment_id.to_owned();
    let output = output.to_owned();
    run_catalog_op(path, move |catalog| {
        catalog.update_deployment_status(&deployment_id, STAGE_BUILDING, Some(&output))?;
        Ok(())
    })
}

/// Builds the versioned image tag for an app deployment.
pub(crate) fn image_tag_for(app_name: &str, version: i64) -> String {
    format!("hakobi/{app_name}:v{version}")
}

/// Container name the pipeline manages on the target host.
pub(crate) fn container_name_for(app_id: &str) -> String {
    format!("hakobi-{app_id}")
}
