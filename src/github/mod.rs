//! CP9: GitHub integration (App setup, OAuth, webhooks, statuses).

pub mod client;

pub use client::cache::RepositoryCache;

pub use client::{
    changed_files_from_push, path_matches, verify_webhook_signature, DetectedApp, DetectionResult,
    GithubClient, RepoSummary,
};
