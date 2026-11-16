use crate::host_agent::{DockerContainerRow, DockerNetworkRow, DockerVolumeRow};
use crate::providers::docker::{ContainerPort, ContainerRow, NetworkRow, VolumeRow};

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

#[test]
fn conversion_keeps_resource_ownership() {
    let app_id = Some("123e4567-e89b-42d3-a456-426614174000".to_owned());
    let volume = VolumeRow {
        name: "data".into(),
        driver: "local".into(),
        mountpoint: "/var/lib/docker/volumes/data/_data".into(),
        app_id: app_id.clone(),
    };
    let volume = VolumeRow::from(DockerVolumeRow::from(volume));
    assert_eq!(volume.app_id, app_id);

    let network = NetworkRow {
        name: "app-net".into(),
        driver: "bridge".into(),
        scope: "local".into(),
        app_id: app_id.clone(),
    };
    let network = NetworkRow::from(DockerNetworkRow::from(network));
    assert_eq!(network.app_id, app_id);
}
