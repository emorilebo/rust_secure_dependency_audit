//! Fetch metadata from GitHub repositories
//!
//! Unauthenticated GitHub API access is limited to 60 requests per hour, so
//! the client is built to degrade gracefully:
//!
//! - repository lookups are cached per `owner/repo`, so crates that share a
//!   repository (e.g. the `windows-*` family) cost one lookup;
//! - each repository costs at most two API requests (repository +
//!   contributors); `SECURITY.md` detection uses `raw.githubusercontent.com`,
//!   which does not count against the API quota;
//! - once GitHub reports an exhausted rate limit (or rejects the token), the
//!   client stops calling the API for the rest of the run and every later
//!   lookup fails fast with the same error.

use crate::config::NetworkConfig;
use crate::error::{AuditError, Result};
use crate::metadata::http::{self, FetchFailure, Fetched};
use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, LINK};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

const GITHUB_API: &str = "https://api.github.com";
const GITHUB_RAW: &str = "https://raw.githubusercontent.com";

/// Locations GitHub recognises for a security policy.
const SECURITY_POLICY_PATHS: [&str; 3] = ["SECURITY.md", ".github/SECURITY.md", "docs/SECURITY.md"];

/// Metadata from GitHub for a repository
#[derive(Debug, Clone)]
pub struct GitHubMetadata {
    pub stars: u32,
    pub open_issues: u32,
    pub is_archived: bool,
    pub pushed_at: DateTime<Utc>,
    /// `None` when the contributor count could not be determined
    pub contributors_count: Option<u32>,
    /// `None` when the presence of a policy could not be determined
    pub has_security_policy: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GitHubRepo {
    stargazers_count: u32,
    open_issues_count: u32,
    archived: bool,
    pushed_at: Option<DateTime<Utc>>,
    updated_at: DateTime<Utc>,
    default_branch: Option<String>,
}

/// Why the client stopped talking to the GitHub API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubOutage {
    /// The rate limit is exhausted; `resets_at` is when GitHub said it resets
    RateLimited { resets_at: Option<SystemTime> },
    /// The configured token was rejected (HTTP 401)
    BadCredentials,
}

type CachedRepo = std::result::Result<GitHubMetadata, FetchFailure>;

/// GitHub client shared by every dependency in one audit run.
pub struct GitHubClient {
    api: Client,
    raw: Client,
    api_base: String,
    raw_base: String,
    authenticated: bool,
    max_retries: u32,
    retry_delay: Duration,
    outage: Mutex<Option<GitHubOutage>>,
    repos: Mutex<HashMap<String, Arc<OnceCell<CachedRepo>>>>,
    owner_policies: Mutex<HashMap<String, Arc<OnceCell<Option<bool>>>>>,
}

impl GitHubClient {
    pub fn new(config: &NetworkConfig) -> Result<Self> {
        Self::with_base_urls(config, GITHUB_API, GITHUB_RAW)
    }

