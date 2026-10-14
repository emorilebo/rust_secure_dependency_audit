//! Fetch metadata from crates.io

use crate::config::NetworkConfig;
use crate::error::{AuditError, Result};
use crate::metadata::http::{self, FetchFailure, Pacer};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::OnceCell;
use tracing::{debug, warn};

const CRATES_IO_API: &str = "https://crates.io/api/v1";

/// The crates.io data-access policy allows at most one API request per second.
pub(crate) const CRATES_IO_MIN_INTERVAL: Duration = Duration::from_secs(1);

/// Metadata from crates.io for one version of a crate
#[derive(Debug, Clone)]
pub struct CrateMetadata {
    /// License of the audited version (`None` if that version is not listed)
    pub license: Option<String>,
    pub repository: Option<String>,
    pub downloads: u64,
    /// When the newest version of the crate (any version) was published
    pub last_published: DateTime<Utc>,
    pub version_count: u32,
    /// Distinct crates.io accounts that published any version
    pub publisher_count: u32,
    pub is_yanked: bool,
}

#[derive(Debug, Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    crate_info: CrateInfo,
    #[serde(default)]
    versions: Option<Vec<VersionInfo>>,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    repository: Option<String>,
    #[serde(default)]
    downloads: u64,
}

#[derive(Debug, Deserialize)]
struct VersionInfo {
    #[serde(rename = "num")]
    version: String,
    license: Option<String>,
    created_at: DateTime<Utc>,
    #[serde(default)]
    yanked: bool,
    #[serde(default)]
    published_by: Option<Publisher>,
}

#[derive(Debug, Deserialize)]
struct Publisher {
    login: String,
}

type CachedResponse = std::result::Result<Arc<CratesIoResponse>, FetchFailure>;

/// crates.io API client shared by every dependency in one audit run.
///
/// Requests are paced to at most one per second (or slower, if
/// `request_delay_ms` asks for it) and responses are cached per crate name,
/// so several versions of the same crate cost a single request.
pub struct CratesIoClient {
    client: Client,
    api_base: String,
    pacer: Pacer,
    max_retries: u32,
    retry_delay: Duration,
    cache: Mutex<HashMap<String, Arc<OnceCell<CachedResponse>>>>,
}

impl CratesIoClient {
    pub fn new(config: &NetworkConfig) -> Result<Self> {
        let interval = config.request_delay().max(CRATES_IO_MIN_INTERVAL);
        Self::with_base_url(config, CRATES_IO_API, interval)
    }

