//! CLI tool for auditing Rust dependencies

use clap::{Parser, Subcommand, ValueEnum};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use rust_secure_dependency_audit::{
    audit_project, AuditConfig, AuditReport, DependencyHealth, HealthStatus, LicenseRisk,
};
use std::path::PathBuf;
use std::process;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "secure-audit")]
#[command(about = "Audit Rust project dependencies for health, security, and maintenance risks", long_about = None)]
#[command(version)]
struct Cli {
    /// Path to the Rust project to audit
    #[arg(short = 'p', long, default_value = ".", global = true)]
    project_path: PathBuf,

    /// Path to custom configuration file (TOML)
    #[arg(short = 'c', long, global = true)]
    config: Option<PathBuf>,

    /// Dependencies to ignore (can be specified multiple times)
    #[arg(long = "ignore", global = true)]
    ignore_dependencies: Vec<String>,

    /// Enable verbose logging
    #[arg(short = 'v', long, global = true)]
    verbose: bool,

    /// Suppress progress output (quiet mode, useful for CI)
    #[arg(short = 'q', long, global = true)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run full audit and display summary
    Scan {
        /// Fail if any dependency has a health score below this threshold (0-100)
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        fail_threshold: Option<u8>,

        /// Display detailed information for each dependency
        #[arg(long)]
        detailed: bool,
    },

    /// Generate detailed audit report
    Report {
        /// Output format
        #[arg(
            short = 'f',
            long,
            value_enum,
            ignore_case = true,
            default_value = "markdown"
        )]
        format: ReportFormat,

        /// Output file (default: stdout)
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },

    /// Check dependencies against thresholds (exit code based)
    Check {
        /// Minimum acceptable health score (0-100)
        #[arg(long, default_value = "60", value_parser = clap::value_parser!(u8).range(0..=100))]
        min_health_score: u8,

        /// Fail on copyleft licenses
        #[arg(long)]
        fail_on_copyleft: bool,

        /// Fail on unknown licenses
        #[arg(long)]
        fail_on_unknown_license: bool,
    },
}

#[derive(Clone, Debug, ValueEnum)]
enum ReportFormat {
    Json,
    #[value(alias = "md")]
    Markdown,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose);

    // Load configuration
    let mut config = if let Some(config_path) = &cli.config {
        match load_config(config_path) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("{} Failed to load config: {}", "Error:".red().bold(), e);
                process::exit(1);
            }
        }
    } else {
        AuditConfig::default()
    };

    // Add ignored dependencies from CLI
    for dep in &cli.ignore_dependencies {
        config.ignored_dependencies.insert(dep.clone());
    }

    // Run audit
    let spinner = if cli.quiet {
        None
    } else {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap(),
        );
        pb.set_message("Auditing dependencies...");
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(pb)
    };

    let result = audit_project(&cli.project_path, &config).await;

    if let Some(pb) = spinner {
        pb.finish_and_clear();
    }

    let report = match result {
        Ok(report) => report,
        Err(e) => {
            eprintln!("{} Audit failed: {}", "Error:".red().bold(), e);
            process::exit(1);
        }
    };

    // Handle subcommand
    match cli.command {
        Commands::Scan {
            fail_threshold,
            detailed,
        } => {
            display_summary(&report);

            if detailed {
                println!();
                display_detailed(&report);
            }

            display_notices(&report);

            // Check threshold
            if let Some(threshold) = fail_threshold {
                let failing: Vec<_> = report
                    .dependencies
                    .iter()
                    .filter(|d| is_below_threshold(d, threshold))
                    .collect();

                if !failing.is_empty() {
                    eprintln!(
                        "\n{} {} dependencies below threshold {}:",
                        "Failed:".red().bold(),
                        failing.len(),
                        threshold
                    );
                    for dep in &failing {
                        eprintln!("  - {}", describe_score_failure(dep, threshold));
                    }
                    process::exit(1);
                }
            }
        }

        Commands::Report { format, output } => {
            display_notices(&report);

            let content = match format {
                ReportFormat::Json => generate_json_report(&report),
                ReportFormat::Markdown => generate_markdown_report(&report),
            };

            if let Some(output_path) = output {
                match std::fs::write(&output_path, content) {
                    Ok(_) => println!("Report written to: {}", output_path.display()),
                    Err(e) => {
                        eprintln!("{} Failed to write report: {}", "Error:".red().bold(), e);
                        process::exit(1);
                    }
                }
            } else {
                println!("{}", content);
            }
        }

        Commands::Check {
            min_health_score,
            fail_on_copyleft,
            fail_on_unknown_license,
        } => {
            let mut failures = Vec::new();

            display_notices(&report);

            for dep in &report.dependencies {
                // Check health score (unscored dependencies fail closed)
                if is_below_threshold(dep, min_health_score) {
                    failures.push(format!(
                        "  - {}",
                        describe_score_failure(dep, min_health_score)
                    ));
                }

                // Check copyleft
                if fail_on_copyleft && dep.license_risk == LicenseRisk::Copyleft {
                    failures.push(format!(
                        "  - {} v{}: copyleft license ({:?})",
                        dep.name, dep.version, dep.license
                    ));
                }

                // Check unknown license
                if fail_on_unknown_license && dep.license_risk == LicenseRisk::Unknown {
                    failures.push(format!(
                        "  - {} v{}: unknown/missing license",
                        dep.name, dep.version
                    ));
                }
            }

            if !failures.is_empty() {
                eprintln!(
                    "{} {} check failures:",
                    "Failed:".red().bold(),
                    failures.len()
                );
                for failure in failures {
                    eprintln!("{}", failure);
                }
                process::exit(1);
            } else {
                println!("{} All checks passed!", "Success:".green().bold());
            }
        }
    }
}

