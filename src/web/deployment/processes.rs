use std::collections::HashMap;

use anyhow::{bail, Result};
use hakobi::catalog::worker_container_name;

use super::stages::{log, BuildContext};
use super::ExecutorDeps;

pub(super) async fn prepare_volumes(
    deps: &ExecutorDeps,
    context: &BuildContext,
    app_id: &str,
) -> Result<()> {
    for volume in &context.volumes {
        deps.backend
            .create_volume(&context.deploy_endpoint, &volume.name, Some(app_id))
            .await?;
    }
    Ok(())
}

pub(super) async fn run_release(
    deps: &ExecutorDeps,
    deployment_id: &str,
    context: &BuildContext,
    image_tag: &str,
) -> Result<()> {
    let Some(command) = context.release_command.as_deref() else {
        return Ok(());
    };
    log(deps, deployment_id, "info", "Running release command").await?;
    let name = format!("hakobi-release-{deployment_id}");
    let result = deps
        .backend
        .release_container(
            &context.deploy_endpoint,
            image_tag,
            &name,
            &context.environment,
            &context.volumes,
            command,
        )
        .await?;
    write_release_logs(deps, deployment_id, &result.logs, &context.environment).await?;
    if result.exit_code != 0 {
        bail!("release command exited with status {}", result.exit_code);
    }
    Ok(())
}

async fn write_release_logs(
    deps: &ExecutorDeps,
    deployment_id: &str,
    output: &str,
    environment: &HashMap<String, String>,
) -> Result<()> {
    let mut redacted = output.to_owned();
    for value in environment.values().filter(|value| !value.is_empty()) {
        redacted = redacted.replace(value, "[redacted]");
    }
    for line in redacted.lines() {
        log(deps, deployment_id, "info", &format!("[release] {line}")).await?;
    }
    Ok(())
}

async fn clear_stale_workers(
    deps: &ExecutorDeps,
    deployment_id: &str,
    app_id: &str,
    context: &BuildContext,
) -> Result<()> {
    let prefix = format!("hakobi-{app_id}-worker-");
    let existing = deps.backend.containers(&context.deploy_endpoint).await?;
    for container in existing.into_iter().filter(|c| c.name.starts_with(&prefix)) {
        super::executor::clear_previous_container(
            deps,
            deployment_id,
            &context.deploy_endpoint,
            &container.name,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn deploy_workers(
    deps: &ExecutorDeps,
    deployment_id: &str,
    app_id: &str,
    context: &BuildContext,
    image_tag: &str,
) -> Result<()> {
    clear_stale_workers(deps, deployment_id, app_id, context).await?;
    for process in &context.processes {
        let name = worker_container_name(app_id, &process.name);
        super::executor::clear_previous_container(
            deps,
            deployment_id,
            &context.deploy_endpoint,
            &name,
        )
        .await?;
        deps.backend
            .run_container(
                &context.deploy_endpoint,
                image_tag,
                &name,
                None,
                None,
                &context.environment,
                context.resources,
                Some(&process.command),
                &context.volumes,
            )
            .await?;
        super::executor::verify_running(deps, &context.deploy_endpoint, &name, image_tag).await?;
        log(
            deps,
            deployment_id,
            "info",
            &format!("worker {} is running", process.name),
        )
        .await?;
    }
    Ok(())
}