    pub(crate) fn with_base_urls(
        config: &NetworkConfig,
        api_base: &str,
        raw_base: &str,
    ) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github+json"),
        );

        let mut authenticated = false;
        if let Some(token) = config.github_token.as_deref().map(str::trim) {
            match HeaderValue::from_str(&format!("Bearer {}", token)) {
                Ok(mut value) if !token.is_empty() => {
                    value.set_sensitive(true);
                    headers.insert(AUTHORIZATION, value);
                    authenticated = true;
                }
                Ok(_) => {}
                Err(_) => warn!(
                    "GITHUB_TOKEN contains characters that are not valid in an HTTP header; \
                     continuing without GitHub authentication"
                ),
            }
        }

        Ok(Self {
            api: http::build(http::client_builder(config.timeout()).default_headers(headers))?,
            raw: http::build(http::client_builder(config.timeout()))?,
            api_base: api_base.trim_end_matches('/').to_string(),
            raw_base: raw_base.trim_end_matches('/').to_string(),
            authenticated,
            max_retries: config.max_retries,
            retry_delay: config.request_delay(),
            outage: Mutex::new(None),
            repos: Mutex::new(HashMap::new()),
            owner_policies: Mutex::new(HashMap::new()),
        })
    }

    /// Whether requests carry a token.
    pub fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    /// Set once the client has stopped calling the GitHub API.
    pub fn outage(&self) -> Option<GitHubOutage> {
        *self.outage.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Fetch metadata for the GitHub repository at `repo_url`.
    pub async fn fetch(&self, repo_url: &str) -> Result<GitHubMetadata> {
        let (owner, repo) = parse_github_url(repo_url)?;
        let key = format!("{}/{}", owner.to_lowercase(), repo.to_lowercase());

        let cell = {
            let mut repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
            repos.entry(key.clone()).or_default().clone()
        };

        cell.get_or_init(|| self.fetch_uncached(&owner, &repo))
            .await
            .clone()
            .map_err(|failure| failure.into_audit_error("GitHub", &key))
    }

    async fn fetch_uncached(&self, owner: &str, repo: &str) -> CachedRepo {
        debug!("Fetching GitHub metadata for {}/{}", owner, repo);
        let url = format!("{}/repos/{}/{}", self.api_base, owner, repo);
        let response = self.api_get(&url).await?;

        if response.status == StatusCode::NOT_FOUND {
            return Err(FetchFailure::NotFound);
        }
        if !response.status.is_success() {
            return Err(FetchFailure::Other(format!("HTTP {}", response.status)));
        }

        let data: GitHubRepo = serde_json::from_slice(&response.body)
            .map_err(|e| FetchFailure::Other(format!("invalid response: {}", e)))?;

        let contributors_count = self.contributors_count(owner, repo).await;
        let has_security_policy = self
            .security_policy(owner, repo, data.default_branch.as_deref())
            .await;

        Ok(GitHubMetadata {
            stars: data.stargazers_count,
            open_issues: data.open_issues_count,
            is_archived: data.archived,
            pushed_at: data.pushed_at.unwrap_or(data.updated_at),
            contributors_count,
            has_security_policy,
        })
    }

    /// GET against the API, honouring (and recording) rate limits.
    async fn api_get(&self, url: &str) -> std::result::Result<Fetched, FetchFailure> {
        let mut attempt = 0;
        let mut delay = self.retry_delay;

        loop {
            if let Some(outage) = self.outage() {
                return Err(outage_failure(outage));
            }

            match http::get(&self.api, url).await {
                Ok(response) => {
                    let status = response.status;
                    if let Some(retry_after) =
                        http::rate_limit_wait(status, &response.headers, SystemTime::now())
                    {
                        let outage = GitHubOutage::RateLimited {
                            resets_at: retry_after.map(|wait| SystemTime::now() + wait),
                        };
                        self.record_outage(outage);
                        return Err(outage_failure(outage));
                    }
                    if status == StatusCode::UNAUTHORIZED && self.authenticated {
                        self.record_outage(GitHubOutage::BadCredentials);
                        return Err(FetchFailure::Unauthorized);
                    }
                    return Ok(response);
                }
                Err(e) if attempt < self.max_retries => {
                    warn!("GitHub request failed, retrying: {}", e);
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    delay *= 2;
                }
                Err(e) => return Err(FetchFailure::Other(format!("request failed: {}", e))),
            }
        }
    }

    fn record_outage(&self, outage: GitHubOutage) {
        let mut current = self.outage.lock().unwrap_or_else(|e| e.into_inner());
        if current.is_none() {
            // Surfaced to users as a report notice; logged here for tracing.
            info!(
                "{}; skipping GitHub lookups for the rest of this run",
                describe_outage(outage, self.authenticated)
            );
            *current = Some(outage);
        }
    }

    /// Contributor count from the `Link: rel="last"` page number.
    async fn contributors_count(&self, owner: &str, repo: &str) -> Option<u32> {
        let url = format!(
            "{}/repos/{}/{}/contributors?per_page=1",
            self.api_base, owner, repo
        );
        let response = self.api_get(&url).await.ok()?;

        match response.status {
            StatusCode::NO_CONTENT => Some(0),
            status if status.is_success() => {
                let from_link = response
                    .headers
                    .get(LINK)
                    .and_then(|value| value.to_str().ok())
                    .and_then(extract_last_page);
                match from_link {
                    Some(count) => Some(count),
                    None => serde_json::from_slice::<Vec<serde_json::Value>>(&response.body)
                        .ok()
                        .map(|items| items.len() as u32),
                }
            }
            // e.g. 403 "contributor list is too large" for huge repositories
            _ => None,
        }
    }

    /// `Some(true)` if the repository (or the owner's `.github` repository)
    /// has a `SECURITY.md`, `Some(false)` if none was found, `None` if it
    /// could not be determined.
    async fn security_policy(
        &self,
        owner: &str,
        repo: &str,
        default_branch: Option<&str>,
    ) -> Option<bool> {
        let branch = default_branch.unwrap_or("HEAD");
        match self.any_file_exists(owner, repo, branch).await? {
            true => Some(true),
            false => self.owner_default_policy(owner).await,
        }
    }

    /// GitHub applies an owner-wide `SECURITY.md` from the `<owner>/.github`
    /// repository to repositories that have none of their own.
    async fn owner_default_policy(&self, owner: &str) -> Option<bool> {
        let cell = {
            let mut owners = self
                .owner_policies
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            owners.entry(owner.to_lowercase()).or_default().clone()
        };
        *cell
            .get_or_init(|| self.any_file_exists(owner, ".github", "HEAD"))
            .await
    }

    async fn any_file_exists(&self, owner: &str, repo: &str, branch: &str) -> Option<bool> {
        for path in SECURITY_POLICY_PATHS {
            let url = format!("{}/{}/{}/{}/{}", self.raw_base, owner, repo, branch, path);
            match self.raw.head(&url).send().await {
                Ok(response) if response.status().is_success() => return Some(true),
                Ok(response) if response.status() == StatusCode::NOT_FOUND => continue,
                Ok(response) => {
                    debug!("Unexpected HTTP {} checking {}", response.status(), url);
                    return None;
                }
                Err(e) => {
                    debug!("Could not check {}: {}", url, e);
                    return None;
                }
            }
        }
        Some(false)
    }
}

