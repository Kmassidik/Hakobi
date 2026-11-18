use super::*;

fn log_catalog() -> (tempfile::TempDir, Catalog, String) {
    let directory = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::open(directory.path().join("catalog.sqlite3")).unwrap();
    let host = catalog
        .create_host_agent("logs", "https://logs.example.com", "tok_logs_01")
        .unwrap();
    let app = catalog
        .create_app(
            "web",
            &host,
            "dockerfile",
            Some("acme/web"),
            Some("main"),
            None,
            Some("Dockerfile"),
            false,
            Some(8080),
            None,
            None,
            None,
        )
        .unwrap();
    let deployment = catalog
        .create_deployment(&app, "manual", None, None)
        .unwrap();
    (directory, catalog, deployment)
}

#[test]
fn cancelled_deployment_cannot_advance() {
    let directory = tempfile::tempdir().unwrap();
    let mut catalog = Catalog::open(directory.path().join("catalog.sqlite3")).unwrap();
    let host = catalog
        .create_host_agent("host", "https://host.example.com", "tok_host_01")
        .unwrap();
    let app = catalog
        .create_app(
            "web",
            &host,
            "dockerfile",
            Some("acme/web"),
            Some("main"),
            None,
            Some("Dockerfile"),
            false,
            Some(8080),
            None,
            None,
            None,
        )
        .unwrap();
    let deployment = catalog
        .create_deployment(&app, "manual", None, None)
        .unwrap();

    assert!(catalog
        .advance_deployment_status(&deployment, "cancelled", None)
        .unwrap());
    assert!(!catalog
        .advance_deployment_status(&deployment, "running", None)
        .unwrap());
    let record = catalog.get_deployment(&deployment).unwrap().unwrap();
    assert_eq!(record.status, "cancelled");
    assert!(record.completed_at.is_some());
}

#[test]
fn log_lines_are_bounded_and_retention_is_explicit() {
    let (_directory, mut catalog, deployment) = log_catalog();
    for index in 0..2005 {
        catalog
            .add_deployment_log(&deployment, "info", &format!("line {index}"))
            .unwrap();
    }
    let logs = catalog.list_deployment_logs(&deployment).unwrap();
    assert_eq!(logs.len(), DEPLOYMENT_LOG_LINE_LIMIT as usize);
    assert_eq!(
        logs.iter()
            .filter(|log| log.message == DEPLOYMENT_LOG_TRUNCATION_MARKER)
            .count(),
        1
    );
    assert_eq!(logs.last().unwrap().message, "line 2004");
}

#[test]
fn log_messages_and_total_bytes_are_bounded() {
    let (_directory, mut catalog, deployment) = log_catalog();
    for _ in 0..70 {
        catalog
            .add_deployment_log(&deployment, "info", &"é".repeat(9000))
            .unwrap();
    }
    let logs = catalog.list_deployment_logs(&deployment).unwrap();
    assert!(logs.iter().all(|log| log.message.len() <= 16 * 1024));
    assert!(logs.iter().map(|log| log.message.len()).sum::<usize>() <= 1024 * 1024);
    assert!(logs
        .iter()
        .any(|log| log.message.contains(MESSAGE_TRUNCATION_MARKER)));
}

#[test]
fn log_retrieval_can_resume_after_an_id() {
    let (_directory, mut catalog, deployment) = log_catalog();
    catalog
        .add_deployment_log(&deployment, "info", "first")
        .unwrap();
    catalog
        .add_deployment_log(&deployment, "warn", "second")
        .unwrap();
    let logs = catalog.list_deployment_logs(&deployment).unwrap();
    let rest = catalog
        .list_deployment_logs_after(&deployment, logs[0].id)
        .unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(rest[0].message, "second");
}
