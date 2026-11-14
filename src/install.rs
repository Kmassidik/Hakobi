//! Host readiness detection for the tooling Hakobi can orchestrate.
//!
//! [`detect`] inspects the local host without running installers or any
//! privileged command. Each probe uses a fixed executable name and a fixed
//! argument vector, never a shell, and is bounded by a short timeout:
//!
//! * Docker is ready when the daemon answers `docker version` or a local
//!   daemon socket exists.
//! * Compose is the Docker Compose plugin (`docker compose version --short`).
//! * Kubernetes is the `kubectl` client.
//! * Nixpacks is the `nixpacks` builder.
//!
//! Missing tools carry a distro-neutral, display-only installation hint. The
//! hint is never executed.

use serde::{Deserialize, Serialize};

mod detection;
mod probes;
#[cfg(all(test, unix))]
mod tests;

pub use detection::detect;

/// Availability of a single host tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ToolStatus {
    /// The tool answered its version probe.
    Ready {
        /// Version reported by the tool.
        version: String,
    },
    /// The tool is not installed or did not answer.
    Missing {
        /// Safe, display-only installation hint.
        install_command: String,
    },
}

/// Readiness of every tool the stack lifecycle features can use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Readiness {
    pub docker: ToolStatus,
    pub compose: ToolStatus,
    pub kubectl: ToolStatus,
    pub nix: ToolStatus,
    pub nixpacks: ToolStatus,
}
