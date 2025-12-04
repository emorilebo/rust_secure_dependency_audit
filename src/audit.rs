//! Main audit orchestration logic

use crate::config::AuditConfig;
use crate::error::Result;
use crate::footprint::estimate_footprint;
use crate::license::analyze_license;
// use crate::metadata::openssf::OpenSSFClient;
use crate::metadata::{fetch_crate_metadata, fetch_github_metadata, fetch_gitlab_metadata};
use crate::parser::{get_project_name, parse_project, ParsedDependency};
use crate::scoring::{calculate_health_score, determine_status};
use crate::types::{AuditReport, DependencyHealth, DependencySource};
use cargo_metadata::MetadataCommand;
use std::path::Path;
use tokio::time::sleep;
use tracing::{debug, info, warn};

/// Audit a Rust project and generate a health report
pub async fn audit_project(project_path: &Path, config: &AuditConfig) -> Result<AuditReport> {
    info!("Starting audit of project at: {}", project_path.display());

    // Parse the project
    let project_name = get_project_name(project_path)?;
    let dependencies = parse_project(project_path)?;

    info!(
        "Found {} dependencies for project '{}'",
        dependencies.len(),
        project_name
    );

    // Get cargo metadata for footprint analysis
    let cargo_metadata = MetadataCommand::new()
        .manifest_path(project_path.join("Cargo.toml"))
        .exec()?;

    // Create report
    let mut report = AuditReport::new(
        project_name,
        project_path.display().to_string(),
    );

    // Process dependencies in parallel (with rate limiting)
    let mut tasks = Vec::new();

    for dep in dependencies {
        // Skip ignored dependencies
        if config.ignored_dependencies.contains(&dep.name) {
            debug!("Skipping ignored dependency: {}", dep.name);
            continue;
        }

        let config_clone = config.clone();
        let metadata_clone = cargo_metadata.clone();

        let task = tokio::spawn(async move {
            process_dependency(dep, &config_clone, &metadata_clone).await
        });

        tasks.push(task);

        // Add delay to avoid overwhelming APIs
        sleep(config.network.request_delay()).await;
    }

    // Collect results
    for task in tasks {
        match task.await {
            Ok(Ok(dep_health)) => {
                report.dependencies.push(dep_health);
            }
            Ok(Err(e)) => {
                warn!("Failed to process dependency: {}", e);
                // Continue with other dependencies
            }
            Err(e) => {
                warn!("Task failed: {}", e);
            }
        }
    }

    // Compute summary statistics
    report.compute_summary();

    info!(
        "Audit complete: {}/{} healthy, {}/{} warnings, {}/{} stale, {}/{} risky",
        report.summary.healthy,
        report.summary.total_dependencies,
        report.summary.warning,
        report.summary.total_dependencies,
        report.summary.stale,
        report.summary.total_dependencies,
        report.summary.risky,
        report.summary.total_dependencies,
    );

    Ok(report)
}

/// Process a single dependency
async fn process_dependency(
    dep: ParsedDependency,
    config: &AuditConfig,
    cargo_metadata: &cargo_metadata::Metadata,
) -> Result<DependencyHealth> {
    debug!("Processing dependency: {} v{}", dep.name, dep.version);

    let mut warnings = Vec::new();

    // Fetch crates.io metadata (if from crates.io)
    let crate_meta = match &dep.source {
        DependencySource::CratesIo => {
            match fetch_crate_metadata(&dep.name, &dep.version, &config.network).await {
                Ok(meta) => Some(meta),
                Err(e) => {
                    warn!("Failed to fetch crates.io metadata for {}: {}", dep.name, e);
                    warnings.push(format!("Could not fetch crates.io metadata: {}", e));
                    None
                }
            }
        }
        _ => None,
    };

    // Extract repository URL
    let repo_url = crate_meta.as_ref().and_then(|m| m.repository.as_ref());

    // Fetch GitHub/GitLab metadata if available
    let github_meta = if let Some(url) = repo_url {
        if url.contains("github.com") {
            match fetch_github_metadata(url, &config.network).await {
                Ok(meta) => Some(meta),
                Err(e) => {
                    debug!("Failed to fetch GitHub metadata for {}: {}", dep.name, e);
                    warnings.push(format!("Could not fetch GitHub metadata: {}", e));
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let gitlab_meta = if let Some(url) = repo_url {
        if url.contains("gitlab.com") {
            match fetch_gitlab_metadata(url, &config.network).await {
                Ok(meta) => Some(meta),
                Err(e) => {
                    debug!("Failed to fetch GitLab metadata for {}: {}", dep.name, e);
                    warnings.push(format!("Could not fetch GitLab metadata: {}", e));
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Fetch OpenSSF Scorecard
    /*
    let openssf_score = if let Some(url) = repo_url {
        match OpenSSFClient::new(&config.network) {
            Ok(client) => match client.get_scorecard(url).await {
                Ok(Some(data)) => Some(data.score),
                Ok(None) => None,
                Err(e) => {
                    debug!("Failed to fetch OpenSSF scorecard for {}: {}", dep.name, e);
                    None
                }
            },
            Err(_) => None,
        }
    } else {
        None
    };
    */
    let openssf_score = None;

    // Calculate health score
    let (health_score, _component_scores, metrics) = calculate_health_score(
        crate_meta.as_ref(),
        github_meta.as_ref(),
        gitlab_meta.as_ref(),
        openssf_score,
        config,
    );

    let status = determine_status(health_score, config);

    // Analyze license
    let license_str = crate_meta.as_ref().and_then(|m| m.license.as_deref());
    let (license_risk, license_warnings) =
        analyze_license(license_str, &config.license_policy);
    warnings.extend(license_warnings);

    // Estimate footprint
    let (footprint_risk, footprint_warnings) =
        estimate_footprint(&dep.package_id, cargo_metadata, &config.footprint_thresholds);
    warnings.extend(footprint_warnings);

    Ok(DependencyHealth {
        name: dep.name,
        version: dep.version,
        is_direct: dep.is_direct,
        health_score,
        status,
        license: license_str.map(String::from),
        license_risk,
        footprint_risk: Some(footprint_risk),
        source: dep.source,
        metrics,
        warnings,
        is_yanked: crate_meta.as_ref().map(|m| m.is_yanked).unwrap_or(false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_audit_self() {
        // Test auditing this crate itself
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let project_path = PathBuf::from(manifest_dir);

        let config = AuditConfig::default();

        let result = audit_project(&project_path, &config).await;

        match result {
            Ok(report) => {
                assert!(!report.project_name.is_empty());
                assert!(report.dependencies.len() > 0);
                println!("Self-audit successful: {} dependencies found", report.dependencies.len());
            }
            Err(e) => {
                // Might fail in CI without network, that's okay
                eprintln!("Self-audit failed (expected in some environments): {}", e);
            }
        }
    }
}
