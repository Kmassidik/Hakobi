use crate::host_agent::DockerContainerRow;
use crate::providers::docker::{ContainerPort, ContainerRow};

#[test]
fn conversion_keeps_container_identity() {
    let row = ContainerRow {
        id: "id".into(),
        name: "demo".into(),
        image: "alpine:3.20".into(),
        state: "running".into(),
        status: "Up".into(),
        health: None,
        compose_project: None,
        compose_service: None,
        ports: vec![ContainerPort {
            host_ip: None,
            host_port: Some(18000),
            container_port: 80,
            protocol: "tcp".into(),
        }],
    };
    let round_trip = ContainerRow::from(DockerContainerRow::from(row.clone()));
    assert_eq!(round_trip.id, row.id);
    assert_eq!(round_trip.name, row.name);
    assert_eq!(round_trip.ports.len(), 1);
    assert_eq!(round_trip.ports[0].container_port, 80);
}
