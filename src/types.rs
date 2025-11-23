//! Core data types for dependency health reporting

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete audit report for a Rust project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    /// Name of the audited project
    pub project_name: String,
    /// Path to the audited project
    pub project_path: String,
    /// Timestamp when audit was performed
    pub timestamp: DateTime<Utc>,
    /// Health information for all dependencies
    pub dependencies: Vec<DependencyHealth>,
    /// Summary statistics
    pub summary: AuditSummary,
}

/// Summary statistics for an audit report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    pub total_dependencies: usize,
    pub healthy: usize,
    pub warning: usize,
    pub stale: usize,
    pub risky: usize,
    pub average_health_score: f32,
    pub license_issues: usize,
    pub high_footprint_count: usize,
}

/// Health information for a single dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyHealth {
    /// Crate name
    pub name: String,
    /// Version
    pub version: String,
    /// Whether this is a direct dependency (vs transitive)
    pub is_direct: bool,
    /// Overall health score (0-100)
    pub health_score: u8,
    /// Health status category
    pub status: HealthStatus,
    /// License information
    pub license: Option<String>,
    /// License risk level
    pub license_risk: LicenseRisk,
    /// Estimated footprint risk (0.0-1.0)
    pub footprint_risk: Option<f32>,
    /// Source of the dependency
    pub source: DependencySource,
    /// Detailed metrics used for scoring
    pub metrics: Option<DependencyMetrics>,
    /// Any warnings or issues
    pub warnings: Vec<String>,
}

/// Health status categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// Healthy: actively maintained, good community support
    Healthy,
    /// Warning: some concerns but generally okay
    Warning,
    /// Stale: not updated recently, limited activity
    Stale,
    /// Risky: deprecated, unmaintained, or high risk
    Risky,
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "Healthy"),
            Self::Warning => write!(f, "Warning"),
            Self::Stale => write!(f, "Stale"),
            Self::Risky => write!(f, "Risky"),
        }
    }
}

/// License risk categorization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LicenseRisk {
    /// Permissive licenses (MIT, Apache, BSD, etc.)
    Permissive,
    /// Copyleft licenses (GPL, LGPL, AGPL, etc.)
    Copyleft,
    /// Proprietary or restrictive licenses
    Proprietary,
    /// License not found or not recognized
    Unknown,
}

impl std::fmt::Display for LicenseRisk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Permissive => write!(f, "Permissive"),
            Self::Copyleft => write!(f, "Copyleft"),
            Self::Proprietary => write!(f, "Proprietary"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Source of a dependency
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum DependencySource {
    /// From crates.io registry
    CratesIo,
    /// From a git repository
    Git { url: String },
    /// From a local path
    Path { path: String },
    /// Unknown source
    Unknown,
}

/// Detailed metrics used for health scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyMetrics {
    /// Days since last publish/commit
    pub days_since_last_update: Option<u32>,
    /// Number of crate versions published
    pub version_count: Option<u32>,
    /// Number of authors/maintainers
    pub maintainer_count: Option<u32>,
    /// Repository metrics (if available)
    pub repository: Option<RepositoryMetrics>,
    /// Individual component scores
    pub scores: ComponentScores,
}

/// Repository-specific metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryMetrics {
    /// Number of open issues
    pub open_issues: Option<u32>,
    /// Number of contributors
    pub contributor_count: Option<u32>,
    /// Days since last commit
    pub days_since_last_commit: Option<u32>,
    /// Number of stars (GitHub/GitLab)
    pub stars: Option<u32>,
    /// Whether the repository is archived
    pub is_archived: Option<bool>,
}

/// Individual component scores (0-100 scale)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentScores {
    /// Score based on recency of updates
    pub recency: f32,
    /// Score based on maintenance activity
    pub maintenance: f32,
    /// Score based on community size and engagement
    pub community: f32,
    /// Score based on version stability
    pub stability: f32,
}

impl AuditReport {
    /// Create a new audit report
    pub fn new(project_name: String, project_path: String) -> Self {
        Self {
            project_name,
            project_path,
            timestamp: Utc::now(),
            dependencies: Vec::new(),
            summary: AuditSummary::default(),
        }
    }

    /// Compute summary statistics from dependencies
    pub fn compute_summary(&mut self) {
        let total = self.dependencies.len();
        let mut healthy = 0;
        let mut warning = 0;
        let mut stale = 0;
        let mut risky = 0;
        let mut total_score = 0u32;
        let mut license_issues = 0;
        let mut high_footprint = 0;

        for dep in &self.dependencies {
            match dep.status {
                HealthStatus::Healthy => healthy += 1,
                HealthStatus::Warning => warning += 1,
                HealthStatus::Stale => stale += 1,
                HealthStatus::Risky => risky += 1,
            }

            total_score += dep.health_score as u32;

            if matches!(
                dep.license_risk,
                LicenseRisk::Copyleft | LicenseRisk::Proprietary | LicenseRisk::Unknown
            ) {
                license_issues += 1;
            }

            if let Some(footprint) = dep.footprint_risk {
                if footprint > 0.7 {
                    high_footprint += 1;
                }
            }
        }

        self.summary = AuditSummary {
            total_dependencies: total,
            healthy,
            warning,
            stale,
            risky,
            average_health_score: if total > 0 {
                total_score as f32 / total as f32
            } else {
                0.0
            },
            license_issues,
            high_footprint_count: high_footprint,
        };
    }
}

impl Default for AuditSummary {
    fn default() -> Self {
        Self {
            total_dependencies: 0,
            healthy: 0,
            warning: 0,
            stale: 0,
            risky: 0,
            average_health_score: 0.0,
            license_issues: 0,
            high_footprint_count: 0,
        }
    }
}
