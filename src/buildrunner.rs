//! HTTP client for the standalone Hakobi build runner.
//!
//! The runner is an optional service that clones a repository and builds an
//! image with `docker build` or `nixpacks`. This module only speaks HTTP: it
//! never starts, stops, or configures the runner, and it never sends the
//! bearer token as part of a URL.

mod client;
mod payload;
mod response;
#[cfg(test)]
mod tests;

pub use client::{BuildRequest, BuildRunnerClient};
pub use response::BuildResponse;
