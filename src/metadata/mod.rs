//! Metadata fetchers for crates.io and Git repositories

pub mod crates_io;
pub mod github;
pub mod gitlab;

pub use crates_io::{CrateMetadata, fetch_crate_metadata};
pub use github::{GitHubMetadata, fetch_github_metadata};
pub use gitlab::{GitLabMetadata, fetch_gitlab_metadata};
