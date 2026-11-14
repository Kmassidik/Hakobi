use serde_json::{json, Value};

use super::{payload_value, rejection, sample_request};
use crate::buildrunner::payload::{BuildKind, BuildPayload, MAX_BRANCH_LEN};

#[test]
fn dockerfile_payload_contains_optional_fields() {
    let mut request = sample_request();
    request.dockerfile_path = Some("deploy/Dockerfile".to_owned());
    request.context_dir = Some("deploy".to_owned());
    request.token = Some("ghp_example".to_owned());

    let value = payload_value(BuildKind::Dockerfile, &request);
    assert_eq!(value["git_url"], "https://github.com/acme/app.git");
    assert_eq!(value["branch"], "main");
    assert_eq!(value["image_tag"], "acme/app:latest");
    assert_eq!(value["dockerfile_path"], "deploy/Dockerfile");
    assert_eq!(value["context_dir"], "deploy");
    assert_eq!(value["token"], "ghp_example");
}

#[test]
fn payload_omits_absent_optional_fields() {
    let request = sample_request();
    let value = payload_value(BuildKind::Dockerfile, &request);
    assert!(value.get("dockerfile_path").is_none());
    assert!(value.get("context_dir").is_none());
    assert!(value.get("token").is_none());
    assert_eq!(value.as_object().unwrap().len(), 3);

    let nixpacks = payload_value(BuildKind::Nixpacks, &request);
    assert_eq!(nixpacks.as_object().unwrap().len(), 3);
    assert!(nixpacks.get("token").is_none());
}

#[test]
fn nixpacks_payload_ignores_nothing_and_rejects_dockerfile_options() {
    let mut request = sample_request();
    request.token = Some("ghp_example".to_owned());
    let value = payload_value(BuildKind::Nixpacks, &request);
    assert_eq!(value.as_object().unwrap().len(), 4);
    assert_eq!(value["token"], "ghp_example");

    request.dockerfile_path = Some("Dockerfile".to_owned());
    let error = rejection(BuildKind::Nixpacks, &request);
    assert!(error.contains("nixpacks builds"), "{error}");

    let mut request = sample_request();
    request.context_dir = Some(".".to_owned());
    assert!(BuildPayload::for_request(BuildKind::Nixpacks, &request).is_err());
}

#[test]
fn request_token_is_redacted_from_debug() {
    let mut request = sample_request();
    request.token = Some("super-secret-value".to_owned());
    let debug = format!("{request:?}");
    assert!(!debug.contains("super-secret-value"));
    assert!(debug.contains("<redacted>"));
}

#[test]
fn request_fields_must_be_present_and_sane() {
    let mut request = sample_request();
    request.git_url.clear();
    assert!(rejection(BuildKind::Dockerfile, &request).contains("git_url"));

    let mut request = sample_request();
    request.branch.clear();
    assert!(rejection(BuildKind::Dockerfile, &request).contains("branch"));

    let mut request = sample_request();
    request.image_tag.clear();
    assert!(rejection(BuildKind::Dockerfile, &request).contains("image_tag"));

    let mut request = sample_request();
    request.branch = "ma\u{0}in".to_owned();
    assert!(rejection(BuildKind::Dockerfile, &request).contains("control"));

    let mut request = sample_request();
    request.image_tag = "acme/app\u{7f}".to_owned();
    assert!(rejection(BuildKind::Dockerfile, &request).contains("control"));

    let mut request = sample_request();
    request.branch = "a".repeat(MAX_BRANCH_LEN + 1);
    assert!(rejection(BuildKind::Dockerfile, &request).contains("at most"));
}

#[test]
fn git_url_transport_is_restricted() {
    for value in [
        "-https://github.com/acme/app.git",
        "file:///srv/git/app.git",
        "git://github.com/acme/app.git",
        "http://github.com/acme/app.git",
        "https://user:secret@github.com/acme/app.git",
        "https://github.com/acme/app.git?token=secret",
        "https://github.com/acme/app.git#main",
        "https://github.com/",
        "https://github.com/acme/../app.git",
        "git@github.com:../app.git",
    ] {
        let mut request = sample_request();
        request.git_url = value.to_owned();
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
            "expected rejection for {value:?}"
        );
    }

    for value in [
        "https://github.com/acme/app.git",
        "ssh://git@github.com/acme/app.git",
        "git@github.com:acme/app.git",
    ] {
        let mut request = sample_request();
        request.git_url = value.to_owned();
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok(),
            "expected acceptance for {value:?}"
        );
    }
}

