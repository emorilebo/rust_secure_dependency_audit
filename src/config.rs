//! Configuration for audit behavior and scoring heuristics

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// Main configuration for the audit process.
///
/// Every field (and every field of the nested sections) has a default, so a
/// TOML config file only needs the keys you want to change. Unknown keys are
/// rejected, so a typo cannot silently fall back to a default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuditConfig {
    /// Scoring weights for health calculation
    pub scoring_weights: ScoringWeights,
    /// Thresholds for staleness detection
    pub staleness_thresholds: StalenessThresholds,
    /// License policy configuration
    pub license_policy: LicensePolicy,
    /// Footprint risk thresholds
    pub footprint_thresholds: FootprintThresholds,
    /// Network configuration
    pub network: NetworkConfig,
    /// Dependencies to ignore in the audit
    pub ignored_dependencies: HashSet<String>,
}

/// Weights for different components of the health score
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScoringWeights {
    /// Weight for recency score (0.0-1.0)
    pub recency: f32,
    /// Weight for maintenance score (0.0-1.0)
    pub maintenance: f32,
    /// Weight for community score (0.0-1.0)
    pub community: f32,
    /// Weight for stability score (0.0-1.0)
    pub stability: f32,
    /// Weight for security score (0.0-1.0)
    pub security: f32,
}

/// Thresholds for determining staleness
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StalenessThresholds {
    /// Days since last update after which the recency component drops to 30/100
    pub stale_days: u32,
    /// Days since last update after which the recency component drops to 10/100
    pub risky_days: u32,
    /// Reserved: not used by the scorer yet
    pub min_maintainers: u32,
}

/// License policy configuration
///
/// Entries in `allowed_licenses` / `forbidden_licenses` are matched
/// case-insensitively as a prefix of each SPDX license id in an expression,
/// so `"GPL-3.0"` matches `GPL-3.0-only` but not `LGPL-3.0-only`, and
/// `"BSD"` matches every `BSD-*` license.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LicensePolicy {
    /// Allowed license types (empty = allow all)
    pub allowed_licenses: HashSet<String>,
    /// Explicitly forbidden licenses
    pub forbidden_licenses: HashSet<String>,
    /// Warn on copyleft licenses
    pub warn_on_copyleft: bool,
    /// Warn on unknown licenses
    pub warn_on_unknown: bool,
}

/// Footprint risk thresholds
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FootprintThresholds {
    /// Maximum acceptable transitive dependency count
    pub max_transitive_deps: Option<u32>,
    /// Maximum acceptable footprint risk score (0.0-1.0)
    pub max_footprint_risk: Option<f32>,
}

/// Network configuration for API calls
///
/// When deserialized from a config file, a missing `github_token` /
/// `gitlab_token` falls back to the `GITHUB_TOKEN` / `GITLAB_TOKEN`
/// environment variables. Tokens are never serialized and are redacted from
/// `Debug` output.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Maximum number of retries for failed requests
    pub max_retries: u32,
    /// Minimum delay between crates.io API requests, and the base delay for
    /// retry backoff (milliseconds). Values below 1000 are raised to 1000 for
    /// crates.io, whose data-access policy allows at most one request per second.
    pub request_delay_ms: u64,
    /// GitHub API token (optional, for higher rate limits)
    #[serde(skip_serializing)]
    pub github_token: Option<String>,
    /// GitLab API token (optional)
    #[serde(skip_serializing)]
    pub gitlab_token: Option<String>,
    /// Enable OpenSSF Scorecard checks (requires network)
    pub enable_openssf: bool,
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            recency: 0.35,
            maintenance: 0.25,
            community: 0.15,
            stability: 0.10,
            security: 0.15,
        }
    }
}

impl ScoringWeights {
    /// Validate that weights sum to approximately 1.0
    pub fn validate(&self) -> Result<(), String> {
        let sum = self.recency + self.maintenance + self.community + self.stability + self.security;
        if (sum - 1.0).abs() > 0.01 {
            return Err(format!("Scoring weights must sum to 1.0, got {}", sum));
        }
        Ok(())
    }

    /// Normalize weights to sum to 1.0
    pub fn normalize(&mut self) {
        let sum = self.recency + self.maintenance + self.community + self.stability + self.security;
        if sum > 0.0 {
            self.recency /= sum;
            self.maintenance /= sum;
            self.community /= sum;
            self.stability /= sum;
            self.security /= sum;
        }
    }
}

impl Default for StalenessThresholds {
    fn default() -> Self {
        Self {
            stale_days: 365, // 1 year
            risky_days: 730, // 2 years
            min_maintainers: 1,
        }
    }
}

impl Default for LicensePolicy {
    fn default() -> Self {
        Self {
            allowed_licenses: HashSet::new(),
            forbidden_licenses: HashSet::new(),
            warn_on_copyleft: true,
            warn_on_unknown: true,
        }
    }
}

impl Default for FootprintThresholds {
    fn default() -> Self {
        Self {
            max_transitive_deps: Some(100),
            max_footprint_risk: Some(0.8),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 30,
            max_retries: 3,
            request_delay_ms: 1000,
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            gitlab_token: std::env::var("GITLAB_TOKEN").ok(),
            enable_openssf: true,
        }
    }
}

