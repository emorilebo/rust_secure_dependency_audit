pub mod crates_io;
pub mod github;
pub mod gitlab;
pub mod openssf;

pub use crates_io::{fetch_crate_metadata, CrateMetadata};
pub use github::{fetch_github_metadata, GitHubMetadata};
pub use gitlab::{fetch_gitlab_metadata, GitLabMetadata};
pub use openssf::OpenSSFClient;
