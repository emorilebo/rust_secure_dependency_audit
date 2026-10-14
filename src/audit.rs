//! Main audit orchestration logic

use crate::config::AuditConfig;
use crate::error::{AuditError, Result};
use crate::footprint::estimate_footprint;
use crate::license::analyze_license;
use crate::metadata::github::{describe_outage, GitHubOutage};
use crate::metadata::{
    fetch_gitlab_metadata, CratesIoClient, GitHubClient, GitLabMetadata, OpenSSFClient,
};
use crate::parser::{parse_project, ParsedDependency};
use crate::scoring::{calculate_health_score, determine_status, ScoringInputs};
use crate::types::{AuditReport, DependencyHealth, DependencySource, HealthStatus};
use cargo_metadata::Metadata;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Shared per-run state handed to every dependency task.
struct AuditContext {
    config: AuditConfig,
    metadata: Metadata,
    crates_io: CratesIoClient,
    github: GitHubClient,
    openssf: OpenSSFClient,
}

/// Audit the Cargo project in `project_path` (the directory containing its
/// `Cargo.toml`) and return a health report.
///
/// Network problems with individual crates never fail the audit: they show
/// up as per-dependency warnings, as [`HealthStatus::Unknown`] when nothing
/// could be fetched, and as run-level [`AuditReport::notices`] (for example
/// when the GitHub API rate limit runs out).
///
/// # Errors
///
/// Returns an error if `Cargo.toml` is missing, `cargo metadata` fails, or an
/// HTTP client cannot be created.
pub async fn audit_project(project_path: &Path, config: &AuditConfig) -> Result<AuditReport> {
    info!("Starting audit of project at: {}", project_path.display());

    let project = parse_project(project_path)?;

    info!(
        "Found {} dependencies for project '{}'",
        project.dependencies.len(),
        project.name
    );

    let mut report = AuditReport::new(project.name, project_path.display().to_string());

    let context = Arc::new(AuditContext {
        config: config.clone(),
        metadata: project.metadata,
        crates_io: CratesIoClient::new(&config.network)?,
        github: GitHubClient::new(&config.network)?,
        openssf: OpenSSFClient::new(&config.network)?,
    });

    let mut tasks = Vec::new();
    let mut skipped_path_deps = 0;

    for dep in project.dependencies {
        if config.ignored_dependencies.contains(&dep.name) {
            debug!("Skipping ignored dependency: {}", dep.name);
            continue;
        }

        // Local path crates are your own code, not third-party supply chain;
        // there is no registry or repository metadata to score them with.
        if matches!(dep.source, DependencySource::Path { .. }) {
            debug!("Skipping local path dependency: {}", dep.name);
            skipped_path_deps += 1;
            continue;
        }

        let context = Arc::clone(&context);
        let label = format!("{} v{}", dep.name, dep.version);
        tasks.push((
            label,
            tokio::spawn(async move { process_dependency(dep, &context).await }),
        ));
    }

    let mut failed_tasks = Vec::new();
    for (label, task) in tasks {
        match task.await {
            Ok(dep_health) => report.dependencies.push(dep_health),
            Err(e) => {
                warn!("Processing {} failed: {}", label, e);
                failed_tasks.push(label);
            }
        }
    }

    report.compute_summary();
    report.notices = build_notices(&report, &context, skipped_path_deps, &failed_tasks);

    info!(
        "Audit complete: {}/{} healthy, {}/{} warnings, {}/{} stale, {}/{} risky, {}/{} unknown",
        report.summary.healthy,
        report.summary.total_dependencies,
        report.summary.warning,
        report.summary.total_dependencies,
        report.summary.stale,
        report.summary.total_dependencies,
        report.summary.risky,
        report.summary.total_dependencies,
        report.summary.unknown,
        report.summary.total_dependencies,
    );

    Ok(report)
}

