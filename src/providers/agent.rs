//! Container backend backed by the native host agent.
//!
//! The dashboard container has no Docker socket. When the host agent is
//! configured, provider rows using the `agent://` scheme are served through the
//! agent's typed Docker endpoints instead. Everything else keeps talking to
//! Docker directly, which is what [`RoutingBackend`] decides per request.

mod backend;
mod conversions;
mod local;
mod routing;
#[cfg(test)]
mod tests;

pub use backend::AgentBackend;
pub use local::{
    detect_local_access, docker_socket_present, ensure_local_provider, LocalDockerAccess,
};
pub use routing::RoutingBackend;
