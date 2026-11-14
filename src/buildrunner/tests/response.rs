use reqwest::StatusCode;

use crate::buildrunner::response::{
    describe_http_error, parse_build_response, ERROR_EXCERPT_BYTES, NO_EXIT_CODE,
};

#[test]
fn build_response_parses_status_and_optional_exit_code() {
    assert_successful_response();
    assert_timed_out_response();
    assert_missing_exit_code_response();
}

fn assert_successful_response() {
    let response = parse_build_response(
        br#"{
            "status": "succeeded",
            "image_tag": "acme/app:latest",
            "exit_code": 0,
            "duration_ms": 1234,
            "log_tail": "ok",
            "truncated": false,
            "detail": null
        }"#,
    )
    .unwrap();
    assert_eq!(response.status, "succeeded");
    assert_eq!(response.exit_code, 0);
    assert_eq!(response.duration_ms, 1234);
    assert_eq!(response.log_tail, "ok");
    assert!(!response.truncated);
    assert!(response.detail.is_none());
}

fn assert_timed_out_response() {
    let timed_out = parse_build_response(
        br#"{
            "status": "timed_out",
            "image_tag": "acme/app:latest",
            "exit_code": null,
            "duration_ms": 9,
            "log_tail": "",
            "truncated": true,
            "detail": "build timed out after 1800s"
        }"#,
    )
    .unwrap();
    assert_eq!(timed_out.exit_code, NO_EXIT_CODE);
    assert!(timed_out.truncated);
    assert_eq!(
        timed_out.detail.as_deref(),
        Some("build timed out after 1800s")
    );
}

fn assert_missing_exit_code_response() {
    let missing_exit = parse_build_response(
        br#"{
            "status": "failed",
            "image_tag": "acme/app:latest",
            "duration_ms": 1,
            "log_tail": "boom",
            "truncated": false
        }"#,
    )
    .unwrap();
    assert_eq!(missing_exit.exit_code, NO_EXIT_CODE);
}

#[test]
fn build_response_rejects_malformed_payloads() {
    for body in [
        b"".as_slice(),
        b"not json".as_slice(),
        br#"{"status":"succeeded"}"#.as_slice(),
        br#"{"status":1,"duration_ms":0,"log_tail":"","truncated":false}"#.as_slice(),
        br#"{"status":"succeeded","exit_code":"0","duration_ms":0,"log_tail":"","truncated":false}"#.as_slice(),
    ] {
        assert!(
            parse_build_response(body).is_err(),
            "expected rejection for {}",
            String::from_utf8_lossy(body)
        );
    }
}

#[test]
fn http_errors_are_compact_and_sanitized() {
    let message = describe_http_error(
        StatusCode::UNAUTHORIZED,
        br#"{"error":{"code":"unauthorized","message":"a valid bearer token is required"}}"#,
    );
    assert!(message.contains("401"));
    assert!(message.contains("unauthorized"));
    assert!(message.contains("a valid bearer token is required"));

    let message = describe_http_error(
        StatusCode::BAD_REQUEST,
        br#"{"error":{"code":"validation_error","message":"branch:\nbad\u0000value"}}"#,
    );
    assert!(!message.chars().any(char::is_control), "{message}");
    assert!(message.contains("branch:"));

    let message = describe_http_error(StatusCode::BAD_GATEWAY, b"<html>\nsecret-ish body\n</html>");
    assert!(message.contains("502"));
    assert!(!message.contains('\n'));
    assert!(message.contains("secret-ish body"));

    let message = describe_http_error(StatusCode::INTERNAL_SERVER_ERROR, b"");
    assert_eq!(
        message,
        "build runner returned HTTP 500 Internal Server Error"
    );
}

#[test]
fn http_error_bodies_are_bounded() {
    let huge = format!(
        "{{\"error\":{{\"code\":\"x\",\"message\":\"{}\"}}}}",
        "a".repeat(10_000)
    );
    let message = describe_http_error(StatusCode::INTERNAL_SERVER_ERROR, huge.as_bytes());
    assert!(
        message.len() <= ERROR_EXCERPT_BYTES + 128,
        "{}",
        message.len()
    );

    let huge_plain = "b".repeat(10_000);
    let message = describe_http_error(StatusCode::BAD_GATEWAY, huge_plain.as_bytes());
    assert!(message.len() <= ERROR_EXCERPT_BYTES + 128);
}
