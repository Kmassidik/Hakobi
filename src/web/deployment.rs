//! Deployment executor: build and deploy stages for one deployment.
//!
//! The state machine lives in `executor.rs`; this module holds the shared
//! types ([`ExecutorDeps`], [`DeploymentGuard`], [`Trigger`]) and the
//! catalog plumbing both stages use.

pub(crate) mod executor;
mod processes;
pub(crate) mod stages;
pub(crate) use stages::{container_name_for, image_tag_for};

use std::collections::{HashMap, HashSet};
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
    /// Serializes each deployment's cancellation with its container start.
    cancellation: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
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

    /// Prevents a container start from racing a successful cancellation.
    pub(crate) async fn lock_cancellation(
        &self,
        deployment_id: &str,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = self
            .cancellation
            .lock()
            .expect("cancellation mutex poisoned")
            .entry(deployment_id.to_owned())
            .or_default()
            .clone();
        lock.lock_owned().await
    }

    /// Releases the claim after the executor finishes.
    pub fn release(&self, app_id: &str) {
        self.active
            .lock()
            .expect("deployment guard mutex poisoned")
            .remove(app_id);
    }

    fn finish(&self, app_id: &str, deployment_id: &str) {
        self.release(app_id);
        self.cancellation
            .lock()
            .expect("cancellation mutex poisoned")
            .remove(deployment_id);
    }
}

fn normalize_commit_field(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
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
    let commit_sha = normalize_commit_field(commit_sha);
    let commit_message = normalize_commit_field(commit_message);
    // `create_deployment` derives the next version inside one transaction;
    // the guard keeps this race-free per app.
    let created = run_catalog_op(deps.catalog_path.clone(), move |catalog| {
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
    });
    let deployment_id = match created {
        Ok(id) => id,
        Err(error) => {
            guard.release(app_id);
            return Err(error);
        }
    };

    spawn_pipeline(
        deps.clone(),
        guard.clone(),
        app_id.to_owned(),
        deployment_id.clone(),
    );
    Ok(Some(deployment_id))
}

fn spawn_pipeline(deps: ExecutorDeps, guard: DeploymentGuard, app: String, deployment_id: String) {
    tokio::spawn(async move {
        let run_guard = guard.clone();
        let outcome = executor::run_pipeline(&deps, &run_guard, &app, &deployment_id).await;
        guard.finish(&app, &deployment_id);
        if let Err(error) = outcome {
            tracing::error!(
                error = %error,
                deployment = %deployment_id,
                "deployment pipeline failed"
            );
        }
    });
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