#[test]
fn branches_follow_git_ref_rules() {
    for value in [
        "-main",
        "@",
        "/main",
        "main/",
        "feature//branch",
        ".hidden",
        "release.",
        "release..candidate",
        "release@{1}",
        "feature~1",
        "feature:1",
        "feature?",
        "feature*",
        "feature[1]",
        "feature\\1",
        "feature/.hidden",
        "feature/main.lock",
        "feature branch",
    ] {
        let mut request = sample_request();
        request.branch = value.to_owned();
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
            "expected rejection for {value:?}"
        );
    }

    for value in ["main", "release/v1.2", "feature-ABC_1"] {
        let mut request = sample_request();
        request.branch = value.to_owned();
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok(),
            "expected acceptance for {value:?}"
        );
    }
}

#[test]
fn image_tags_are_checked() {
    for value in [
        "-app",
        "app latest",
        "app@sha256:abc123",
        "app//nested",
        "app/../nested",
        "/app",
        "app/",
        "app:",
        "app:-tag",
        "app:tag with space",
        "app:tag/extra",
    ] {
        let mut request = sample_request();
        request.image_tag = value.to_owned();
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
            "expected rejection for {value:?}"
        );
    }

    for value in [
        "app",
        "app:latest",
        "acme/app:v1.2.3",
        "localhost:5000/team/app:v1",
        "ghcr.io/acme/app",
    ] {
        let mut request = sample_request();
        request.image_tag = value.to_owned();
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok(),
            "expected acceptance for {value:?}"
        );
    }
}

#[test]
fn repository_paths_are_relative_and_safe() {
    for value in [
        "/etc/passwd",
        "~/Dockerfile",
        "../Dockerfile",
        "deploy/../../Dockerfile",
        "deploy\\Dockerfile",
        "C:\\Dockerfile",
        "deploy//Dockerfile",
        "deploy Dockerfile",
        "deploy/\u{0}Dockerfile",
        ".",
        "..",
    ] {
        let mut request = sample_request();
        request.dockerfile_path = Some(value.to_owned());
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
            "expected dockerfile_path rejection for {value:?}"
        );
    }

    for value in [
        "Dockerfile",
        "./Dockerfile",
        "deploy/Dockerfile",
        "dir/Dockerfile",
    ] {
        let mut request = sample_request();
        request.dockerfile_path = Some(value.to_owned());
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok(),
            "expected dockerfile_path acceptance for {value:?}"
        );
    }

    assert_valid_root_context();

    for value in ["/", "..", "../outside", "a/../b", "C:\\repo"] {
        let mut request = sample_request();
        request.context_dir = Some(value.to_owned());
        assert!(
            BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err(),
            "expected context_dir rejection for {value:?}"
        );
    }
}

fn assert_valid_root_context() {
    let mut request = sample_request();
    request.context_dir = Some(".".to_owned());
    assert!(BuildPayload::for_request(BuildKind::Dockerfile, &request).is_ok());
}

#[test]
fn per_build_token_requires_https_and_json_only() {
    let mut request = sample_request();
    request.token = Some("token with space".to_owned());
    assert!(BuildPayload::for_request(BuildKind::Dockerfile, &request).is_err());

    let mut request = sample_request();
    request.git_url = "ssh://git@github.com/acme/app.git".to_owned();
    request.token = Some("ghp_example".to_owned());
    let error = rejection(BuildKind::Dockerfile, &request);
    assert!(error.contains("https"), "{error}");

    let mut request = sample_request();
    request.token = Some("ghp_example".to_owned());
    assert!(BuildPayload::for_request(BuildKind::Nixpacks, &request).is_ok());
}

#[test]
fn json_serialization_does_not_leak_absent_tokens() {
    let request = sample_request();
    let serialized =
        serde_json::to_string(&BuildPayload::for_request(BuildKind::Dockerfile, &request).unwrap())
            .unwrap();
    let value: Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        value,
        json!({
            "git_url": "https://github.com/acme/app.git",
            "branch": "main",
            "image_tag": "acme/app:latest",
        })
    );
    assert!(!serialized.contains("token"));
}

#[test]
fn compose_serialization_carries_context_and_rejects_dockerfile() {
    let mut request = sample_request();
    request.context_dir = Some("deploy".to_owned());
    let serialized =
        serde_json::to_string(&BuildPayload::for_request(BuildKind::Compose, &request).unwrap())
            .unwrap();
    let value: Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        value,
        json!({
            "git_url": "https://github.com/acme/app.git",
            "branch": "main",
            "image_tag": "acme/app:latest",
            "context_dir": "deploy",
        })
    );

    let mut request = sample_request();
    request.dockerfile_path = Some("Dockerfile".to_owned());
    assert!(BuildPayload::for_request(BuildKind::Compose, &request).is_err());

    let request = sample_request();
    let serialized =
        serde_json::to_string(&BuildPayload::for_request(BuildKind::Nixpacks, &request).unwrap())
            .unwrap();
    assert!(!serialized.contains("context_dir"));
    assert!(!serialized.contains("dockerfile_path"));
}
