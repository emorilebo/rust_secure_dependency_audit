//! Shared HTTP plumbing: User-Agent, request pacing, rate-limit detection,
//! and a small cacheable failure type used by the per-service clients.

use crate::error::{AuditError, Result};
use reqwest::header::HeaderMap;
use reqwest::{Client, StatusCode};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Identifies the tool and gives API operators a way to reach the maintainers,
/// as requested by the crates.io data-access policy.
pub(crate) const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " (+",
    env!("CARGO_PKG_REPOSITORY"),
    ")"
);

/// Longest server-requested back-off we are willing to sleep through before
/// giving up on a request.
pub(crate) const MAX_RETRY_WAIT: Duration = Duration::from_secs(60);

/// Build an HTTP client with the crate's User-Agent and the configured timeout.
pub(crate) fn client_builder(timeout: Duration) -> reqwest::ClientBuilder {
    Client::builder().user_agent(USER_AGENT).timeout(timeout)
}

/// Convert a `reqwest` builder error into an [`AuditError`].
pub(crate) fn build(builder: reqwest::ClientBuilder) -> Result<Client> {
    builder
        .build()
        .map_err(|e| AuditError::network(format!("Failed to build HTTP client: {}", e)))
}

/// A fully read HTTP response.
pub(crate) struct Fetched {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

/// GET `url` and read the whole body. Connection errors *and* errors while
/// reading the body (e.g. a timeout half-way through a large response) are
/// returned as `Err`, so callers can retry both the same way.
pub(crate) async fn get(client: &Client, url: &str) -> reqwest::Result<Fetched> {
    let response = client.get(url).send().await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.bytes().await?.to_vec();
    Ok(Fetched {
        status,
        headers,
        body,
    })
}

/// Serializes requests so that consecutive requests start at least
/// `interval` apart, across all tasks sharing the pacer.
pub(crate) struct Pacer {
    interval: Duration,
    next_slot: Mutex<Option<Instant>>,
}

impl Pacer {
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_slot: Mutex::new(None),
        }
    }

    /// Wait until the next request slot is available and reserve it.
    pub(crate) async fn wait(&self) {
        let mut next_slot = self.next_slot.lock().await;
        if let Some(slot) = *next_slot {
            tokio::time::sleep_until(slot).await;
        }
        *next_slot = Some(Instant::now() + self.interval);
    }
}

/// A failed fetch, in a form that can be cached and shared between the
/// dependencies that point at the same crate or repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FetchFailure {
    /// HTTP 404
    NotFound,
    /// The service's rate limit is exhausted; `retry_after` is when it resets
    RateLimited { retry_after: Option<Duration> },
    /// The service rejected our credentials (HTTP 401)
    Unauthorized,
    /// Anything else (transport error, unexpected status, bad JSON)
    Other(String),
}

impl FetchFailure {
    pub(crate) fn into_audit_error(self, service: &str, subject: &str) -> AuditError {
        match self {
            Self::NotFound => AuditError::DependencyNotFound(format!("{} ({})", subject, service)),
            Self::RateLimited { retry_after } => AuditError::RateLimitExceeded {
                service: service.to_string(),
                retry_after,
            },
            Self::Unauthorized => AuditError::api(
                service,
                "credentials rejected (HTTP 401); check the configured token",
            ),
            Self::Other(message) => AuditError::api(service, message),
        }
    }
}

/// Decide whether a response means "rate limited".
///
/// GitHub signals its primary rate limit with `403` or `429` plus
/// `x-ratelimit-remaining: 0`, and secondary limits with a `retry-after`
/// header. A bare `403` is a permission problem, not a rate limit.
/// Returns `Some(retry_after)` when rate limited.
pub(crate) fn rate_limit_wait(
    status: StatusCode,
    headers: &HeaderMap,
    now: SystemTime,
) -> Option<Option<Duration>> {
    if status != StatusCode::FORBIDDEN && status != StatusCode::TOO_MANY_REQUESTS {
        return None;
    }

    let retry_after = header_u64(headers, "retry-after").map(Duration::from_secs);
    let exhausted = headers
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim() == "0")
        .unwrap_or(false);

    if !(exhausted || retry_after.is_some() || status == StatusCode::TOO_MANY_REQUESTS) {
        return None;
    }

    let until_reset = header_u64(headers, "x-ratelimit-reset").map(|reset_epoch| {
        let now_epoch = now
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0);
        Duration::from_secs(reset_epoch.saturating_sub(now_epoch))
    });

    Some(retry_after.or(until_reset))
}

/// The `Retry-After` header in seconds, if present.
pub(crate) fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    header_u64(headers, "retry-after").map(Duration::from_secs)
}

fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
}

/// Human-friendly rendering of a wait time ("42s", "17m", "2h 5m").
pub(crate) fn format_wait(wait: Duration) -> String {
    let secs = wait.as_secs();
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs.div_ceil(60))
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    #[test]
    fn primary_rate_limit_is_detected_with_reset_time() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000);
        let h = headers(&[
            ("x-ratelimit-remaining", "0"),
            ("x-ratelimit-reset", "1600"),
        ]);
        assert_eq!(
            rate_limit_wait(StatusCode::FORBIDDEN, &h, now),
            Some(Some(Duration::from_secs(600)))
        );
    }

    #[test]
    fn secondary_rate_limit_uses_retry_after() {
        let h = headers(&[("retry-after", "60"), ("x-ratelimit-remaining", "12")]);
        assert_eq!(
            rate_limit_wait(StatusCode::FORBIDDEN, &h, SystemTime::now()),
            Some(Some(Duration::from_secs(60)))
        );
    }

    #[test]
    fn plain_forbidden_is_not_a_rate_limit() {
        let h = headers(&[("x-ratelimit-remaining", "42")]);
        assert_eq!(
            rate_limit_wait(StatusCode::FORBIDDEN, &h, SystemTime::now()),
            None
        );
        assert_eq!(
            rate_limit_wait(StatusCode::FORBIDDEN, &HeaderMap::new(), SystemTime::now()),
            None
        );
    }

    #[test]
    fn too_many_requests_is_always_a_rate_limit() {
        assert_eq!(
            rate_limit_wait(
                StatusCode::TOO_MANY_REQUESTS,
                &HeaderMap::new(),
                SystemTime::now()
            ),
            Some(None)
        );
    }

    #[test]
    fn other_statuses_are_not_rate_limits() {
        let h = headers(&[("x-ratelimit-remaining", "0")]);
        assert_eq!(
            rate_limit_wait(StatusCode::NOT_FOUND, &h, SystemTime::now()),
            None
        );
    }

    #[test]
    fn user_agent_has_contact_url() {
        assert!(USER_AGENT.starts_with("rust_secure_dependency_audit/"));
        assert!(USER_AGENT.contains("https://github.com/emorilebo/rust_secure_dependency_audit"));
    }

    #[test]
    fn wait_formatting() {
        assert_eq!(format_wait(Duration::from_secs(42)), "42s");
        assert_eq!(format_wait(Duration::from_secs(1277)), "22m");
        assert_eq!(format_wait(Duration::from_secs(7500)), "2h 5m");
    }

    #[tokio::test(start_paused = true)]
    async fn pacer_spaces_requests() {
        let pacer = Pacer::new(Duration::from_secs(1));
        let start = Instant::now();
        pacer.wait().await;
        pacer.wait().await;
        pacer.wait().await;
        assert!(Instant::now() - start >= Duration::from_secs(2));
    }
}
