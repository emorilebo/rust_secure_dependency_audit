//! Fetch metadata from GitHub repositories

use crate::config::NetworkConfig;
use crate::error::{AuditError, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

const GITHUB_API: &str = "https://api.github.com";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Metadata from GitHub for a repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubMetadata {
    pub name: String,
    pub full_name: String,
    pub description: Option<String>,
    pub stars: u32,
    pub forks: u32,
    pub open_issues: u32,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub pushed_at: DateTime<Utc>,
    pub contributors_count: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct GitHubRepo {
    name: String,
    full_name: String,
    description: Option<String>,
    stargazers_count: u32,
    forks_count: u32,
    open_issues_count: u32,
    archived: bool,
    created_at: String,
    updated_at: String,
    pushed_at: String,
}

/// Fetch metadata for a GitHub repository
pub async fn fetch_github_metadata(
    repo_url: &str,
    config: &NetworkConfig,
) -> Result<GitHubMetadata> {
    let (owner, repo) = parse_github_url(repo_url)?;
    debug!("Fetching GitHub metadata for {}/{}", owner, repo);

    let client = build_client(config)?;
    let repo_url = format!("{}/repos/{}/{}", GITHUB_API, owner, repo);

    // Fetch repository info
    let repo_data = fetch_with_retry(&client, &repo_url, config).await?;

    // Optionally fetch contributors count (separate API call)
    let contributors_url = format!("{}/contributors?per_page=1", repo_url);
    let contributors_count = fetch_contributors_count(&client, &contributors_url, config).await.ok();

    let created_at = parse_github_datetime(&repo_data.created_at)?;
    let updated_at = parse_github_datetime(&repo_data.updated_at)?;
    let pushed_at = parse_github_datetime(&repo_data.pushed_at)?;

    Ok(GitHubMetadata {
        name: repo_data.name,
        full_name: repo_data.full_name,
        description: repo_data.description,
        stars: repo_data.stargazers_count,
        forks: repo_data.forks_count,
        open_issues: repo_data.open_issues_count,
        is_archived: repo_data.archived,
        created_at,
        updated_at,
        pushed_at,
        contributors_count,
    })
}

/// Parse GitHub URL to extract owner and repo name
fn parse_github_url(url: &str) -> Result<(String, String)> {
    // Handle various GitHub URL formats:
    // - https://github.com/owner/repo
    // - https://github.com/owner/repo.git
    // - git://github.com/owner/repo
    // - git@github.com:owner/repo.git

    let url = url.trim_end_matches(".git");
    let url = url.trim_end_matches('/');

    let parts: Vec<&str> = if url.contains("github.com:") {
        // SSH format: git@github.com:owner/repo
        url.split("github.com:").nth(1).unwrap_or("").split('/').collect()
    } else if url.contains("github.com/") {
        // HTTPS/Git format: https://github.com/owner/repo
        url.split("github.com/").nth(1).unwrap_or("").split('/').collect()
    } else {
        return Err(AuditError::parse(format!("Invalid GitHub URL: {}", url)));
    };

    if parts.len() >= 2 {
        Ok((parts[0].to_string(), parts[1].to_string()))
    } else {
        Err(AuditError::parse(format!("Invalid GitHub URL: {}", url)))
    }
}

/// Build HTTP client with GitHub authentication if available
fn build_client(config: &NetworkConfig) -> Result<Client> {
    let mut builder = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(config.timeout());

    // Add default headers for GitHub API
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        reqwest::header::ACCEPT,
        "application/vnd.github.v3+json".parse().unwrap(),
    );

    if let Some(token) = &config.github_token {
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("token {}", token).parse().unwrap(),
        );
    }

    builder = builder.default_headers(headers);

    builder.build()
        .map_err(|e| AuditError::network(format!("Failed to build HTTP client: {}", e)))
}

/// Fetch data with retry logic
async fn fetch_with_retry(
    client: &Client,
    url: &str,
    config: &NetworkConfig,
) -> Result<GitHubRepo> {
    let mut attempts = 0;
    let mut delay = config.request_delay();

    loop {
        match client.get(url).send().await {
            Ok(response) => {
                // Check for rate limiting
                if response.status().as_u16() == 403 {
                    let retry_after = response
                        .headers()
                        .get("x-ratelimit-reset")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|timestamp| {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs();
                            Duration::from_secs(timestamp.saturating_sub(now))
                        });

                    return Err(AuditError::RateLimitExceeded {
                        service: "GitHub".to_string(),
                        retry_after,
                    });
                }

                if response.status().as_u16() == 404 {
                    return Err(AuditError::api("GitHub", "Repository not found"));
                }

                if !response.status().is_success() {
                    return Err(AuditError::api(
                        "GitHub",
                        format!("HTTP {}", response.status()),
                    ));
                }

                let data: GitHubRepo = response.json().await?;
                return Ok(data);
            }
            Err(e) => {
                if attempts >= config.max_retries {
                    return Err(AuditError::network(format!("GitHub request failed: {}", e)));
                }
                warn!("GitHub request failed, retrying: {}", e);
                tokio::time::sleep(delay).await;
                attempts += 1;
                delay *= 2;
            }
        }
    }
}

/// Fetch contributors count from Link header pagination
async fn fetch_contributors_count(
    client: &Client,
    url: &str,
    config: &NetworkConfig,
) -> Result<u32> {
    match client.get(url).send().await {
        Ok(response) => {
            if !response.status().is_success() {
                return Ok(0);
            }

            // GitHub returns Link header with pagination info
            // We can estimate from the "last" page link
            if let Some(link_header) = response.headers().get("link") {
                if let Ok(link_str) = link_header.to_str() {
                    if let Some(last_page) = extract_last_page(link_str) {
                        return Ok(last_page);
                    }
                }
            }

            // Fallback: count items in response
            if let Ok(contributors) = response.json::<Vec<serde_json::Value>>().await {
                Ok(contributors.len() as u32)
            } else {
                Ok(0)
            }
        }
        Err(_) => Ok(0), // Don't fail if contributors fetch fails
    }
}

/// Extract last page number from Link header
fn extract_last_page(link_header: &str) -> Option<u32> {
    for link in link_header.split(',') {
        if link.contains("rel=\"last\"") {
            // Extract page number from URL
            if let Some(page_str) = link
                .split("page=")
                .nth(1)
                .and_then(|s| s.split('>').next())
            {
                return page_str.parse().ok();
            }
        }
    }
    None
}

/// Parse GitHub datetime format
fn parse_github_datetime(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| AuditError::parse(format!("Invalid GitHub datetime: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url() {
        let test_cases = vec![
            ("https://github.com/serde-rs/serde", ("serde-rs", "serde")),
            ("https://github.com/serde-rs/serde.git", ("serde-rs", "serde")),
            ("git://github.com/serde-rs/serde", ("serde-rs", "serde")),
            ("git@github.com:serde-rs/serde.git", ("serde-rs", "serde")),
        ];

        for (url, expected) in test_cases {
            let result = parse_github_url(url).unwrap();
            assert_eq!(result, (expected.0.to_string(), expected.1.to_string()));
        }
    }

    #[test]
    fn test_extract_last_page() {
        let link_header = r#"<https://api.github.com/repos/rust-lang/rust/contributors?page=2>; rel="next", <https://api.github.com/repos/rust-lang/rust/contributors?page=50>; rel="last""#;
        assert_eq!(extract_last_page(link_header), Some(50));
    }
}