/// Process a single dependency. Never fails: missing data becomes warnings
/// (and, if nothing at all could be fetched, an `Unknown` status).
async fn process_dependency(dep: ParsedDependency, context: &AuditContext) -> DependencyHealth {
    debug!("Processing dependency: {} v{}", dep.name, dep.version);

    let config = &context.config;
    let mut warnings = Vec::new();

    let crate_meta = match &dep.source {
        DependencySource::CratesIo => {
            match context.crates_io.fetch(&dep.name, &dep.version).await {
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

    // Repository: crates.io first, then the package's own manifest, then
    // the git source URL for git dependencies.
    let repo_url = crate_meta
        .as_ref()
        .and_then(|meta| meta.repository.clone())
        .or_else(|| dep.repository.clone())
        .or_else(|| match &dep.source {
            DependencySource::Git { url } => Some(url.clone()),
            _ => None,
        });

    let github_meta = match repo_url.as_deref() {
        Some(url) if url.contains("github.com") => match context.github.fetch(url).await {
            Ok(meta) => Some(meta),
            Err(e) => {
                debug!("Failed to fetch GitHub metadata for {}: {}", dep.name, e);
                warnings.push(github_warning(&e));
                None
            }
        },
        _ => None,
    };

    let gitlab_meta: Option<GitLabMetadata> = match repo_url.as_deref() {
        Some(url) if url.contains("gitlab.com") => {
            match fetch_gitlab_metadata(url, &config.network).await {
                Ok(meta) => Some(meta),
                Err(e) => {
                    debug!("Failed to fetch GitLab metadata for {}: {}", dep.name, e);
                    warnings.push(format!("Could not fetch GitLab metadata: {}", e));
                    None
                }
            }
        }
        _ => None,
    };

    let openssf_score = match repo_url.as_deref() {
        Some(url) => context.openssf.get_score(url).await,
        None => None,
    };

    let inputs = ScoringInputs {
        crate_meta: crate_meta.as_ref(),
        github: github_meta.as_ref(),
        gitlab: gitlab_meta.as_ref(),
        openssf_score,
    };

    let (health_score, status, metrics) = match calculate_health_score(inputs, config) {
        Some((score, metrics)) => (score, determine_status(score, config), Some(metrics)),
        None => {
            warnings.push(
                "Not scored: no crates.io or repository metadata could be fetched".to_string(),
            );
            (0, HealthStatus::Unknown, None)
        }
    };

    // The local manifest is authoritative for the locked version and works
    // offline; crates.io is the fallback.
    let license = dep
        .license
        .clone()
        .or_else(|| crate_meta.as_ref().and_then(|meta| meta.license.clone()));
    let (license_risk, license_warnings) =
        analyze_license(license.as_deref(), &config.license_policy);
    warnings.extend(license_warnings);

    let (footprint_risk, footprint_warnings) = estimate_footprint(
        &dep.package_id,
        &context.metadata,
        &config.footprint_thresholds,
    );
    warnings.extend(footprint_warnings);

    let is_yanked = crate_meta.as_ref().is_some_and(|meta| meta.is_yanked);
    if is_yanked {
        warnings.push(format!("Version {} is yanked on crates.io", dep.version));
    }

    DependencyHealth {
        name: dep.name,
        version: dep.version,
        is_direct: dep.is_direct,
        health_score,
        status,
        license,
        license_risk,
        footprint_risk: Some(footprint_risk),
        source: dep.source,
        metrics,
        warnings,
        is_yanked,
    }
}

const GITHUB_SKIPPED_PREFIX: &str = "GitHub metadata unavailable";

fn github_warning(error: &AuditError) -> String {
    match error {
        AuditError::RateLimitExceeded { .. } => format!(
            "{}: GitHub API rate limit reached; scored without repository signals",
            GITHUB_SKIPPED_PREFIX
        ),
        other => format!("{}: {}", GITHUB_SKIPPED_PREFIX, other),
    }
}

fn build_notices(
    report: &AuditReport,
    context: &AuditContext,
    skipped_path_deps: usize,
    failed_tasks: &[String],
) -> Vec<String> {
    let mut notices = Vec::new();

    if let Some(outage) = context.github.outage() {
        let affected = report
            .dependencies
            .iter()
            .filter(|dep| {
                dep.warnings
                    .iter()
                    .any(|w| w.starts_with(GITHUB_SKIPPED_PREFIX))
            })
            .count();
        let advice = match (outage, context.github.is_authenticated()) {
            (GitHubOutage::BadCredentials, _) => "Check or unset GITHUB_TOKEN.",
            (_, true) => "Re-run after the reset for complete results.",
            (_, false) => {
                "Set GITHUB_TOKEN to raise the limit (a personal access token with no scopes is enough)."
            }
        };
        notices.push(format!(
            "{}. {} {} scored without GitHub repository data \
             (maintenance and community components left out). {}",
            describe_outage(outage, context.github.is_authenticated()),
            dependencies(affected),
            was_were(affected),
            advice
        ));
    }

    if report.summary.unknown > 0 {
        notices.push(format!(
            "{} could not be scored because no crates.io or repository \
             metadata was available (status \"Unknown\").",
            dependencies(report.summary.unknown)
        ));
    }

    if skipped_path_deps > 0 {
        notices.push(format!(
            "Skipped {} local path {} (your own code, not third-party).",
            skipped_path_deps,
            if skipped_path_deps == 1 {
                "dependency"
            } else {
                "dependencies"
            }
        ));
    }

    if !failed_tasks.is_empty() {
        notices.push(format!(
            "Internal error while processing: {}",
            failed_tasks.join(", ")
        ));
    }

    notices
}

/// "1 dependency" / "3 dependencies"
fn dependencies(count: usize) -> String {
    if count == 1 {
        "1 dependency".to_string()
    } else {
        format!("{} dependencies", count)
    }
}

fn was_were(count: usize) -> &'static str {
    if count == 1 {
        "was"
    } else {
        "were"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    #[ignore = "requires network access; makes one crates.io request per second"]
    async fn test_audit_self() {
        let project_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let report = audit_project(&project_path, &AuditConfig::default())
            .await
            .unwrap();

        assert_eq!(report.project_name, "rust_secure_dependency_audit");
        assert!(!report.dependencies.is_empty());
    }

    #[tokio::test]
    async fn path_dependencies_are_skipped_offline() {
        // A project whose only dependency is a local path crate needs no
        // network: the dependency is skipped and reported in a notice.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("helper/src")).unwrap();
        std::fs::write(root.join("helper/src/lib.rs"), "").unwrap();
        std::fs::write(
            root.join("helper/Cargo.toml"),
            "[package]\nname = \"helper\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nhelper = { path = \"helper\" }\n\n[workspace]\n",
        )
        .unwrap();

        let report = audit_project(root, &AuditConfig::default()).await.unwrap();

        assert_eq!(report.project_name, "app");
        assert!(report.dependencies.is_empty());
        assert_eq!(report.summary.total_dependencies, 0);
        assert!(report
            .notices
            .iter()
            .any(|n| n.contains("Skipped 1 local path dependency")));
    }
}