    pub(crate) fn with_base_url(
        config: &NetworkConfig,
        api_base: &str,
        interval: Duration,
    ) -> Result<Self> {
        Ok(Self {
            client: http::build(http::client_builder(config.timeout()))?,
            api_base: api_base.trim_end_matches('/').to_string(),
            pacer: Pacer::new(interval),
            max_retries: config.max_retries,
            retry_delay: config.request_delay(),
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// Fetch metadata for `crate_name`, with version-specific fields taken
    /// from `version`.
    pub async fn fetch(&self, crate_name: &str, version: &str) -> Result<CrateMetadata> {
        let cell = {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.entry(crate_name.to_string()).or_default().clone()
        };

        let response = cell
            .get_or_init(|| self.fetch_uncached(crate_name))
            .await
            .clone()
            .map_err(|failure| failure.into_audit_error("crates.io", crate_name))?;

        build_metadata(&response, version)
    }

    async fn fetch_uncached(&self, crate_name: &str) -> CachedResponse {
        debug!("Fetching crates.io metadata for {}", crate_name);
        let url = format!("{}/crates/{}", self.api_base, crate_name);
        let mut attempt = 0;
        let mut delay = self.retry_delay;

        loop {
            self.pacer.wait().await;
            let wait = match http::get(&self.client, &url).await {
                Ok(fetched) if fetched.status.is_success() => {
                    return serde_json::from_slice::<CratesIoResponse>(&fetched.body)
                        .map(Arc::new)
                        .map_err(|e| FetchFailure::Other(format!("invalid response: {}", e)));
                }
                Ok(fetched) if fetched.status == StatusCode::NOT_FOUND => {
                    return Err(FetchFailure::NotFound);
                }
                Ok(fetched) => {
                    let status = fetched.status;
                    let retryable =
                        status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
                    let server_wait = http::retry_after(&fetched.headers);
                    if !retryable || attempt >= self.max_retries {
                        return Err(if status == StatusCode::TOO_MANY_REQUESTS {
                            FetchFailure::RateLimited {
                                retry_after: server_wait,
                            }
                        } else {
                            FetchFailure::Other(format!("HTTP {}", status))
                        });
                    }
                    warn!(
                        "crates.io returned HTTP {} for {}, retrying",
                        status, crate_name
                    );
                    server_wait.unwrap_or(delay).min(http::MAX_RETRY_WAIT)
                }
                Err(e) => {
                    if attempt >= self.max_retries {
                        return Err(FetchFailure::Other(format!("request failed: {}", e)));
                    }
                    warn!(
                        "crates.io request for {} failed, retrying: {}",
                        crate_name, e
                    );
                    delay
                }
            };

            tokio::time::sleep(wait).await;
            attempt += 1;
            delay *= 2;
        }
    }
}

fn build_metadata(response: &CratesIoResponse, version: &str) -> Result<CrateMetadata> {
    let versions = response.versions.as_deref().unwrap_or_default();

    let last_published = versions
        .iter()
        .map(|v| v.created_at)
        .max()
        .ok_or_else(|| AuditError::parse("No versions found for crate"))?;

    let publishers: HashSet<&str> = versions
        .iter()
        .filter_map(|v| v.published_by.as_ref())
        .map(|p| p.login.as_str())
        .collect();

    let audited = versions.iter().find(|v| v.version == version);

    Ok(CrateMetadata {
        license: audited.and_then(|v| v.license.clone()),
        repository: response.crate_info.repository.clone(),
        downloads: response.crate_info.downloads,
        last_published,
        version_count: versions.len() as u32,
        publisher_count: publishers.len() as u32,
        is_yanked: audited.map(|v| v.yanked).unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "crate": {"name": "demo", "repository": "https://github.com/o/demo", "downloads": 1234},
        "versions": [
            {"num": "1.1.0", "license": "MIT", "created_at": "2026-01-02T00:00:00Z",
             "yanked": false, "published_by": {"login": "alice"}},
            {"num": "1.0.1", "license": "MIT OR Apache-2.0", "created_at": "2025-06-01T00:00:00Z",
             "yanked": true, "published_by": {"login": "bob"}},
            {"num": "1.0.0", "license": "MIT", "created_at": "2025-01-01T00:00:00Z",
             "yanked": false, "published_by": {"login": "alice"}}
        ]
    }"#;

    fn test_config() -> NetworkConfig {
        NetworkConfig {
            max_retries: 1,
            request_delay_ms: 1,
            ..NetworkConfig::default()
        }
    }

    #[test]
    fn metadata_uses_audited_version_and_newest_publish_date() {
        let response: CratesIoResponse = serde_json::from_str(SAMPLE).unwrap();
        let meta = build_metadata(&response, "1.0.1").unwrap();

        assert_eq!(meta.license.as_deref(), Some("MIT OR Apache-2.0"));
        assert!(meta.is_yanked);
        assert_eq!(meta.version_count, 3);
        // crates.io no longer returns `authors`; publishers are the signal.
        assert_eq!(meta.publisher_count, 2);
        assert_eq!(
            meta.last_published.to_rfc3339(),
            "2026-01-02T00:00:00+00:00"
        );
    }

    #[test]
    fn unknown_version_is_not_reported_as_yanked() {
        let response: CratesIoResponse = serde_json::from_str(SAMPLE).unwrap();
        let meta = build_metadata(&response, "9.9.9").unwrap();
        assert!(!meta.is_yanked);
        assert_eq!(meta.license, None);
    }

    #[tokio::test]
    async fn same_crate_is_fetched_once() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/crates/demo")
            .with_status(200)
            .with_body(SAMPLE)
            .expect(1)
            .create_async()
            .await;

        let client =
            CratesIoClient::with_base_url(&test_config(), &server.url(), Duration::ZERO).unwrap();
        let (a, b) = tokio::join!(client.fetch("demo", "1.0.0"), client.fetch("demo", "1.1.0"));
        assert!(!a.unwrap().is_yanked);
        assert_eq!(b.unwrap().license.as_deref(), Some("MIT"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn too_many_requests_is_retried_then_reported_as_rate_limit() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/crates/busy")
            .with_status(429)
            .with_header("retry-after", "0")
            .expect(2)
            .create_async()
            .await;

        let client =
            CratesIoClient::with_base_url(&test_config(), &server.url(), Duration::ZERO).unwrap();
        let err = client.fetch("busy", "1.0.0").await.unwrap_err();
        assert!(matches!(err, AuditError::RateLimitExceeded { .. }), "{err}");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn truncated_body_is_retried() {
        // Observed for real: a large crates.io response (serde lists 300+
        // versions) timed out half-way through the body, and the crate was
        // reported as unscorable without a retry.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0u8; 1024];
                let _ = socket.read(&mut request).await.unwrap();
                let response = if attempt == 0 {
                    // Promise more bytes than we send, then hang up.
                    "HTTP/1.1 200 OK\r\ncontent-length: 5000\r\n\r\n{\"crate\"".to_string()
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        SAMPLE.len(),
                        SAMPLE
                    )
                };
                socket.write_all(response.as_bytes()).await.unwrap();
                socket.shutdown().await.unwrap();
            }
        });

        let client = CratesIoClient::with_base_url(&test_config(), &base, Duration::ZERO).unwrap();
        let meta = client.fetch("demo", "1.1.0").await.unwrap();
        assert_eq!(meta.version_count, 3);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn missing_crate_is_not_found() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/crates/nope")
            .with_status(404)
            .create_async()
            .await;

        let client =
            CratesIoClient::with_base_url(&test_config(), &server.url(), Duration::ZERO).unwrap();
        let err = client.fetch("nope", "1.0.0").await.unwrap_err();
        assert!(matches!(err, AuditError::DependencyNotFound(_)), "{err}");
    }

    #[tokio::test]
    #[ignore = "requires network access to crates.io"]
    async fn live_fetch_serde() {
        let client = CratesIoClient::new(&NetworkConfig::default()).unwrap();
        let meta = client.fetch("serde", "1.0.0").await.unwrap();
        assert!(meta.repository.unwrap().contains("serde-rs/serde"));
        assert!(meta.version_count > 100);
        assert!(meta.publisher_count >= 1);
    }
}
