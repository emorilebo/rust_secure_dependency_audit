//! Basic example of using the audit API

use rust_secure_dependency_audit::{audit_project, AuditConfig, HealthStatus};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Use default configuration
    let config = AuditConfig::default();
    
    // Audit the current project
    let project_path = Path::new(".");
    println!("Auditing project at: {}", project_path.display());
    
    let report = audit_project(project_path, &config).await?;
    
    println!("\n=== Audit Results ===");
    println!("Project: {}", report.project_name);
    println!("Total dependencies: {}", report.dependencies.len());
    println!();
    
    // Print summary
    println!("Health Summary:");
    println!("  Healthy: {}", report.summary.healthy);
    println!("  Warning: {}", report.summary.warning);
    println!("  Stale: {}", report.summary.stale);
    println!("  Risky: {}", report.summary.risky);
    println!("  Average score: {:.1}", report.summary.average_health_score);
    println!();
    
    // Show risky dependencies
    let risky: Vec<_> = report
        .dependencies
        .iter()
        .filter(|d| matches!(d.status, HealthStatus::Risky | HealthStatus::Stale))
        .collect();
    
    if !risky.is_empty() {
        println!("Risky/Stale Dependencies:");
        for dep in risky {
            println!(
                "  - {} v{}: score {} ({})",
                dep.name, dep.version, dep.health_score, dep.status
            );
            
            if !dep.warnings.is_empty() {
                for warning in &dep.warnings {
                    println!("    ⚠  {}", warning);
                }
            }
        }
    } else {
        println!("✓ No risky or stale dependencies found!");
    }
    
    Ok(())
}
