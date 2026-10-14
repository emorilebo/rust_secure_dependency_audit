pub mod crates_io;
pub mod github;
pub mod gitlab;
pub(crate) mod http;
pub mod openssf;

pub use crates_io::{CrateMetadata, CratesIoClient};
pub use github::{GitHubClient, GitHubMetadata};
pub use gitlab::{fetch_gitlab_metadata, GitLabMetadata};
pub use openssf::OpenSSFClient;
