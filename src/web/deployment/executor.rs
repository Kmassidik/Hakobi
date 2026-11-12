//! The deployment state machine: pending → building → deploying → running,
//! with any failure marking the deployment failed.

use std::time::Duration;

use anyhow::{bail, Context, Result};

use hakobi::buildrunner::BuildRequest;
use hakobi::providers::docker::validate_image_reference;

use super::stages::{
    container_name_for, BuildContext, STAGE_BUILDING, STAGE_DEPLOYING, STAGE_FAILED, STAGE_RUNNING,
};
use super::{run_catalog_op, ExecutorDeps};

/// Pause after starting the container before the first state poll.
const CONTAINER_SETTLE: Duration = Duration::from_secs(2);

/// Budget for waiting until the new container reports a running state.
const CONTAINER_VERIFY_TIMEOUT: Duration = Duration::from_secs(30);

/// Executes the full pipeline for an existing pending deployment.
pub(super) async fn run_pipeline(
    deps: &ExecutorDeps,
    app_id: &str,
    deployment_id: &str,
) -> Result<()> {
    transition(deps, deployment_id, STAGE_BUILDING, "Build started").await?;
    let image_tag = match build_image(deps, app_id, deployment_id).await {
        Ok(tag) => tag,
        Err(error) => {
            return fail(deps, deployment_id, &format!("build failed: {error:#}")).await;
        }
    };
    super::stages::log(
        deps,
        deployment_id,
        "info",
        &format!("built image {image_tag}"),
    )
    .await?;

    transition(deps, deployment_id, STAGE_DEPLOYING, "Deploying to host").await?;
    let context = match load_build_context(deps, app_id, deployment_id).await {
        Ok(context) => context,
        Err(error) => {
            return fail(deps, deployment_id, &format!("deploy failed: {error:#}")).await;
        }
    };
    if let Err(error) = deploy_image(
        deps,
        app_id,
        deployment_id,
        &image_tag,
        context.container_port,
    )
    .await
    {
        return fail(deps, deployment_id, &format!("deploy failed: {error:#}")).await;
    }
    transition(deps, deployment_id, STAGE_RUNNING, "Deployment is running").await
}

/// Runs the build through the configured build runner and returns the image
/// tag (`hakobi/{app}:v{version}`).
async fn build_image(deps: &ExecutorDeps, app_id: &str, deployment_id: &str) -> Result<String> {
    let context = load_build_context(deps, app_id, deployment_id).await?;
    let runner = deps
        .build_runner
        .as_ref()
        .context("no build runner is configured; set HAKOBI_BUILDRUNNER_URL")?;
    let request = BuildRequest {
        git_url: context.git_url.clone(),
        branch: context.branch.clone(),
        image_tag: context.image_tag.clone(),
        dockerfile_path: context.dockerfile_path.clone(),
        context_dir: context.context_dir.clone(),
        token: None,
    };
    let response = match context.build_type.as_str() {
        "compose" => runner.build_compose(&request).await,
        "dockerfile" => runner.build_dockerfile(&request).await,
        "nixpacks" => runner.build_nixpacks(&request).await,
        other => bail!("app has unsupported build type {other:?}"),
    }
    .context("build runner request failed")?;
    let output = format!(
        "status={} exit={} duration_ms={}{}",
        response.status,
        response.exit_code,
        response.duration_ms,
        response
            .detail
            .as_deref()
            .map(|detail| format!(" detail={detail}"))
            .unwrap_or_default(),
    );
    super::stages::record_build_output(deps, deployment_id, &output).await?;
    if response.status != "succeeded" || response.exit_code != 0 {
        let detail = response.detail.unwrap_or_else(|| response.log_tail.clone());
        bail!(
            "runner reported {} (exit {}): {}",
            response.status,
            response.exit_code,
            truncate_message(&detail)
        );
    }
    Ok(context.image_tag)
}

/// Stops the old container, starts the built image, and verifies it runs.
async fn deploy_image(
    deps: &ExecutorDeps,
    app_id: &str,
    deployment_id: &str,
    image_tag: &str,
    container_port: u16,
) -> Result<()> {
    validate_image_reference(image_tag)?;
    let endpoint = load_deploy_endpoint(deps, app_id, deployment_id).await?;
    let container_name = container_name_for(app_id);
    if let Err(error) = deps
        .backend
        .stop_container(&endpoint, &container_name)
        .await
    {
        super::stages::log(
            deps,
            deployment_id,
            "warn",
            &format!(
                "no previous container to stop: {}",
                truncate_message(&error.to_string())
            ),
        )
        .await?;
    }
    if let Err(error) = deps
        .backend
        .remove_container(&endpoint, &container_name)
        .await
    {
        super::stages::log(
            deps,
            deployment_id,
            "warn",
            &format!(
                "previous container removal skipped: {}",
                truncate_message(&error.to_string())
            ),
        )
        .await?;
    }
    let run = deps
        .backend
        .run_container(&endpoint, image_tag, &container_name, container_port)
        .await
        .context("agent could not start the new container")?;
    super::stages::log(
        deps,
        deployment_id,
        "info",
        &format!(
            "container {} started on host port {}",
            run.name, run.host_port
        ),
    )
    .await?;
    verify_running(deps, &endpoint, &container_name, image_tag).await
}

