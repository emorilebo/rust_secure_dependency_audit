//! # rust_secure_dependency_audit
//!
//! Audit the dependencies of a Rust (Cargo) project for **maintenance health,
//! SPDX license risk and footprint** — a supply-chain health check that
//! complements `cargo-audit` (known vulnerabilities) and `cargo-deny`
//! (policy). The same engine powers the `secure-audit` CLI.
//!
//! For every crate in the resolved dependency graph the audit combines:
//!
//! - **crates.io** — newest release, version count, downloads, publishers,
//!   yanked status (one request per unique crate, at most one per second);
//! - **GitHub / GitLab** — last push, archived flag, open issues, stars,
//!   contributors, `SECURITY.md`;
//! - **[OpenSSF Scorecard](https://securityscorecards.dev/)** — when the
//!   repository has been scored;
//! - **`cargo metadata`** — SPDX license expression, dependency graph and
//!   features, used offline.
//!
//! Missing data degrades the result instead of failing the run: if the GitHub
//! API rate limit runs out, remaining crates are scored without repository
//! signals and [`AuditReport::notices`] says so; a crate with no data at all
//! gets [`HealthStatus::Unknown`].
//!
//! ## Quick start
//!
//! ```no_run
//! use rust_secure_dependency_audit::{audit_project, AuditConfig, HealthStatus};
//! use std::path::Path;
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! // Reads GITHUB_TOKEN / GITLAB_TOKEN from the environment when set.
//! let config = AuditConfig::default();
//! let report = audit_project(Path::new("."), &config).await?;
//!
//! for notice in &report.notices {
//!     eprintln!("note: {notice}");
//! }
//! for dep in &report.dependencies {
//!     if dep.status != HealthStatus::Healthy {
//!         println!("{} v{}: {} ({})", dep.name, dep.version, dep.status, dep.health_score);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Configuration
//!
//! [`AuditConfig`] deserializes from TOML with every key optional, or can be
//! built in code:
//!
//! ```
//! use rust_secure_dependency_audit::{AuditConfig, ScoringWeights};
//!
//! let config = AuditConfig::builder()
//!     .scoring_weights(ScoringWeights {
//!         recency: 0.4,
//!         maintenance: 0.3,
//!         community: 0.1,
//!         stability: 0.1,
//!         security: 0.1,
//!     })
//!     .ignore_dependency("internal-crate".to_string())
//!     .build();
//!
//! assert!(config.scoring_weights.validate().is_ok());
//! assert!(config.ignored_dependencies.contains("internal-crate"));
//! ```
//!
//! See the [README](https://github.com/emorilebo/rust_secure_dependency_audit#readme)
//! for the CLI, scoring details and limitations.

mod audit;
mod config;
mod error;
mod footprint;
mod license;
mod metadata;
mod parser;
mod scoring;
mod types;

// Re-export public API
pub use audit::audit_project;
pub use config::{
    AuditConfig, AuditConfigBuilder, FootprintThresholds, LicensePolicy, NetworkConfig,
    ScoringWeights, StalenessThresholds,
};
pub use error::{AuditError, Result};
pub use types::{
    AuditReport, AuditSummary, ComponentScores, DependencyHealth, DependencyMetrics,
    DependencySource, HealthStatus, LicenseRisk, RepositoryMetrics,
};

/// Compiles the Rust code blocks in README.md as doctests, so the README
/// examples cannot drift from the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
