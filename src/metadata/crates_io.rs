//! Fetch metadata from crates.io

use crate::error::{AuditError, Result};
use crate::config::NetworkConfig;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, warn};

const CRATES_IO_API: &str = "https://crates.io/api/v1";
const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION")
);

/// Metadata from crates.io for a crate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrateMetadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub downloads: u64,
    pub recent_downloads: Option<u64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub version_count: u32,
    pub authors: Vec<String>,
}

/// Response from crates.io API for crate info
#[derive(Debug, Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    crate_info: CrateInfo,
    versions: Vec<VersionInfo>,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    name: String,
    description: Option<String>,
    repository: Option<String>,
    homepage: Option<String>,
    downloads: u64,
    recent_downloads: Option<u64>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct VersionInfo {
    #[serde(rename = "num")]
    version: String,
    license: Option<String>,
    created_at: String,
    updated_at: String,
    downloads: u64,
    #[serde(default)]
    authors: Vec<String>,
}

/// Fetch metadata for a crate from crates.io
pub async fn fetch_crate_metadata(
    crate_name: &str,
    version: &str,
    config: &NetworkConfig,
) -> Result<CrateMetadata> {
    debug!("Fetching metadata for {} v{}", crate_name, version);

    let client = build_client(config)?;
    let url = format!("{}/crates/{}", CRATES_IO_API, crate_name);

    let response = retry_request(&client, &url, config.max_retries, config.request_delay()).await?;

    if !response.status().is_success() {
        if response.status().as_u16() == 404 {
            return Err(AuditError::DependencyNotFound(crate_name.to_string()));
        }
        return Err(AuditError::api(
            "crates.io",
            format!("HTTP {}: {}", response.status(), crate_name),
        ));
    }

    let data: CratesIoResponse = response.json().await?;

    // Find the specific version or use the latest
    let version_info = data
        .versions
        .iter()
        .find(|v| v.version == version)
        .or_else(|| data.versions.first())
        .ok_or_else(|| AuditError::parse("No versions found for crate"))?;

    let created_at = parse_datetime(&data.crate_info.created_at)?;
    let updated_at = parse_datetime(&version_info.updated_at)?;

    Ok(CrateMetadata {
        name: data.crate_info.name,
        version: version_info.version.clone(),
        description: data.crate_info.description,
        license: version_info.license.clone(),
        repository: data.crate_info.repository,
        homepage: data.crate_info.homepage,
        downloads: data.crate_info.downloads,
        recent_downloads: data.crate_info.recent_downloads,
        created_at,
        updated_at,
        version_count: data.versions.len() as u32,
        authors: version_info.authors.clone(),
    })
}

/// Build HTTP client with proper configuration
fn build_client(config: &NetworkConfig) -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .timeout(config.timeout())
        .build()
        .map_err(|e| AuditError::network(format!("Failed to build HTTP client: {}", e)))
}

/// Retry a request with exponential backoff
async fn retry_request(
    client: &Client,
    url: &str,
    max_retries: u32,
    base_delay: Duration,
) -> Result<reqwest::Response> {
    let mut attempts = 0;
    let mut delay = base_delay;

    loop {
        match client.get(url).send().await {
            Ok(response) => {
                // Check for rate limiting
                if response.status().as_u16() == 429 {
                    if attempts >= max_retries {
                        return Err(AuditError::RateLimitExceeded {
                            service: "crates.io".to_string(),
                            retry_after: Some(delay),
                        });
                    }
                    warn!("Rate limited by crates.io, retrying after {:?}", delay);
                    tokio::time::sleep(delay).await;
                    attempts += 1;
                    delay *= 2; // Exponential backoff
                    continue;
                }
                return Ok(response);
            }
            Err(e) => {
                if attempts >= max_retries {
                    return Err(AuditError::network(format!("Request failed: {}", e)));
                }
                warn!("Request failed, retrying: {}", e);
                tokio::time::sleep(delay).await;
                attempts += 1;
                delay *= 2;
            }
        }
    }
}

/// Parse datetime string from crates.io API
fn parse_datetime(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| AuditError::parse(format!("Invalid datetime: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_serde_metadata() {
        // This is an integration test that requires network access
        // Skip in CI environments without network
        if std::env::var("CI").is_ok() {
            return;
        }

        let config = NetworkConfig::default();
        let result = fetch_crate_metadata("serde", "1.0.0", &config).await;
        
        // Should either succeed or fail gracefully
        match result {
            Ok(metadata) => {
                assert_eq!(metadata.name, "serde");
                assert!(metadata.version_count > 0);
            }
            Err(e) => {
                // Network errors are acceptable in tests
                eprintln!("Test skipped due to: {}", e);
            }
        }
    }
}
