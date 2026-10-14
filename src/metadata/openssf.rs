//! Fetch OpenSSF Scorecard results from <https://api.securityscorecards.dev>

use crate::config::NetworkConfig;
use crate::metadata::github::parse_github_url;
use crate::metadata::gitlab::parse_gitlab_url;
use crate::metadata::http;
use crate::Result;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::OnceCell;
use tracing::{debug, warn};

const OPENSSF_API_BASE: &str = "https://api.securityscorecards.dev";

/// The part of the Scorecard response the audit uses.
#[derive(Debug, Deserialize)]
struct ScorecardResponse {
    score: f32,
}

/// OpenSSF Scorecard client shared by every dependency in one audit run.
/// Results are cached per repository.
pub struct OpenSSFClient {
    client: Client,
    api_base: String,
    enabled: bool,
    max_retries: u32,
    retry_delay: std::time::Duration,
    cache: Mutex<HashMap<String, Arc<OnceCell<Option<f32>>>>>,
}

impl OpenSSFClient {
    pub fn new(config: &NetworkConfig) -> Result<Self> {
        Self::with_base_url(config, OPENSSF_API_BASE)
    }

    pub(crate) fn with_base_url(config: &NetworkConfig, api_base: &str) -> Result<Self> {
        Ok(Self {
            client: http::build(http::client_builder(config.timeout()))?,
            api_base: api_base.trim_end_matches('/').to_string(),
            enabled: config.enable_openssf,
            max_retries: config.max_retries,
            retry_delay: config.request_delay(),
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Scorecard score (0.0-10.0) for the repository, or `None` if
    /// unavailable (disabled, unsupported host, not in the dataset, or error).
    pub async fn get_score(&self, repo_url: &str) -> Option<f32> {
        if !self.enabled {
            return None;
        }

        let project = scorecard_project(repo_url)?;
        let cell = {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.entry(project.clone()).or_default().clone()
        };

        *cell.get_or_init(|| self.fetch_score(&project)).await
    }

    async fn fetch_score(&self, project: &str) -> Option<f32> {
        let url = format!("{}/projects/{}", self.api_base, project);
        debug!("Fetching OpenSSF Scorecard for {}", project);

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tokio::time::sleep(self.retry_delay * 2u32.pow(attempt - 1)).await;
            }

            match http::get(&self.client, &url).await {
                Ok(response) if response.status.is_success() => {
                    return match serde_json::from_slice::<ScorecardResponse>(&response.body) {
                        Ok(data) => Some(data.score),
                        Err(e) => {
                            warn!("Failed to parse OpenSSF response for {}: {}", project, e);
                            None
                        }
                    };
                }
                Ok(response) => {
                    let status = response.status;
                    let retryable =
                        status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
                    if !retryable {
                        if status != StatusCode::NOT_FOUND {
                            debug!("OpenSSF API returned HTTP {} for {}", status, project);
                        }
                        return None;
                    }
                    warn!("OpenSSF API returned HTTP {} for {}", status, project);
                }
                Err(e) => warn!("OpenSSF request for {} failed: {}", project, e),
            }
        }

        None
    }
}

/// `github.com/<owner>/<repo>` or `gitlab.com/<path>` for a repository URL.
fn scorecard_project(repo_url: &str) -> Option<String> {
    if repo_url.contains("github.com") {
        let (owner, repo) = parse_github_url(repo_url).ok()?;
        Some(format!("github.com/{}/{}", owner, repo))
    } else if repo_url.contains("gitlab.com") {
        let path = parse_gitlab_url(repo_url).ok()?;
        Some(format!("gitlab.com/{}", path))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_path_strips_git_suffix_and_subpaths() {
        // The Scorecard API answers 404 for `.../serde.git`, so the suffix
        // must be stripped before the lookup.
        assert_eq!(
            scorecard_project("https://github.com/serde-rs/serde.git").as_deref(),
            Some("github.com/serde-rs/serde")
        );
        assert_eq!(
            scorecard_project("https://github.com/rust-lang/cargo/tree/master/crates/home")
                .as_deref(),
            Some("github.com/rust-lang/cargo")
        );
        assert_eq!(
            scorecard_project("https://gitlab.com/group/sub/project").as_deref(),
            Some("gitlab.com/group/sub/project")
        );
        assert_eq!(scorecard_project("https://codeberg.org/a/b"), None);
    }

    #[tokio::test]
    async fn scores_are_cached_per_repository() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/projects/github.com/o/r")
            .with_status(200)
            .with_body(r#"{"score": 6.5, "date": "2026-10-12", "checks": []}"#)
            .expect(1)
            .create_async()
            .await;

        let config = NetworkConfig {
            max_retries: 0,
            ..NetworkConfig::default()
        };
        let client = OpenSSFClient::with_base_url(&config, &server.url()).unwrap();
        assert_eq!(client.get_score("https://github.com/o/r").await, Some(6.5));
        assert_eq!(
            client.get_score("https://github.com/o/r.git").await,
            Some(6.5)
        );
        mock.assert_async().await;
    }
}
