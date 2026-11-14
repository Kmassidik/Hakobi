use super::compose::{
    compose_up_args, validate_compose_project, validate_compose_yaml, MAX_COMPOSE_BYTES,
};
use super::containers::{
    container_config, remove_options, validate_container_name, validate_container_port,
    validate_image_reference,
};
use super::ports::is_port_collision;
use super::{apply_compose, choose_free_port, run_image, PORT_RANGE_END, PORT_RANGE_START};

#[test]
fn free_port_starts_at_range_start() {
    assert_eq!(choose_free_port(std::iter::empty()), PORT_RANGE_START);
}

#[test]
fn free_port_skips_used_ports() {
    assert_eq!(choose_free_port([18000, 18002]), 18001);
    assert_eq!(choose_free_port(18000..18197), 18197);
}

#[test]
fn free_port_ignores_out_of_range_ports() {
    assert_eq!(choose_free_port([1, 2, 65535]), PORT_RANGE_START);
}

#[test]
fn free_port_falls_back_to_start_when_range_is_full() {
    assert_eq!(
        choose_free_port(PORT_RANGE_START..PORT_RANGE_END),
        PORT_RANGE_START
    );
}

#[test]
fn image_references_are_validated() {
    for image in [
        "nginx",
        "nginx:1.27",
        "ghcr.io/acme/app:v1.2.3",
        "localhost:5000/team/app@sha256:abc123",
    ] {
        assert!(validate_image_reference(image).is_ok(), "{image}");
    }
    for image in [
        "",
        "-rm",
        "nginx latest",
        "nginx;rm",
        "nginx\nlatest",
        "../app",
        "app//x",
    ] {
        assert!(validate_image_reference(image).is_err(), "{image}");
    }
}

#[test]
fn container_names_are_tightly_scoped() {
    for name in ["web", "web-1", "web_1.blue", "1web"] {
        assert!(validate_container_name(name).is_ok(), "{name}");
    }
    for name in ["", "-web", "web/evil", "web evil", "../web"] {
        assert!(validate_container_name(name).is_err(), "{name}");
    }
}

#[test]
fn container_ports_exclude_zero() {
    assert!(validate_container_port(0).is_err());
    assert!(validate_container_port(1).is_ok());
    assert!(validate_container_port(80).is_ok());
    assert!(validate_container_port(u16::MAX).is_ok());
}

#[test]
fn compose_project_is_validated() {
    for project in ["app", "my_app", "app-2", "1app"] {
        assert!(validate_compose_project(project).is_ok(), "{project}");
    }
    for project in ["", "MyApp", "-app", "app two", "app/2"] {
        assert!(validate_compose_project(project).is_err(), "{project}");
    }
}

#[test]
fn compose_file_is_validated() {
    assert!(validate_compose_yaml("services: {}").is_ok());
    assert!(validate_compose_yaml("   ").is_err());
    assert!(validate_compose_yaml("services:\0").is_err());
    assert!(validate_compose_yaml(&"a".repeat(MAX_COMPOSE_BYTES + 1)).is_err());
}

#[test]
fn compose_command_is_fixed_and_uses_stdin() {
    assert_eq!(
        compose_up_args("myapp"),
        vec![
            "compose",
            "-f",
            "-",
            "-p",
            "myapp",
            "up",
            "-d",
            "--remove-orphans"
        ]
    );
}

#[test]
fn compose_command_keeps_project_as_one_argument() {
    let args = compose_up_args("app-x");
    assert_eq!(args.iter().filter(|arg| *arg == "app-x").count(), 1);
    assert!(!args.iter().any(|arg| arg.contains(' ')));
}

#[test]
fn port_collision_messages_are_recognized() {
    let collision = bollard::errors::Error::DockerResponseServerError {
        status_code: 500,
        message: "Bind for 0.0.0.0:18000 failed: port is already allocated".into(),
    };
    assert!(is_port_collision(&collision));

    let conflict = bollard::errors::Error::DockerResponseServerError {
        status_code: 409,
        message: "Conflict. The container name is already in use".into(),
    };
    assert!(!is_port_collision(&conflict));

    let missing_image = bollard::errors::Error::DockerResponseServerError {
        status_code: 404,
        message: "No such image: nginx".into(),
    };
    assert!(!is_port_collision(&missing_image));
}

#[test]
fn remove_options_keep_volumes() {
    let options = remove_options();
    assert!(options.force);
    assert!(!options.v);
}

#[tokio::test]
async fn invalid_inputs_fail_before_connecting() {
    let error = run_image(
        "tcp://127.0.0.1:1",
        "bad image",
        "web",
        80,
        &Default::default(),
    )
    .await
    .expect_err("invalid image");
    assert!(error.to_string().contains("image reference"), "{error}");

    let error = run_image(
        "tcp://127.0.0.1:1",
        "nginx",
        "bad name",
        80,
        &Default::default(),
    )
    .await
    .expect_err("invalid name");
    assert!(error.to_string().contains("container name"), "{error}");

    let error = run_image("tcp://127.0.0.1:1", "nginx", "web", 0, &Default::default())
        .await
        .expect_err("invalid port");
    assert!(error.to_string().contains("container port"), "{error}");
}

#[tokio::test]
async fn compose_inputs_fail_before_invoking_docker() {
    let error = apply_compose("unix:///var/run/docker.sock", "Bad Project", "services: {}")
        .await
        .expect_err("invalid project");
    assert!(error.to_string().contains("Compose project"), "{error}");

    let error = apply_compose("unix:///var/run/docker.sock", "myapp", "  ")
        .await
        .expect_err("empty compose file");
    assert!(error.to_string().contains("must not be empty"), "{error}");
}

#[test]
fn container_config_binds_exposed_port() {
    let config = container_config("nginx", 80, 18000, &Default::default());
    assert_eq!(config.image.as_deref(), Some("nginx"));
    assert_eq!(
        config.exposed_ports.as_deref(),
        Some(&["80/tcp".to_owned()][..])
    );
    let host = config.host_config.expect("host config");
    let binding = host
        .port_bindings
        .expect("port bindings")
        .get("80/tcp")
        .cloned()
        .flatten()
        .expect("binding");
    assert_eq!(binding.len(), 1);
    assert_eq!(binding[0].host_port.as_deref(), Some("18000"));
}