fn outage_failure(outage: GitHubOutage) -> FetchFailure {
    match outage {
        GitHubOutage::RateLimited { resets_at } => FetchFailure::RateLimited {
            retry_after: resets_at.and_then(|reset| reset.duration_since(SystemTime::now()).ok()),
        },
        GitHubOutage::BadCredentials => FetchFailure::Unauthorized,
    }
}

/// One-line, user-facing explanation of a GitHub outage.
pub fn describe_outage(outage: GitHubOutage, authenticated: bool) -> String {
    match outage {
        GitHubOutage::RateLimited { resets_at } => {
            let quota = if authenticated {
                "authenticated"
            } else {
                "unauthenticated (60 requests/hour)"
            };
            let reset = resets_at
                .and_then(|reset| reset.duration_since(SystemTime::now()).ok())
                .map(|wait| format!(", resets in about {}", http::format_wait(wait)))
                .unwrap_or_default();
            format!("GitHub API rate limit reached ({} quota{})", quota, reset)
        }
        GitHubOutage::BadCredentials => {
            "GitHub rejected the configured token (HTTP 401 Bad credentials)".to_string()
        }
    }
}

/// Parse GitHub URL to extract owner and repo name
pub(crate) fn parse_github_url(url: &str) -> Result<(String, String)> {
    // Handle various GitHub URL formats:
    // - https://github.com/owner/repo
    // - https://github.com/owner/repo.git
    // - https://github.com/owner/repo/tree/main/subcrate
    // - git://github.com/owner/repo
    // - git@github.com:owner/repo.git
    let path = url
        .split_once("github.com:")
        .or_else(|| url.split_once("github.com/"))
        .map(|(_, path)| path)
        .ok_or_else(|| AuditError::parse(format!("Invalid GitHub URL: {}", url)))?;

    let path = path.split(['?', '#']).next().unwrap_or_default();
    let mut segments = path.split('/').filter(|segment| !segment.is_empty());

    match (segments.next(), segments.next()) {
        (Some(owner), Some(repo)) => {
            let repo = repo.strip_suffix(".git").unwrap_or(repo);
            if repo.is_empty() {
                return Err(AuditError::parse(format!("Invalid GitHub URL: {}", url)));
            }
            Ok((owner.to_string(), repo.to_string()))
        }
        _ => Err(AuditError::parse(format!("Invalid GitHub URL: {}", url))),
    }
}

