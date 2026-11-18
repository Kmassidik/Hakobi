use std::{path::Path, time::Instant};

use anyhow::{bail, Result};
use tokio::sync::mpsc::UnboundedSender;

use super::{
    encode_log_event,
    process::{run_capture, sanitize, LogTail},
    BuildJob, BuildOutcome,
};

pub(super) struct BuildLogger<'a> {
    sender: Option<&'a UnboundedSender<String>>,
    token: Option<&'a str>,
    tail: LogTail,
}

impl<'a> BuildLogger<'a> {
    fn new(sender: Option<&'a UnboundedSender<String>>, token: Option<&'a str>) -> Self {
        Self {
            sender,
            token,
            tail: LogTail::default(),
        }
    }

    fn emit(&mut self, stage: &str, message: &str) {
        if message.is_empty() {
            return;
        }
        self.tail.push(message);
        if let Some(sender) = self.sender {
            let _ = sender.send(encode_log_event(stage, message));
        }
    }

    fn emit_unfiltered(&mut self, stage: &str, message: &str) {
        let message = sanitize(message, self.token);
        self.emit(stage, &message);
    }

    fn outcome(
        self,
        status: &str,
        exit_code: i32,
        started: Instant,
        detail: Option<String>,
    ) -> BuildOutcome {
        let (log_tail, truncated) = self.tail.finish();
        BuildOutcome {
            status: status.to_owned(),
            exit_code,
            duration_ms: started.elapsed().as_millis() as u64,
            log_tail,
            truncated,
            detail,
        }
    }
}

pub(super) fn run(
    runs_root: &Path,
    job: &BuildJob,
    sender: Option<&UnboundedSender<String>>,
) -> BuildOutcome {
    let started = Instant::now();
    let mut logs = BuildLogger::new(sender, job.token.as_deref());
    if let Err(error) = validate_inputs(job) {
        logs.emit_unfiltered("runner", &format!("{error:#}\n"));
        return logs.outcome("failed", 1, started, Some(error.to_string()));
    }
    let workdir = super::workspace_dir(runs_root, &job.git_url, &job.branch);
    if let Err(error) = reset_workspace(&workdir) {
        logs.emit_unfiltered("runner", &format!("{error}\n"));
        return logs.outcome("failed", 1, started, Some(error.to_string()));
    }
    execute(&workdir, job, started, logs)
}

fn execute(
    workdir: &Path,
    job: &BuildJob,
    started: Instant,
    mut logs: BuildLogger<'_>,
) -> BuildOutcome {
    let code = clone_repository(workdir, job, &mut logs);
    if code != 0 {
        return logs.outcome("failed", code, started, Some("git clone failed".to_owned()));
    }
    let code = build_image(workdir, job, &mut logs);
    if code != 0 {
        return logs.outcome(
            "failed",
            code,
            started,
            Some("docker build failed".to_owned()),
        );
    }
    logs.outcome("succeeded", 0, started, None)
}

fn validate_inputs(job: &BuildJob) -> Result<()> {
    if !job.git_url.starts_with("https://") {
        bail!("git_url must be an https URL");
    }
    if job.branch.is_empty() || job.branch.len() > 200 {
        bail!("branch must be 1-200 characters");
    }
    if job.image_tag.is_empty() || job.image_tag.contains(char::is_whitespace) {
        bail!("image_tag must be a non-empty docker tag");
    }
    for path in [job.dockerfile_path.as_deref(), job.context_dir.as_deref()]
        .into_iter()
        .flatten()
    {
        if path.starts_with('/') || path.split('/').any(|part| part == "..") {
            bail!("repository paths must be relative");
        }
    }
    Ok(())
}

fn reset_workspace(workdir: &Path) -> std::io::Result<()> {
    // Reset any previous checkout so the clone is deterministic.
    let _ = std::fs::remove_dir_all(workdir);
    std::fs::create_dir_all(workdir)
}

fn clone_repository(workdir: &Path, job: &BuildJob, logs: &mut BuildLogger<'_>) -> i32 {
    // Private repos: embed the token as a basic-auth user (git accepts
    // x-access-token:<token> for GitHub App installation tokens).
    let clone_url = job
        .token
        .as_deref()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| {
            job.git_url
                .replacen("https://", &format!("https://x-access-token:{token}@"), 1)
        })
        .unwrap_or_else(|| job.git_url.clone());
    let args = [
        "clone",
        "--depth",
        "1",
        "--branch",
        &job.branch,
        &clone_url,
        "repo",
    ];
    run_capture(
        workdir,
        "git",
        &args,
        job.token.as_deref(),
        &mut |message| {
            logs.emit("clone", message);
        },
    )
}

fn build_image(workdir: &Path, job: &BuildJob, logs: &mut BuildLogger<'_>) -> i32 {
    let repo = workdir.join("repo");
    let dockerfile = job.dockerfile_path.as_deref().unwrap_or("Dockerfile");
    let context = job.context_dir.as_deref().unwrap_or(".");
    let context_path = repo.join(context);
    let args = ["build", "-f", dockerfile, "-t", &job.image_tag, "."];
    run_capture(
        &context_path,
        "docker",
        &args,
        job.token.as_deref(),
        &mut |message| logs.emit("build", message),
    )
}
