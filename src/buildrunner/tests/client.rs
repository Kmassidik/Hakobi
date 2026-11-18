use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use tokio::sync::mpsc::unbounded_channel;

use super::sample_request;
use crate::buildrunner::{BuildLogEvent, BuildRunnerClient};

#[test]
fn base_url_is_validated_and_trimmed() {
    let client = BuildRunnerClient::new("https://runner.example.com/", None).unwrap();
    assert_eq!(client.base_url, "https://runner.example.com");
    assert_eq!(
        client.endpoint("/build/dockerfile"),
        "https://runner.example.com/build/dockerfile"
    );

    let client = BuildRunnerClient::new("http://127.0.0.1:8787///", None).unwrap();
    assert_eq!(client.base_url, "http://127.0.0.1:8787");
    assert_eq!(client.endpoint("/healthz"), "http://127.0.0.1:8787/healthz");

    let client = BuildRunnerClient::new(" https://runner.example.com/base ", None).unwrap();
    assert_eq!(client.base_url, "https://runner.example.com/base");
    assert_eq!(
        client.endpoint("/build/nixpacks"),
        "https://runner.example.com/base/build/nixpacks"
    );
}

#[test]
fn base_url_rejects_non_http_and_unsafe_forms() {
    for value in [
        "",
        "   ",
        "runner.example.com",
        "ftp://runner.example.com",
        "file:///tmp/runner.sock",
        "https://user:secret@runner.example.com",
        "https://user@runner.example.com",
        "https://runner.example.com/?token=secret",
        "https://runner.example.com/#frag",
        "https://runner.example.com/\u{0}",
    ] {
        assert!(
            BuildRunnerClient::new(value, None).is_err(),
            "expected rejection for {value:?}"
        );
    }

    for value in ["https://runner.example.com", "http://127.0.0.1:8787"] {
        assert!(
            BuildRunnerClient::new(value, None).is_ok(),
            "expected acceptance for {value:?}"
        );
    }
}

#[test]
fn client_token_is_validated_and_redacted() {
    for token in ["", "short token", "line\nbreak", "tökén", "tab\tin-token"] {
        assert!(
            BuildRunnerClient::new("https://runner.example.com", Some(token)).is_err(),
            "expected rejection for token {token:?}"
        );
    }
    let client =
        BuildRunnerClient::new("https://runner.example.com", Some("0123456789abcdef")).unwrap();
    let debug = format!("{client:?}");
    assert!(!debug.contains("0123456789abcdef"));
    assert!(debug.contains("<redacted>"));
}

#[test]
fn from_optional_requires_only_a_url() {
    assert!(BuildRunnerClient::from_optional(None, None)
        .unwrap()
        .is_none());
    assert!(
        BuildRunnerClient::from_optional(Some("   "), Some("ignored"))
            .unwrap()
            .is_none()
    );
    assert!(
        BuildRunnerClient::from_optional(Some("https://runner.example.com"), Some("   "))
            .unwrap()
            .is_some()
    );
    assert!(BuildRunnerClient::from_optional(
        Some("https://runner.example.com"),
        Some("bad token")
    )
    .is_err());
    assert!(BuildRunnerClient::from_optional(Some("ftp://runner.example.com"), None).is_err());
}

async fn streaming_server(content_type: &'static str, body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().fallback(post(move |headers: HeaderMap| async move {
        let accept = headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if accept != "application/x-ndjson" {
            return StatusCode::NOT_ACCEPTABLE.into_response();
        }
        Response::builder()
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(body))
            .unwrap()
    }));
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{address}")
}

#[tokio::test]
async fn ndjson_stream_forwards_logs_and_returns_completion() {
    let body = concat!(
        "{\"type\":\"log\",\"stage\":\"clone\",\"message\":\"cloned\"}\n",
        "{\"type\":\"log\",\"stage\":\"build\",\"message\":\"built\"}\n",
        "{\"type\":\"complete\",\"outcome\":{\"status\":\"succeeded\",",
        "\"exit_code\":0,\"duration_ms\":12,\"log_tail\":\"built\",",
        "\"truncated\":false,\"detail\":null}}\n"
    );
    let url = streaming_server("application/x-ndjson; charset=utf-8", body).await;
    let client = BuildRunnerClient::new(&url, None).unwrap();
    let (sender, mut receiver) = unbounded_channel();
    let response = client
        .build_dockerfile_stream(&sample_request(), sender)
        .await
        .unwrap();
    assert_eq!(response.status, "succeeded");
    assert_eq!(response.log_tail, "built");
    assert_eq!(
        receiver.try_recv().unwrap(),
        BuildLogEvent {
            stage: "clone".to_owned(),
            message: "cloned".to_owned()
        }
    );
    assert_eq!(
        receiver.try_recv().unwrap(),
        BuildLogEvent {
            stage: "build".to_owned(),
            message: "built".to_owned()
        }
    );
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn json_fallback_forwards_one_nonempty_log_tail() {
    let body = concat!(
        "{\"status\":\"failed\",\"exit_code\":1,\"duration_ms\":12,",
        "\"log_tail\":\"clone failed\",\"truncated\":false,",
        "\"detail\":\"git clone failed\"}"
    );
    let url = streaming_server("application/json", body).await;
    let client = BuildRunnerClient::new(&url, None).unwrap();
    let (sender, mut receiver) = unbounded_channel();
    let response = client
        .build_compose_stream(&sample_request(), sender)
        .await
        .unwrap();
    assert_eq!(response.status, "failed");
    assert_eq!(
        receiver.try_recv().unwrap(),
        BuildLogEvent {
            stage: "runner".to_owned(),
            message: "clone failed".to_owned()
        }
    );
    assert!(receiver.try_recv().is_err());
}

#[tokio::test]
async fn ndjson_stream_requires_a_complete_event() {
    let url = streaming_server(
        "application/x-ndjson",
        "{\"type\":\"log\",\"stage\":\"build\",\"message\":\"working\"}\n",
    )
    .await;
    let client = BuildRunnerClient::new(&url, None).unwrap();
    let (sender, _receiver) = unbounded_channel();
    let error = client
        .build_nixpacks_stream(&sample_request(), sender)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("without a complete event"));
}