/// Extract the `page` query parameter of the `rel="last"` link.
fn extract_last_page(link_header: &str) -> Option<u32> {
    let last = link_header
        .split(',')
        .find(|link| link.contains("rel=\"last\""))?;
    let url = last.split(['<', '>']).nth(1)?;
    let (_, query) = url.split_once('?')?;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("page="))
        .and_then(|page| page.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_github_url() {
        let test_cases = vec![
            ("https://github.com/serde-rs/serde", ("serde-rs", "serde")),
            (
                "https://github.com/serde-rs/serde.git",
                ("serde-rs", "serde"),
            ),
            ("https://github.com/serde-rs/serde/", ("serde-rs", "serde")),
            ("git://github.com/serde-rs/serde", ("serde-rs", "serde")),
            ("git@github.com:serde-rs/serde.git", ("serde-rs", "serde")),
            (
                "https://github.com/rust-lang/cargo/tree/master/crates/home",
                ("rust-lang", "cargo"),
            ),
            ("https://github.com/o/r?branch=main#abc123", ("o", "r")),
            ("https://github.com/o/r#abc123", ("o", "r")),
        ];

        for (url, expected) in test_cases {
            let result = parse_github_url(url).unwrap();
            assert_eq!(
                result,
                (expected.0.to_string(), expected.1.to_string()),
                "{url}"
            );
        }

        assert!(parse_github_url("https://github.com/only-owner").is_err());
        assert!(parse_github_url("https://gitlab.com/a/b").is_err());
    }

    #[test]
    fn test_extract_last_page() {
        let link_header = r#"<https://api.github.com/repos/rust-lang/rust/contributors?page=2>; rel="next", <https://api.github.com/repos/rust-lang/rust/contributors?page=50>; rel="last""#;
        assert_eq!(extract_last_page(link_header), Some(50));
    }

    #[test]
    fn extract_last_page_ignores_per_page() {
        // Real header shape for `?per_page=1`: the old parser matched the
        // `page=` inside `per_page=` and always fell back to a count of 1.
        let link_header = r#"<https://api.github.com/repositories/1/contributors?per_page=1&page=2>; rel="next", <https://api.github.com/repositories/1/contributors?per_page=1&page=187>; rel="last""#;
        assert_eq!(extract_last_page(link_header), Some(187));
    }

    fn test_config(token: Option<&str>) -> NetworkConfig {
        NetworkConfig {
            max_retries: 0,
            request_delay_ms: 1,
            github_token: token.map(String::from),
            ..NetworkConfig::default()
        }
    }

    const REPO_JSON: &str = r#"{"stargazers_count": 1500, "open_issues_count": 3, "archived": false,
        "pushed_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-01T00:00:00Z",
        "default_branch": "main"}"#;

    #[tokio::test]
    async fn rate_limit_trips_breaker_and_stops_calling_github() {
        let mut server = mockito::Server::new_async().await;
        let limited = server
            .mock("GET", mockito::Matcher::Regex(r"^/repos/.*".to_string()))
            .with_status(403)
            .with_header("x-ratelimit-remaining", "0")
            .with_header("x-ratelimit-reset", "4102444800")
            .with_body(r#"{"message": "API rate limit exceeded"}"#)
            .expect(1)
            .create_async()
            .await;

        let client =
            GitHubClient::with_base_urls(&test_config(None), &server.url(), &server.url()).unwrap();

        let first = client.fetch("https://github.com/a/one").await.unwrap_err();
        let second = client.fetch("https://github.com/b/two").await.unwrap_err();

        assert!(
            matches!(first, AuditError::RateLimitExceeded { .. }),
            "{first}"
        );
        assert!(
            matches!(second, AuditError::RateLimitExceeded { .. }),
            "{second}"
        );
        assert!(matches!(
            client.outage(),
            Some(GitHubOutage::RateLimited { resets_at: Some(_) })
        ));
        // Only the first lookup reached the server.
        limited.assert_async().await;
    }

    #[tokio::test]
    async fn plain_forbidden_does_not_trip_breaker() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/repos/a/blocked")
            .with_status(403)
            .with_header("x-ratelimit-remaining", "55")
            .create_async()
            .await;

        let client =
            GitHubClient::with_base_urls(&test_config(None), &server.url(), &server.url()).unwrap();
        let err = client
            .fetch("https://github.com/a/blocked")
            .await
            .unwrap_err();

        assert!(matches!(err, AuditError::ApiError { .. }), "{err}");
        assert_eq!(client.outage(), None);
    }

    #[tokio::test]
    async fn shared_repository_is_fetched_once_and_signals_are_parsed() {
        let mut server = mockito::Server::new_async().await;
        let repo = server
            .mock("GET", "/repos/o/r")
            .with_status(200)
            .with_body(REPO_JSON)
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/repos/o/r/contributors?per_page=1")
            .with_status(200)
            .with_header(
                "link",
                r#"<https://api.github.com/repositories/1/contributors?per_page=1&page=2>; rel="next", <https://api.github.com/repositories/1/contributors?per_page=1&page=42>; rel="last""#,
            )
            .with_body("[{}]")
            .create_async()
            .await;
        server
            .mock("HEAD", "/o/r/main/SECURITY.md")
            .with_status(200)
            .create_async()
            .await;

        let client =
            GitHubClient::with_base_urls(&test_config(None), &server.url(), &server.url()).unwrap();

        let (a, b) = tokio::join!(
            client.fetch("https://github.com/o/r"),
            client.fetch("https://github.com/O/R.git")
        );
        let meta = a.unwrap();
        assert_eq!(b.unwrap().stars, 1500);
        assert_eq!(meta.contributors_count, Some(42));
        assert_eq!(meta.has_security_policy, Some(true));
        repo.assert_async().await;
    }

    #[tokio::test]
    async fn security_policy_falls_back_to_owner_dot_github_repo() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/repos/o/r")
            .with_status(200)
            .with_body(REPO_JSON)
            .create_async()
            .await;
        server
            .mock("GET", "/repos/o/r/contributors?per_page=1")
            .with_status(500)
            .create_async()
            .await;
        server
            .mock(
                "HEAD",
                mockito::Matcher::Regex(r"^/o/r/main/.*".to_string()),
            )
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("HEAD", "/o/.github/HEAD/SECURITY.md")
            .with_status(200)
            .create_async()
            .await;

        let client =
            GitHubClient::with_base_urls(&test_config(None), &server.url(), &server.url()).unwrap();
        let meta = client.fetch("https://github.com/o/r").await.unwrap();

        assert_eq!(meta.has_security_policy, Some(true));
        // A failed contributors call is "unknown", not "zero contributors".
        assert_eq!(meta.contributors_count, None);
    }

    #[tokio::test]
    async fn unreachable_raw_host_means_unknown_policy_not_missing() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/repos/o/r")
            .with_status(200)
            .with_body(REPO_JSON)
            .create_async()
            .await;
        server
            .mock("GET", "/repos/o/r/contributors?per_page=1")
            .with_status(200)
            .with_body("[{}]")
            .create_async()
            .await;

        // Raw checks go to a port nothing listens on.
        let client =
            GitHubClient::with_base_urls(&test_config(None), &server.url(), "http://127.0.0.1:9")
                .unwrap();
        let meta = client.fetch("https://github.com/o/r").await.unwrap();

        assert_eq!(meta.has_security_policy, None);
        assert_eq!(meta.contributors_count, Some(1));
    }

    #[tokio::test]
    async fn rejected_token_trips_breaker() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", mockito::Matcher::Regex(r"^/repos/.*".to_string()))
            .with_status(401)
            .expect(1)
            .create_async()
            .await;

        let client = GitHubClient::with_base_urls(
            &test_config(Some("ghp_invalid")),
            &server.url(),
            &server.url(),
        )
        .unwrap();
        assert!(client.is_authenticated());
        assert!(client.fetch("https://github.com/a/one").await.is_err());
        assert!(client.fetch("https://github.com/b/two").await.is_err());
        assert_eq!(client.outage(), Some(GitHubOutage::BadCredentials));
        mock.assert_async().await;
    }

    #[test]
    fn token_with_newline_does_not_panic() {
        // `GITHUB_TOKEN=$(cat token.txt)` style values often end in a newline;
        // they used to panic inside a spawned task and silently drop the
        // dependency from the report.
        let client = GitHubClient::new(&test_config(Some("ghp_abc\n"))).unwrap();
        assert!(client.is_authenticated());

        let client = GitHubClient::new(&test_config(Some("bad\u{7}token"))).unwrap();
        assert!(!client.is_authenticated());
    }
}
