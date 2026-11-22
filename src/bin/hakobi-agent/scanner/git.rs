use std::{
    collections::BTreeSet,
    path::Path,
    process::{ExitStatus, Stdio},
};

use anyhow::{bail, ensure, Context, Result};
use hakobi::buildrunner::validate_branch;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

const MAX_REFS: usize = 50;
const MAX_STDOUT_BYTES: usize = 1024 * 1024;
const MAX_STDERR_BYTES: usize = 16 * 1024;
const MAX_ERROR_CHARS: usize = 500;

pub(super) struct DiscoveredRefs {
    pub default_branch: String,
    pub branches: Vec<String>,
    pub tags: Vec<String>,
}

pub(super) async fn discover(url: &str, cwd: &Path) -> Result<DiscoveredRefs> {
    let output = run_git(cwd, |command| {
        command.args([
            "ls-remote",
            "--symref",
            url,
            "HEAD",
            "refs/heads/*",
            "refs/tags/*",
        ]);
    })
    .await
    .context("list public repository refs")?;
    parse_refs(&output.stdout)
}

pub(super) async fn clone(
    url: &str,
    selected_ref: &str,
    checkout: &Path,
    cwd: &Path,
) -> Result<()> {
    run_git(cwd, |command| {
        command.args([
            "clone",
            "--quiet",
            "--depth",
            "1",
            "--no-tags",
            "--single-branch",
            "--branch",
            selected_ref,
            "--",
            url,
        ]);
        command.arg(checkout);
    })
    .await
    .context("shallow clone public repository")?;
    Ok(())
}

struct Output {
    stdout: String,
}

async fn run_git(cwd: &Path, configure: impl FnOnce(&mut Command)) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .current_dir(cwd)
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(["-c", "credential.helper="]);
    isolated_environment(&mut command, cwd);
    configure(&mut command);
    let mut child = command.spawn().context("start git")?;
    let stdout = child.stdout.take().context("capture git stdout")?;
    let stderr = child.stderr.take().context("capture git stderr")?;
    let (stdout, stderr, status) = tokio::try_join!(
        read_bounded(stdout, MAX_STDOUT_BYTES),
        read_bounded(stderr, MAX_STDERR_BYTES),
        async { Ok::<_, anyhow::Error>(child.wait().await?) },
    )?;
    check_status(status, &stderr)?;
    let stdout = String::from_utf8(stdout).context("git output is not UTF-8")?;
    Ok(Output { stdout })
}

fn isolated_environment(command: &mut Command, cwd: &Path) {
    command.env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    command
        .env("HOME", cwd)
        .env("TMPDIR", cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "false")
        .env("SSH_ASKPASS", "false")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null");
}

async fn read_bounded(reader: impl AsyncRead + Unpin, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await?;
    ensure!(bytes.len() <= limit, "git output exceeded {limit} bytes");
    Ok(bytes)
}

fn check_status(status: ExitStatus, stderr: &[u8]) -> Result<()> {
    if status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(stderr);
    let message: String = message.trim().chars().take(MAX_ERROR_CHARS).collect();
    if message.is_empty() {
        bail!("git exited with {status}");
    }
    bail!("git exited with {status}: {message}")
}

fn parse_refs(output: &str) -> Result<DiscoveredRefs> {
    let mut default_branch = None;
    let mut branches = BTreeSet::new();
    let mut tags = BTreeSet::new();
    for line in output.lines() {
        if let Some(name) = line.strip_prefix("ref: refs/heads/") {
            if let Some(name) = name.strip_suffix("\tHEAD") {
                default_branch = Some(name.to_owned());
            }
        } else if let Some((_, name)) = line.split_once('\t') {
            collect_ref(name, &mut branches, &mut tags);
        }
    }
    let default_branch = default_branch.context("repository did not advertise a default branch")?;
    validate_branch(&default_branch)?;
    Ok(DiscoveredRefs {
        default_branch,
        branches: branches.into_iter().take(MAX_REFS).collect(),
        tags: tags.into_iter().take(MAX_REFS).collect(),
    })
}

fn collect_ref(name: &str, branches: &mut BTreeSet<String>, tags: &mut BTreeSet<String>) {
    let (name, target) = match name.strip_prefix("refs/heads/") {
        Some(name) => (name, branches),
        None => match name.strip_prefix("refs/tags/") {
            Some(name) if !name.ends_with("^{}") => (name, tags),
            _ => return,
        },
    };
    if validate_branch(name).is_ok() {
        target.insert(name.to_owned());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn refs_include_symref_head_sorted_and_peeled_tags_removed() {
        let refs = super::parse_refs(concat!(
            "ref: refs/heads/main\tHEAD\n",
            "abc\tHEAD\n",
            "abc\trefs/heads/zeta\n",
            "abc\trefs/heads/main\n",
            "def\trefs/tags/v2\n",
            "def\trefs/tags/v2^{}\n",
            "ghi\trefs/tags/v1\n",
        ))
        .unwrap();
        assert_eq!(refs.default_branch, "main");
        assert_eq!(refs.branches, ["main", "zeta"]);
        assert_eq!(refs.tags, ["v1", "v2"]);
    }

    #[test]
    fn refs_are_capped_and_default_branch_is_required() {
        let mut listing = "ref: refs/heads/main\tHEAD\n".to_owned();
        for index in 0..60 {
            listing.push_str(&format!("abc\trefs/heads/b{index:02}\n"));
        }
        let refs = super::parse_refs(&listing).unwrap();
        assert_eq!(refs.branches.len(), 50);
        assert!(super::parse_refs("abc\trefs/heads/main\n").is_err());
    }
}
