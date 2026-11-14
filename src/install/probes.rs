use std::{
    env,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use super::detection::DetectConfig;

/// Poll interval while waiting for a probe process to exit.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// Extra time allowed for draining a probe's stdout after it exits.
const OUTPUT_GRACE: Duration = Duration::from_millis(250);

/// Upper bound on a probe's stdout; version strings are tiny.
const MAX_OUTPUT_BYTES: u64 = 8 * 1024;

/// Runs a version probe with a hard timeout and returns trimmed stdout.
///
/// The process is spawned directly (no shell) and killed if it outlives the
/// configured timeout. `PATH` is set explicitly so detection can be pointed at
/// a controlled set of executables.
pub(super) fn probe_version(config: &DetectConfig, program: &str, args: &[&str]) -> Option<String> {
    let executable = resolve_program(config, program)?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .env("PATH", &config.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let receiver = read_output(stdout);
    let deadline = Instant::now() + config.timeout;
    wait_for_child(&mut child, deadline)?;
    drain_output(receiver, deadline)
}

fn read_output(stdout: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut stdout = stdout.take(MAX_OUTPUT_BYTES);
        let mut output = Vec::new();
        let _ = stdout.read_to_end(&mut output);
        let _ = sender.send(output);
    });
    receiver
}

fn wait_for_child(child: &mut Child, deadline: Instant) -> Option<()> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then_some(()),
            Ok(None) => {}
            Err(_) => return None,
        }
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

fn drain_output(receiver: mpsc::Receiver<Vec<u8>>, deadline: Instant) -> Option<String> {
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .checked_add(OUTPUT_GRACE)
        .unwrap_or(OUTPUT_GRACE);
    let output = receiver.recv_timeout(remaining).ok()?;
    let output = String::from_utf8_lossy(&output);
    let version = output.trim();
    (!version.is_empty()).then(|| version.to_owned())
}

/// Resolves an executable through the configured `PATH` without a shell.
fn resolve_program(config: &DetectConfig, program: &str) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() || program.contains(std::path::MAIN_SEPARATOR) {
        return is_executable(path).then(|| path.to_path_buf());
    }

    #[cfg(windows)]
    let names = [format!("{program}.exe"), program.to_owned()];
    #[cfg(not(windows))]
    let names = [program.to_owned()];

    for directory in env::split_paths(&config.path) {
        for name in &names {
            let candidate = directory.join(name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

pub(super) fn nix_version(output: &str) -> String {
    output
        .split_whitespace()
        .last()
        .filter(|version| version.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .unwrap_or(output.trim())
        .to_owned()
}

/// Extracts the client version from `kubectl version --client -o json`,
/// falling back to the first non-empty output line.
pub(super) fn kubectl_version(output: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(output) {
        if let Some(version) = value
            .pointer("/clientVersion/gitVersion")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|version| !version.is_empty())
        {
            return version.to_owned();
        }
    }
    let line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let line = line.strip_prefix("Client Version:").unwrap_or(line).trim();
    line.to_owned()
}
