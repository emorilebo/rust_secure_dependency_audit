//! Configuration for audit behavior and scoring heuristics

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

/// Main configuration for the audit process
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct ScoringWeights {
    /// Weight for recency score (0.0-1.0)
    pub recency: f32,
    /// Weight for maintenance score (0.0-1.0)
    pub maintenance: f32,
    /// Weight for community score (0.0-1.0)
    pub community: f32,
    /// Weight for stability score (0.0-1.0)
    pub stability: f32,
}

/// Thresholds for determining staleness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StalenessThresholds {
    /// Days since last update before considering "stale"
    pub stale_days: u32,
    /// Days since last update before considering "risky"
    pub risky_days: u32,
    /// Minimum number of maintainers to be considered healthy
    pub min_maintainers: u32,
}

/// License policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct FootprintThresholds {
    /// Maximum acceptable transitive dependency count
    pub max_transitive_deps: Option<u32>,
    /// Maximum acceptable footprint risk score (0.0-1.0)
    pub max_footprint_risk: Option<f32>,
}

/// Network configuration for API calls
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Maximum number of retries for failed requests
    pub max_retries: u32,
    /// Delay between requests to avoid rate limiting (milliseconds)
    pub request_delay_ms: u64,
    /// GitHub API token (optional, for higher rate limits)
    pub github_token: Option<String>,
    /// GitLab API token (optional)
    pub gitlab_token: Option<String>,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            scoring_weights: ScoringWeights::default(),
            staleness_thresholds: StalenessThresholds::default(),
            license_policy: LicensePolicy::default(),
            footprint_thresholds: FootprintThresholds::default(),
            network: NetworkConfig::default(),
            ignored_dependencies: HashSet::new(),
        }
    }
}

impl Default for ScoringWeights {
    fn default() -> Self {
        Self {
            recency: 0.40,
            maintenance: 0.30,
            community: 0.20,
            stability: 0.10,
        }
    }
}

impl ScoringWeights {
    /// Validate that weights sum to approximately 1.0
    pub fn validate(&self) -> Result<(), String> {
        let sum = self.recency + self.maintenance + self.community + self.stability;
        if (sum - 1.0).abs() > 0.01 {
            return Err(format!(
                "Scoring weights must sum to 1.0, got {}",
                sum
            ));
        }
        Ok(())
    }

    /// Normalize weights to sum to 1.0
    pub fn normalize(&mut self) {
        let sum = self.recency + self.maintenance + self.community + self.stability;
        if sum > 0.0 {
            self.recency /= sum;
            self.maintenance /= sum;
            self.community /= sum;
            self.stability /= sum;
        }
    }
}

impl Default for StalenessThresholds {
    fn default() -> Self {
        Self {
            stale_days: 365,      // 1 year
            risky_days: 730,      // 2 years
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
            request_delay_ms: 100,
            github_token: std::env::var("GITHUB_TOKEN").ok(),
            gitlab_token: std::env::var("GITLAB_TOKEN").ok(),
        }
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