fn init_logging(verbose: bool) {
    let filter = if verbose {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    } else {
        EnvFilter::new("warn")
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn load_config(path: &PathBuf) -> Result<AuditConfig, Box<dyn std::error::Error>> {
    let content = std::fs::read_to_string(path)?;
    let config: AuditConfig = toml::from_str(&content)?;
    config.scoring_weights.validate()?;
    Ok(config)
}

/// Unscored (`Unknown`) dependencies count as failing: a gate that passes
/// because the network was down would be a gate that never fails.
fn is_below_threshold(dep: &DependencyHealth, threshold: u8) -> bool {
    dep.status == HealthStatus::Unknown || dep.health_score < threshold
}

fn describe_score_failure(dep: &DependencyHealth, threshold: u8) -> String {
    if dep.status == HealthStatus::Unknown {
        format!(
            "{} v{}: could not be scored (no metadata available)",
            dep.name, dep.version
        )
    } else {
        format!(
            "{} v{}: health score {} < {}",
            dep.name, dep.version, dep.health_score, threshold
        )
    }
}

/// Run-level notices go to stderr so they never corrupt a report on stdout.
fn display_notices(report: &AuditReport) {
    for notice in &report.notices {
        eprintln!("{} {}", "Note:".yellow().bold(), notice);
    }
}

fn display_summary(report: &AuditReport) {
    println!("\n{}", "=== Audit Summary ===".bold());
    println!("Project: {}", report.project_name.cyan());
    println!("Total dependencies: {}", report.summary.total_dependencies);
    println!();

    let total = report.summary.total_dependencies;
    let pct = |n: usize| -> f32 {
        if total == 0 {
            0.0
        } else {
            (n as f32 / total as f32) * 100.0
        }
    };

    println!("Health Status:");
    println!(
        "  {} {} ({:.1}%)",
        "●".green(),
        format!("Healthy: {}", report.summary.healthy).green(),
        pct(report.summary.healthy)
    );
    println!(
        "  {} {} ({:.1}%)",
        "●".yellow(),
        format!("Warning: {}", report.summary.warning).yellow(),
        pct(report.summary.warning)
    );
    println!(
        "  {} {} ({:.1}%)",
        "●".truecolor(255, 165, 0), // Orange
        format!("Stale: {}", report.summary.stale).truecolor(255, 165, 0),
        pct(report.summary.stale)
    );
    println!(
        "  {} {} ({:.1}%)",
        "●".red(),
        format!("Risky: {}", report.summary.risky).red(),
        pct(report.summary.risky)
    );
    if report.summary.unknown > 0 {
        println!(
            "  {} {} ({:.1}%)",
            "●".dimmed(),
            format!("Unknown: {}", report.summary.unknown).dimmed(),
            pct(report.summary.unknown)
        );
    }
    println!();

    println!(
        "Average health score: {:.1}",
        report.summary.average_health_score
    );
    println!("License issues: {}", report.summary.license_issues);
    println!(
        "High footprint dependencies: {}",
        report.summary.high_footprint_count
    );
}

fn display_detailed(report: &AuditReport) {
    println!("{}", "=== Detailed Results ===".bold());

    for dep in &report.dependencies {
        let status_str = match dep.status {
            HealthStatus::Healthy => dep.status.to_string().green(),
            HealthStatus::Warning => dep.status.to_string().yellow(),
            HealthStatus::Stale => dep.status.to_string().truecolor(255, 165, 0),
            HealthStatus::Risky => dep.status.to_string().red(),
            HealthStatus::Unknown => dep.status.to_string().dimmed(),
        };

        let score = if dep.status == HealthStatus::Unknown {
            "n/a".to_string()
        } else {
            dep.health_score.to_string()
        };
        println!(
            "\n{} v{} [{}] Score: {}",
            dep.name.bold(),
            dep.version,
            status_str,
            score
        );

        if let Some(license) = &dep.license {
            println!("  License: {} ({})", license, dep.license_risk);
        }

        if let Some(footprint) = dep.footprint_risk {
            println!("  Footprint risk: {:.2}", footprint);
        }

        if !dep.warnings.is_empty() {
            println!("  Warnings:");
            for warning in &dep.warnings {
                println!("    - {}", warning.yellow());
            }
        }
    }
}

fn generate_json_report(report: &AuditReport) -> String {
    serde_json::to_string_pretty(report).unwrap_or_else(|e| {
        eprintln!("Failed to serialize report: {}", e);
        process::exit(1);
    })
}

fn generate_markdown_report(report: &AuditReport) -> String {
    let mut md = String::new();

    md.push_str(&format!(
        "# Dependency Audit Report: {}\n\n",
        report.project_name
    ));
    md.push_str(&format!("**Generated:** {}\n\n", report.timestamp));

    md.push_str("## Summary\n\n");
    md.push_str(&format!(
        "- Total dependencies: {}\n",
        report.summary.total_dependencies
    ));
    md.push_str(&format!("- Healthy: {}\n", report.summary.healthy));
    md.push_str(&format!("- Warning: {}\n", report.summary.warning));
    md.push_str(&format!("- Stale: {}\n", report.summary.stale));
    md.push_str(&format!("- Risky: {}\n", report.summary.risky));
    md.push_str(&format!("- Unknown: {}\n", report.summary.unknown));
    md.push_str(&format!(
        "- Average health score: {:.1}\n",
        report.summary.average_health_score
    ));
    md.push_str(&format!(
        "- License issues: {}\n",
        report.summary.license_issues
    ));
    md.push_str(&format!(
        "- High footprint count: {}\n\n",
        report.summary.high_footprint_count
    ));

    if !report.notices.is_empty() {
        md.push_str("## Notices\n\n");
        for notice in &report.notices {
            md.push_str(&format!("- {}\n", notice));
        }
        md.push('\n');
    }

    md.push_str("## Dependencies\n\n");
    md.push_str("| Name | Version | Status | Score | License | Footprint |\n");
    md.push_str("|------|---------|--------|-------|---------|----------|\n");

    for dep in &report.dependencies {
        let score = if dep.status == HealthStatus::Unknown {
            "n/a".to_string()
        } else {
            dep.health_score.to_string()
        };
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.2} |\n",
            dep.name,
            dep.version,
            dep.status,
            score,
            dep.license.as_deref().unwrap_or("Unknown"),
            dep.footprint_risk.unwrap_or(0.0)
        ));
    }

    md
}
