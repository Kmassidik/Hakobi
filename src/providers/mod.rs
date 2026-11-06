pub mod agent;
pub mod docker;
pub mod kubernetes;

pub use agent::{
    detect_local_access, docker_socket_present, ensure_local_provider, AgentBackend,
    LocalDockerAccess, RoutingBackend,
};
pub use docker::{
    validate_container_name, validate_image_reference, ContainerBackend, ContainerGroup,
    ContainerInventory, ContainerPort, ContainerRow, DockerBackend, DockerSnapshot, HostInfo,
    ImageRow, NetworkRow, RunResult, ServiceGroup, StackGroup, VolumeRow,
};
pub use kubernetes::{
    ConnectionInfo, DeploymentRow, KubernetesProvider, NamespaceRow, PodRow, ServicePort,
    ServiceRow,
};
