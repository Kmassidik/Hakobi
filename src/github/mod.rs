//! CP9: GitHub integration (App setup, OAuth, webhooks, statuses).

pub mod client;

pub use client::cache::RepositoryCache;

pub use client::{
    changed_files_from_push, example_file_path, parse_example_environment, path_matches,
    verify_webhook_signature, DetectedApp, DetectionResult, GithubClient, RepoSummary,
};
