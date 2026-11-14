use crate::buildrunner::BuildRunnerClient;

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
