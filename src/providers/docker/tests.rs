use super::{validate_container_name, validate_endpoint, validate_image_reference};

#[test]
fn docker_endpoints_are_tightly_scoped() {
    assert!(validate_endpoint("unix:///var/run/docker.sock").is_ok());
    assert!(validate_endpoint("tcp://127.0.0.1:2375").is_ok());
    assert!(validate_endpoint("unix://relative.sock").is_err());
    assert!(validate_endpoint("unix:///var/../etc/passwd").is_err());
    assert!(validate_endpoint("tcp://127.0.0.1").is_err());
    assert!(validate_endpoint("tcp://169.254.169.254:80").is_err());
    assert!(validate_endpoint("http://127.0.0.1:2375").is_err());
}

#[test]
fn container_names_are_restricted_to_docker_safe_characters() {
    assert!(validate_container_name("demo-api").is_ok());
    assert!(validate_container_name("demo.api_2").is_ok());
    assert!(validate_container_name("9front").is_ok());
    assert!(validate_container_name("").is_err());
    assert!(validate_container_name("-leading-dash").is_err());
    assert!(validate_container_name("has space").is_err());
    assert!(validate_container_name("slash/name").is_err());
    assert!(validate_container_name("shell;rm").is_err());
    assert!(validate_container_name("flag--option").is_ok());
    assert!(validate_container_name(&"a".repeat(129)).is_err());
}

#[test]
fn image_references_reject_whitespace_and_shell_metacharacters() {
    assert!(validate_image_reference("alpine:3.20").is_ok());
    assert!(validate_image_reference("ghcr.io/hakobi/demo:latest").is_ok());
    assert!(validate_image_reference("registry.example.com:5000/team/app@sha256:abcdef").is_ok());
    assert!(validate_image_reference("").is_err());
    assert!(validate_image_reference("-alpine").is_err());
    assert!(validate_image_reference("alpine latest").is_err());
    assert!(validate_image_reference("alpine;rm -rf /").is_err());
    assert!(validate_image_reference("alpine..latest").is_err());
    assert!(validate_image_reference("registry//app").is_err());
    assert!(validate_image_reference(&"a".repeat(256)).is_err());
}
