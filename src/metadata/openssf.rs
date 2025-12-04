use crate::config::NetworkConfig;
use crate::{AuditError, Result};
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, warn};

const OPENSSF_API_BASE: &str = "https://api.securityscorecards.dev";

#[derive(Debug, Deserialize)]
pub struct ScorecardResponse {
    pub score: f32,
    pub date: String,
    pub repo: RepoInfo,
    pub checks: Vec<ScorecardCheck>,
}

#[derive(Debug, Deserialize)]
pub struct RepoInfo {
    pub name: String,
    pub commit: String,
}

#[derive(Debug, Deserialize)]
pub struct ScorecardCheck {
    pub name: String,
    pub score: i32,
    pub reason: String,
    pub details: Option<Vec<String>>,
}

pub struct OpenSSFClient {
    client: Client,
    config: NetworkConfig,
}

impl OpenSSFClient {
    pub fn new(config: &NetworkConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout())
            .user_agent("rust_secure_dependency_audit/0.1")
            .build()
            .map_err(|e| AuditError::network(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            client,
            config: config.clone(),
        })
    }

    pub async fn get_scorecard(&self, repo_url: &str) -> Result<Option<ScorecardResponse>> {
        if !self.config.enable_openssf {
            return Ok(None);
        }

        // Parse repo owner/name from URL
        // Expected format: https://github.com/owner/name
        let parts: Vec<&str> = repo_url.trim_end_matches('/').split('/').collect();
        if parts.len() < 2 {
            return Ok(None);
        }
        
        let name = parts.last().unwrap();
        let owner = parts[parts.len() - 2];
        let platform = if repo_url.contains("github.com") {
            "github.com"
        } else if repo_url.contains("gitlab.com") {
            "gitlab.com"
        } else {
            return Ok(None); // OpenSSF mainly supports GitHub/GitLab
        };

        let url = format!("{}/projects/{}/{}/{}", OPENSSF_API_BASE, platform, owner, name);

        debug!("Fetching OpenSSF Scorecard for {}/{}", owner, name);

        // Simple retry logic
        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                tokio::time::sleep(self.config.request_delay()).await;
            }

            match self.client.get(&url).send().await {
                Ok(resp) => {
                    if resp.status() == 404 {
                        return Ok(None);
                    }
                    if !resp.status().is_success() {
                        warn!("OpenSSF API error: {}", resp.status());
                        continue;
                    }

                    match resp.json::<ScorecardResponse>().await {
                        Ok(data) => return Ok(Some(data)),
                        Err(e) => {
                            warn!("Failed to parse OpenSSF response: {}", e);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("OpenSSF request failed: {}", e);
                }
            }
        }

        Ok(None)
    }
}