impl std::fmt::Debug for NetworkConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redact = |token: &Option<String>| token.as_ref().map(|_| "<redacted>");
        f.debug_struct("NetworkConfig")
            .field("timeout_secs", &self.timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("request_delay_ms", &self.request_delay_ms)
            .field("github_token", &redact(&self.github_token))
            .field("gitlab_token", &redact(&self.gitlab_token))
            .field("enable_openssf", &self.enable_openssf)
            .finish()
    }
}

impl NetworkConfig {
    /// Get timeout as Duration
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    /// Get request delay as Duration
    pub fn request_delay(&self) -> Duration {
        Duration::from_millis(self.request_delay_ms)
    }
}

impl AuditConfig {
    /// Create a new builder for AuditConfig
    pub fn builder() -> AuditConfigBuilder {
        AuditConfigBuilder::default()
    }
}

/// Builder for AuditConfig
#[derive(Default)]
pub struct AuditConfigBuilder {
    scoring_weights: Option<ScoringWeights>,
    staleness_thresholds: Option<StalenessThresholds>,
    license_policy: Option<LicensePolicy>,
    footprint_thresholds: Option<FootprintThresholds>,
    network: Option<NetworkConfig>,
    ignored_dependencies: HashSet<String>,
}

impl AuditConfigBuilder {
    pub fn scoring_weights(mut self, weights: ScoringWeights) -> Self {
        self.scoring_weights = Some(weights);
        self
    }

    pub fn staleness_thresholds(mut self, thresholds: StalenessThresholds) -> Self {
        self.staleness_thresholds = Some(thresholds);
        self
    }

    pub fn license_policy(mut self, policy: LicensePolicy) -> Self {
        self.license_policy = Some(policy);
        self
    }

    pub fn footprint_thresholds(mut self, thresholds: FootprintThresholds) -> Self {
        self.footprint_thresholds = Some(thresholds);
        self
    }

    pub fn network(mut self, network: NetworkConfig) -> Self {
        self.network = Some(network);
        self
    }

    pub fn ignore_dependency(mut self, name: String) -> Self {
        self.ignored_dependencies.insert(name);
        self
    }

    pub fn build(self) -> AuditConfig {
        AuditConfig {
            scoring_weights: self.scoring_weights.unwrap_or_default(),
            staleness_thresholds: self.staleness_thresholds.unwrap_or_default(),
            license_policy: self.license_policy.unwrap_or_default(),
            footprint_thresholds: self.footprint_thresholds.unwrap_or_default(),
            network: self.network.unwrap_or_default(),
            ignored_dependencies: self.ignored_dependencies,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_file_uses_defaults() {
        let config: AuditConfig = toml::from_str("").unwrap();
        assert!(config.scoring_weights.validate().is_ok());
        assert_eq!(config.staleness_thresholds.stale_days, 365);
        assert!(config.ignored_dependencies.is_empty());
    }

    #[test]
    fn partial_section_keeps_other_defaults() {
        let config: AuditConfig = toml::from_str("[network]\nrequest_delay_ms = 2500\n").unwrap();
        assert_eq!(config.network.request_delay_ms, 2500);
        assert_eq!(config.network.timeout_secs, 30);
        assert!(config.network.enable_openssf);
        assert_eq!(config.footprint_thresholds.max_transitive_deps, Some(100));
    }

    #[test]
    fn readme_configuration_example_parses() {
        let readme = include_str!("../README.md");
        let section = readme
            .split("## Configuration")
            .nth(1)
            .expect("README has a Configuration section");
        let toml_block = section
            .split("```toml")
            .nth(1)
            .and_then(|rest| rest.split("```").next())
            .expect("Configuration section has a toml code block");

        let config: AuditConfig = toml::from_str(toml_block).unwrap();
        assert!(config.scoring_weights.validate().is_ok());
        assert_eq!(config.staleness_thresholds.stale_days, 180);
        assert!(config.ignored_dependencies.contains("some-internal-crate"));
    }

    #[test]
    fn unknown_keys_are_rejected() {
        // A misspelt key (or a top-level key placed under a [table]) used to
        // be ignored silently.
        let err = toml::from_str::<AuditConfig>("[staleness_thresholds]\nstale_day = 10\n")
            .unwrap_err()
            .to_string();
        assert!(err.contains("stale_day"), "{err}");

        let err = toml::from_str::<AuditConfig>(
            "[network]\ntimeout_secs = 5\nignored_dependencies = [\"x\"]\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("ignored_dependencies"), "{err}");
    }

    #[test]
    fn debug_output_redacts_tokens() {
        let network = NetworkConfig {
            github_token: Some("ghp_secret_value".to_string()),
            gitlab_token: Some("glpat_secret_value".to_string()),
            ..NetworkConfig::default()
        };
        let debug = format!("{:?}", network);
        assert!(!debug.contains("secret_value"));
        assert!(debug.contains("<redacted>"));

        let serialized = toml::to_string(&network).unwrap();
        assert!(!serialized.contains("secret_value"));
    }
}
