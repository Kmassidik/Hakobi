//! Deployment executor: build and deploy stages for one deployment.
//!
//! The state machine lives in `executor.rs`; this module holds the shared
//! types ([`ExecutorDeps`], [`DeploymentGuard`], [`Trigger`]) and the
//! catalog plumbing both stages use.

pub(crate) mod executor;
mod stages;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hakobi::buildrunner::BuildRunnerClient;
use hakobi::catalog::Catalog;
use hakobi::providers::ContainerBackend;

/// A deployment trigger decided before the executor runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Manual,
    Webhook,
}

impl Trigger {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Webhook => "webhook",
        }
    }
}

/// Everything the executor needs beyond the catalog path.
#[derive(Clone)]
pub(crate) struct ExecutorDeps {
    pub catalog_path: PathBuf,
    pub build_runner: Option<BuildRunnerClient>,
    pub backend: Arc<dyn ContainerBackend>,
}

/// Guard that allows only one active deployment per app.
///
/// Claims are inserted when a deployment is queued and removed when the
/// executor task finishes, success or failure. Clones share the same claim
/// set, so a spawned executor task can release its own claim.
#[derive(Clone, Default)]
pub struct DeploymentGuard {
    active: Arc<Mutex<HashSet<String>>>,
}

impl DeploymentGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Claims the app for a deployment. `false` means another
    /// deployment is already active.
    pub fn try_claim(&self, app_id: &str) -> bool {
        self.active
            .lock()
            .expect("deployment guard mutex poisoned")
            .insert(app_id.to_owned())
    }

    /// Releases the claim after the executor finishes.
    pub fn release(&self, app_id: &str) {
        self.active
            .lock()
            .expect("deployment guard mutex poisoned")
            .remove(app_id);
    }
}

/// Queues a deployment and spawns the executor task.
///
/// Creates the pending deployment record synchronously so the caller gets an
/// id to render, then hands the pipeline to a tokio task. Returns `None`
/// when the concurrent-deployment guard rejects the request.
pub(crate) fn start_deployment(
    deps: &ExecutorDeps,
    guard: &DeploymentGuard,
    app_id: &str,
    trigger: Trigger,
    commit_sha: Option<&str>,
    commit_message: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if !guard.try_claim(app_id) {
        return Ok(None);
    }
    let app = app_id.to_owned();
    let commit_sha = commit_sha
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let commit_message = commit_message
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    // `create_deployment` derives the next version inside one transaction;
    // the guard keeps this race-free per app.
    let deployment_id = run_catalog_op(deps.catalog_path.clone(), move |catalog| {
        let id = catalog.create_deployment(
            &app,
            trigger.as_str(),
            commit_sha.as_deref(),
            commit_message.as_deref(),
        )?;
        if let Some(message) = commit_message {
            catalog.add_deployment_log(&id, "info", &message)?;
        }
        Ok(id)
    })?;

    let deps = deps.clone();
    let guard = guard.clone();
    let app = app_id.to_owned();
    let pipeline_deployment = deployment_id.clone();
    tokio::spawn(async move {
        let outcome = executor::run_pipeline(&deps, &app, &pipeline_deployment).await;
        guard.release(&app);
        if let Err(error) = outcome {
            tracing::error!(
                error = %error,
                deployment = %pipeline_deployment,
                "deployment pipeline failed"
            );
        }
    });
    Ok(Some(deployment_id))
}

/// Runs a blocking catalog operation off the async runtime.
pub(crate) fn run_catalog_op<T, F>(path: PathBuf, operation: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Catalog) -> anyhow::Result<T> + Send + 'static,
{
    let result = std::thread::scope(|scope| {
        scope
            .spawn(move || {
                let mut catalog = Catalog::open(path)?;
                operation(&mut catalog)
            })
            .join()
            .map_err(|_| anyhow::anyhow!("catalog thread panicked"))?
    });
    result
}
