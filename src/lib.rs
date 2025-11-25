//! # rust_secure_dependency_audit
//!
//! A comprehensive tool for auditing Rust project dependencies, providing insights into:
//! - **Health scoring**: Assess dependency maintenance status and community activity
//! - **License analysis**: Identify license risks and compliance issues
//! - **Footprint estimation**: Evaluate dependency bloat for embedded/mobile projects
//! - **Risk assessment**: Detect stale, unmaintained, or risky dependencies
//!
//! ## Quick Start
//!
//! ```no_run
//! use rust_secure_dependency_audit::{audit_project, AuditConfig};
//! use std::path::Path;
//!
//! # #[tokio::main]
//! # async fn main() -> anyhow::Result<()> {
//! let config = AuditConfig::default();
//! let report = audit_project(Path::new("."), &config).await?;
//!
//! for dep in report.dependencies {
//!     println!("{}: {} (score: {})", dep.name, dep.status, dep.health_score);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Features
//!
//! - Parallel metadata fetching for fast analysis
//! - Configurable health scoring heuristics
//! - Support for crates.io and Git-hosted dependencies
//! - Comprehensive license categorization (SPDX)
//! - CLI tool with multiple output formats (JSON, Markdown)

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
pub use config::{AuditConfig, FootprintThresholds, LicensePolicy, NetworkConfig, ScoringWeights, StalenessThresholds};
pub use error::{AuditError, Result};
pub use types::{AuditReport, DependencyHealth, HealthStatus, LicenseRisk};
