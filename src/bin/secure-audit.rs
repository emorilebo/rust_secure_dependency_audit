//! CLI tool for auditing Rust dependencies

use clap::{Parser, Subcommand};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use rust_secure_dependency_audit::{
    audit_project, AuditConfig, AuditReport, HealthStatus, LicenseRisk,
};
use std::path::PathBuf;
use std::process;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Parser)]
#[command(name = "secure-audit")]
#[command(about = "Audit Rust project dependencies for health, security, and maintenance risks", long_about = None)]
#[command(version)]
struct Cli {
    /// Path to the Rust project to audit
    #[arg(short = 'p', long, default_value = ".")]
    project_path: PathBuf,

    /// Path to custom configuration file (TOML)
    #[arg(short = 'c', long)]
    config: Option<PathBuf>,

    /// Dependencies to ignore (can be specified multiple times)
    #[arg(long = "ignore")]
    ignore_dependencies: Vec<String>,

    /// Enable verbose logging
    #[arg(short = 'v', long)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run full audit and display summary
    Scan {
        /// Fail if any dependency has a health score below this threshold (0-100)
        #[arg(long)]
        fail_threshold: Option<u8>,

        /// Display detailed information for each dependency
        #[arg(long)]
        detailed: bool,
    },

    /// Generate detailed audit report
    Report {
        /// Output format
        #[arg(short = 'f', long, default_value = "markdown")]
        format: ReportFormat,

        /// Output file (default: stdout)
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },

    /// Check dependencies against thresholds (exit code based)
    Check {
        /// Minimum acceptable health score (0-100)
        #[arg(long, default_value = "60")]
        min_health_score: u8,

        /// Fail on copyleft licenses
        #[arg(long)]
        fail_on_copyleft: bool,

        /// Fail on unknown licenses
        #[arg(long)]
        fail_on_unknown_license: bool,
    },
}

#[derive(Clone, Debug)]
enum ReportFormat {
    Json,
    Markdown,
}

impl std::str::FromStr for ReportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(ReportFormat::Json),
            "markdown" | "md" => Ok(ReportFormat::Markdown),
            _ => Err(format!("Unknown format: {}", s)),
        }
    }
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
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Auditing dependencies...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));

    let result = audit_project(&cli.project_path, &config).await;

    spinner.finish_and_clear();

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

            // Check threshold
            if let Some(threshold) = fail_threshold {
                let failing: Vec<_> = report
                    .dependencies
                    .iter()
                    .filter(|d| d.health_score < threshold)
                    .collect();

                if !failing.is_empty() {
                    eprintln!(
                        "\n{} {} dependencies below threshold {}:",
                        "Failed:".red().bold(),
                        failing.len(),
                        threshold
                    );
                    for dep in &failing {
                        eprintln!(
                            "  - {} v{}: score {}",
                            dep.name, dep.version, dep.health_score
                        );
                    }
                    process::exit(1);
                }
            }
        }

        Commands::Report { format, output } => {
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

            for dep in &report.dependencies {
                // Check health score
                if dep.health_score < min_health_score {
                    failures.push(format!(
                        "  - {} v{}: health score {} < {}",
                        dep.name, dep.version, dep.health_score, min_health_score
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
                eprintln!("{} {} check failures:", "Failed:".red().bold(), failures.len());
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
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info"))
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
    Ok(config)
}

fn display_summary(report: &AuditReport) {
    println!("\n{}", "=== Audit Summary ===".bold());
    println!("Project: {}", report.project_name.cyan());
    println!("Total dependencies: {}", report.summary.total_dependencies);
    println!();

    println!("Health Status:");
    println!(
        "  {} {} ({:.1}%)",
        "●".green(),
        format!("Healthy: {}", report.summary.healthy).green(),
        (report.summary.healthy as f32 / report.summary.total_dependencies as f32) * 100.0
    );
    println!(
        "  {} {} ({:.1}%)",
        "●".yellow(),
        format!("Warning: {}", report.summary.warning).yellow(),
        (report.summary.warning as f32 / report.summary.total_dependencies as f32) * 100.0
    );
    println!(
        "  {} {} ({:.1}%)",
        "●".truecolor(255, 165, 0), // Orange
        format!("Stale: {}", report.summary.stale).truecolor(255, 165, 0),
        (report.summary.stale as f32 / report.summary.total_dependencies as f32) * 100.0
    );
    println!(
        "  {} {} ({:.1}%)",
        "●".red(),
        format!("Risky: {}", report.summary.risky).red(),
        (report.summary.risky as f32 / report.summary.total_dependencies as f32) * 100.0
    );
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
        };

        println!(
            "\n{} v{} [{}] Score: {}",
            dep.name.bold(),
            dep.version,
            status_str,
            dep.health_score
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

    md.push_str(&format!("# Dependency Audit Report: {}\n\n", report.project_name));
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

    md.push_str("## Dependencies\n\n");
    md.push_str("| Name | Version | Status | Score | License | Footprint |\n");
    md.push_str("|------|---------|--------|-------|---------|----------|\n");

    for dep in &report.dependencies {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} | {:.2} |\n",
            dep.name,
            dep.version,
            dep.status,
            dep.health_score,
            dep.license.as_deref().unwrap_or("Unknown"),
            dep.footprint_risk.unwrap_or(0.0)
        ));
    }

    md
}
