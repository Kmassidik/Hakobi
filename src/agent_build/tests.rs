use std::path::{Path, PathBuf};

use axum::{
    body::to_bytes,
    extract::{Json, State},
    http::{header, HeaderMap, HeaderValue},
};
use serde_json::Value;

use super::{
    build_dockerfile, build_nixpacks,
    process::{run_capture, LogTail, OutputFilter, MAX_LOG_TAIL},
    BuildJob, BuildState,
};

fn invalid_job() -> BuildJob {
    BuildJob {
        git_url: "not-https".to_owned(),
        branch: "main".to_owned(),
        image_tag: "acme/app:latest".to_owned(),
        dockerfile_path: None,
        context_dir: None,
        token: None,
    }
}

fn state() -> BuildState {
    BuildState {
        runs_root: std::env::temp_dir().join("hakobi-native-build-tests"),
        slots: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
    }
}

#[test]
fn filter_redacts_split_tokens_credentials_and_ansi() {
    let mut filter = OutputFilter::new(Some("s3cr3t-token"));
    let mut output = String::new();
    for chunk in [
        b"\x1b[".as_slice(),
        b"31mtoken=s3cr3t".as_slice(),
        b"-token\x1b[0m\nhttps://user:p".as_slice(),
        b"assword@example.com/repo\0\x07\n".as_slice(),
    ] {
        output.push_str(&filter.push(chunk));
    }
    output.push_str(&filter.finish());
    assert!(!output.contains("s3cr3t-token"));
    assert!(!output.contains("user:password"));
    assert!(!output.contains("\x1b"));
    assert!(!output.contains('\0'));
    assert!(output.contains("token=<redacted>"));
    assert!(output.contains("https://<redacted>@example.com/repo"));
}

#[test]
fn filter_preserves_split_utf8_without_control_characters() {
    let mut filter = OutputFilter::new(None);
    let encoded = "héllo".as_bytes();
    let mut output = filter.push(&encoded[..2]);
    output.push_str(&filter.push(&encoded[2..]));
    output.push_str(&filter.push(b"\x01\n"));
    output.push_str(&filter.finish());
    assert_eq!(output, "héllo\n");
}

#[test]
fn retained_log_tail_is_bounded_on_character_boundaries() {
    let mut tail = LogTail::default();
    tail.push(&"ü".repeat(MAX_LOG_TAIL));
    tail.push("final");
    let (text, truncated) = tail.finish();
    assert!(truncated);
    assert!(text.len() <= MAX_LOG_TAIL);
    assert!(text.ends_with("final"));
}

#[test]
fn real_process_stdout_and_stderr_are_both_forwarded() {
    let mut output = String::new();
    let code = run_capture(
        &std::env::temp_dir(),
        "sh",
        &["-c", "printf 'stdout'; printf 'stderr' >&2"],
        None,
        &mut |message| output.push_str(message),
    );
    assert_eq!(code, 0);
    assert!(output.contains("stdout"), "{output}");
    assert!(output.contains("stderr"), "{output}");
}

#[tokio::test]
async fn ndjson_response_emits_runner_error_then_complete() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("text/plain, application/x-ndjson; q=1"),
    );
    let response = build_dockerfile(State(state()), headers, Json(invalid_job())).await;
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/x-ndjson"
    );
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let events: Vec<Value> = body
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["type"], "log");
    assert_eq!(events[0]["stage"], "runner");
    assert_eq!(events[1]["type"], "complete");
    assert_eq!(events[1]["outcome"]["status"], "failed");
}

#[tokio::test]
async fn old_json_response_is_unchanged_without_ndjson_accept() {
    let mut headers = HeaderMap::new();
    headers.insert(header::ACCEPT, HeaderValue::from_static("*/*"));
    let response = build_nixpacks(State(state()), headers, Json(invalid_job())).await;
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    let body = to_bytes(response.into_body(), 64 * 1024).await.unwrap();
    let outcome: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(outcome["status"], "failed");
    assert_eq!(outcome["exit_code"], 1);
    assert!(outcome.get("type").is_none());
    assert!(outcome.get("log_tail").is_some());
    assert!(outcome.get("truncated").is_some());
}

#[test]
fn owned_sources_and_functions_stay_within_limits() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("src/agent_build.rs")];
    collect_sources(&root.join("src/agent_build"), &mut files);
    collect_sources(&root.join("src/buildrunner"), &mut files);
    files.push(root.join("src/buildrunner.rs"));
    for path in files {
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(
            source.lines().count() <= 500,
            "{} is too long",
            path.display()
        );
        assert_function_lengths(&path, &source);
    }
}

fn collect_sources(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn assert_function_lengths(path: &Path, source: &str) {
    let lines: Vec<&str> = source.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if !trimmed.starts_with("//") && trimmed.contains("fn ") {
            let length = function_length(&lines, index);
            assert!(
                length <= 50,
                "{}:{} is {length} lines",
                path.display(),
                index + 1
            );
            index += length;
        } else {
            index += 1;
        }
    }
}

fn function_length(lines: &[&str], start: usize) -> usize {
    let mut depth = 0_i32;
    let mut opened = false;
    for (offset, line) in lines[start..].iter().enumerate() {
        for character in line.chars() {
            match character {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth == 0 {
            return offset + 1;
        }
    }
    1
}
