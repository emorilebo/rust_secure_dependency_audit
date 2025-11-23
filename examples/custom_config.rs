//! Example showing custom configuration

use rust_secure_dependency_audit::{
    audit_project, AuditConfig, LicensePolicy, ScoringWeights, StalenessThresholds,
};
use std::collections::HashSet;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create custom configuration
    let mut config = AuditConfig::builder()
        .scoring_weights(ScoringWeights {
            recency: 0.50,      // Emphasize recency more
            maintenance: 0.30,
            community: 0.15,
            stability: 0.05,
        })
        .staleness_thresholds(StalenessThresholds {
            stale_days: 180,    // 6 months instead of 1 year
            risky_days: 365,    // 1 year instead of 2 years
            min_maintainers: 2, // Require at least 2 maintainers
        })
        .license_policy(LicensePolicy {
            allowed_licenses: HashSet::from([
                "MIT".to_string(),
                "Apache-2.0".to_string(),
                "BSD".to_string(),
            ]),
            forbidden_licenses: HashSet::from([
                "AGPL".to_string(),
            ]),
            warn_on_copyleft: true,
            warn_on_unknown: true,
        })
        .ignore_dependency("some-dev-tool".to_string())
        .build();
    
    // Validate and normalize weights
    config.scoring_weights.normalize();
    
    let project_path = Path::new(".");
    println!("Auditing with custom configuration...\n");
    
    let report = audit_project(project_path, &config).await?;
    
    println!("=== Custom Audit Results ===");
    println!("Project: {}", report.project_name);
    println!("Average health score: {:.1}", report.summary.average_health_score);
    println!("License issues: {}", report.summary.license_issues);
    
    // Check for license violations
    let license_violations: Vec<_> = report
        .dependencies
        .iter()
        .filter(|d| !d.warnings.is_empty() && d.warnings.iter().any(|w| w.contains("license")))
        .collect();
    
    if !license_violations.is_empty() {
        println!("\n⚠  License Violations:");
        for dep in license_violations {
            println!("  - {} v{} ({:?})", dep.name, dep.version, dep.license);
            for warning in &dep.warnings {
                if warning.contains("license") {
                    println!("    {}", warning);
                }
            }
        }
    }
    
    Ok(())
}