/// Polls the backend until the container reports `running` with the deployed
/// image, or the verification budget elapses.
async fn verify_running(
    deps: &ExecutorDeps,
    endpoint: &str,
    container_name: &str,
    image_tag: &str,
) -> Result<()> {
    tokio::time::sleep(CONTAINER_SETTLE).await;
    let deadline = tokio::time::Instant::now() + CONTAINER_VERIFY_TIMEOUT;
    loop {
        let containers = deps
            .backend
            .containers(endpoint)
            .await
            .context("could not list containers on the target host")?;
        let now = tokio::time::Instant::now();
        match containers.iter().find(|row| row.name == container_name) {
            Some(row) if row.state == "running" && row.image == image_tag => return Ok(()),
            Some(row) if now >= deadline => {
                bail!(
                    "container {container_name} did not reach running state (state: {})",
                    row.state
                );
            }
            Some(_) => {}
            None if now >= deadline => bail!("container {container_name} disappeared after start"),
            None => {}
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Marks a deployment failed and records the reason.
async fn fail(deps: &ExecutorDeps, deployment_id: &str, message: &str) -> Result<()> {
    let path = deps.catalog_path.clone();
    let deployment_id = deployment_id.to_owned();
    let message = message.to_owned();
    run_catalog_op(path, move |catalog| {
        catalog.update_deployment_status(&deployment_id, STAGE_FAILED, None)?;
        catalog.add_deployment_log(&deployment_id, "error", &message)?;
        Ok(())
    })
}

/// Transitions a deployment to `status` with a log line.
async fn transition(
    deps: &ExecutorDeps,
    deployment_id: &str,
    status: &str,
    message: &str,
) -> Result<()> {
    let path = deps.catalog_path.clone();
    let deployment_id = deployment_id.to_owned();
    let status = status.to_owned();
    let message = message.to_owned();
    run_catalog_op(path, move |catalog| {
        catalog.update_deployment_status(&deployment_id, &status, None)?;
        catalog.add_deployment_log(&deployment_id, "info", &message)?;
        Ok(())
    })
}

/// Reads everything the build stage needs in one catalog pass.
async fn load_build_context(
    deps: &ExecutorDeps,
    app_id: &str,
    deployment_id: &str,
) -> Result<BuildContext> {
    let path = deps.catalog_path.clone();
    let app_id = app_id.to_owned();
    let deployment_id = deployment_id.to_owned();
    run_catalog_op(path, move |catalog| {
        let app = catalog
            .get_app(&app_id)?
            .context("app disappeared mid-deployment")?;
        let deployment = catalog
            .get_deployment(&deployment_id)?
            .context("deployment record disappeared mid-deployment")?;
        let git_url = app
            .github_repo
            .as_deref()
            .map(|repo| format!("https://github.com/{repo}.git"))
            .filter(|url| !url.trim().is_empty())
            .context("app has no GitHub repository configured")?;
        let build_type = app.build_type.clone();
        let is_compose = build_type == "compose";
        Ok(BuildContext {
            container_port: app.container_port.unwrap_or(8080),
            build_type,
            branch: app
                .github_branch
                .filter(|branch| !branch.trim().is_empty())
                .unwrap_or_else(|| "main".to_owned()),
            dockerfile_path: app
                .github_dockerfile
                .filter(|value| !value.trim().is_empty()),
            context_dir: is_compose.then(|| ".".to_owned()),
            image_tag: super::stages::image_tag_for(&app.name, deployment.version),
            git_url,
        })
    })
}

/// Resolves the `agent://{host_id}` endpoint for the app.s host.
async fn load_deploy_endpoint(
    deps: &ExecutorDeps,
    app_id: &str,
    deployment_id: &str,
) -> Result<String> {
    let path = deps.catalog_path.clone();
    let app_id = app_id.to_owned();
    let deployment_id = deployment_id.to_owned();
    run_catalog_op(path, move |catalog| {
        let app = catalog
            .get_app(&app_id)?
            .context("app disappeared mid-deployment")?;
        catalog
            .get_deployment(&deployment_id)?
            .context("deployment record disappeared mid-deployment")?;
        Ok(format!("agent://{}", app.host_id))
    })
}

/// Truncates an error message before it goes into a log line.
pub(super) fn truncate_message(message: &str) -> String {
    let cleaned: String = message.chars().filter(|c| !c.is_control()).collect();
    let mut result: String = cleaned.chars().take(400).collect();
    if cleaned.chars().count() > 400 {
        result.push('…');
    }
    result
}
