//! Docker execution primitives for app lifecycle operations.
//!
//! This module only talks to a Docker daemon (or the Docker Compose CLI) at an
//! explicit endpoint. It never pulls images, shells out with user-controlled
//! fragments, or removes volumes.

mod compose;
mod containers;
mod endpoint;
mod ports;

#[cfg(test)]
mod tests;

pub use compose::{apply_compose, compose_available};
pub use containers::{remove_container, run_image, RunResult};
pub use ports::{choose_free_port, PORT_RANGE_END, PORT_RANGE_START};
