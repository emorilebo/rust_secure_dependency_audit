//! Fetch metadata from GitLab repositories

use crate::config::NetworkConfig;
use crate::error::{AuditError, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::debug;

const GITLAB_API: &str = "https://gitlab.com/api/v4";
const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Metadata from GitLab for a repository
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLabMetadata {
    pub name: String,
    pub path_with_namespace: String,
    pub description: Option<String>,
    pub stars: u32,
    pub forks: u32,
    pub open_issues: u32,
    pub is_archived: bool,
    pub created_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct GitLabProject {
    name: String,
    path_with_namespace: String,
    description: Option<String>,
    star_count: u32,
    forks_count: u32,
    archived: bool,
    created_at: String,
    last_activity_at: String,
    #[serde(default)]
    open_issues_count: u32,
}

/// Fetch metadata for a GitLab repository
pub async fn fetch_gitlab_metadata(
    repo_url: &str,
    config: &NetworkConfig,
) -> Result<GitLabMetadata> {
    let project_path = parse_gitlab_url(repo_url)?;
    debug!("Fetching GitLab metadata for {}", project_path);

    let client = build_client(config)?;
    
    // URL-encode the project path
    let encoded_path = urlencoding::encode(&project_path);
    let url = format!("{}/projects/{}", GITLAB_API, encoded_path);

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        if response.status().as_u16() == 404 {
            return Err(AuditError::api("GitLab", "Project not found"));
        }
        return Err(AuditError::api(
            "GitLab",
            format!("HTTP {}", response.status()),
        ));
    }

    let project: GitLabProject = response.json().await?;

    let created_at = parse_gitlab_datetime(&project.created_at)?;
    let last_activity_at = parse_gitlab_datetime(&project.last_activity_at)?;

    Ok(GitLabMetadata {
        name: project.name,
        path_with_namespace: project.path_with_namespace,
        description: project.description,
        stars: project.star_count,
        forks: project.forks_count,
        open_issues: project.open_issues_count,
        is_archived: project.archived,
        created_at,
        last_activity_at,
    })
}

/// Parse GitLab URL to extract project path
fn parse_gitlab_url(url: &str) -> Result<String> {
    // Handle various GitLab URL formats:
    // - https://gitlab.com/group/project
    // - https://gitlab.com/group/subgroup/project
    // - git@gitlab.com:group/project.git

    let url = url.trim_end_matches(".git");
    let url = url.trim_end_matches('/');

    let path = if url.contains("gitlab.com:") {
        // SSH format
        url.split("gitlab.com:").nth(1).unwrap_or("")
    } else if url.contains("gitlab.com/") {
        // HTTPS format
        url.split("gitlab.com/").nth(1).unwrap_or("")
    } else {
        return Err(AuditError::parse(format!("Invalid GitLab URL: {}", url)));
    };

    if path.is_empty() {
        return Err(AuditError::parse(format!("Invalid GitLab URL: {}", url)));
    }

    Ok(path.to_string())
}

/// Build HTTP client with GitLab authentication if available
fn build_client(config: &NetworkConfig) -> Result<Client> {
    let mut builder = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(config.timeout());

    if let Some(token) = &config.gitlab_token {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "PRIVATE-TOKEN",
            token.parse().unwrap(),
        );
        builder = builder.default_headers(headers);
    }

    builder.build()
        .map_err(|e| AuditError::network(format!("Failed to build HTTP client: {}", e)))
}

/// Parse GitLab datetime format (ISO 8601)
fn parse_gitlab_datetime(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| AuditError::parse(format!("Invalid GitLab datetime: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gitlab_url() {
        let test_cases = vec![
            ("https://gitlab.com/gitlab-org/gitlab", "gitlab-org/gitlab"),
            ("https://gitlab.com/gitlab-org/gitlab.git", "gitlab-org/gitlab"),
            ("git@gitlab.com:gitlab-org/gitlab.git", "gitlab-org/gitlab"),
        ];

        for (url, expected) in test_cases {
            let result = parse_gitlab_url(url).unwrap();
            assert_eq!(result, expected);
        }
    }
}
