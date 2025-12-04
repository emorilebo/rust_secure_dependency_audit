# rust_secure_dependency_audit

[![Crates.io](https://img.shields.io/crates/v/rust_secure_dependency_audit.svg)](https://crates.io/crates/rust_secure_dependency_audit)
[![Documentation](https://docs.rs/rust_secure_dependency_audit/badge.svg)](https://docs.rs/rust_secure_dependency_audit)
[![License](https://img.shields.io/crates/l/rust_secure_dependency_audit.svg)](LICENSE)

A comprehensive tool for auditing Rust project dependencies, providing insights into health, maintenance status, license compliance, and supply-chain risks.

## Why This Tool?

Modern software projects depend on dozens or hundreds of external crates. While Rust's ecosystem is generally well-maintained, dependencies can become:
- **Stale**: Not updated in months or years
- **Unmaintained**: Original authors have moved on
- **Risky**: Security issues, licensing problems, or abandoned repositories
- **Bloated**: Excessive footprint for embedded/mobile projects

This tool helps you identify and mitigate these risks by analyzing:
- 📊 **Health scoring**: Weighted algorithm considering recency, maintenance, community, and stability
- 📜 **License analysis**: Categorize licenses (permissive, copyleft, proprietary) and detect compliance issues
- 📦 **Footprint estimation**: Identify dependencies that may bloat your binary (useful for embedded/mobile)
- 🔍 **Metadata aggregation**: Fetch data from crates.io, GitHub, and GitLab

## Features

- **Multi-source metadata**: Combines crates.io, GitHub, and GitLab data for comprehensive analysis
- **Configurable scoring**: Customize weights and thresholds for your project's needs
- **Both library and CLI**: Use as a library in your tools or run standalone
- **Fast parallel processing**: Concurrent API calls with rate-limiting protection
- **Multiple output formats**: JSON and Markdown reports
- **CI/CD integration**: Exit codes based on thresholds for automated checks

## Installation

### As a CLI tool

```bash
cargo install rust_secure_dependency_audit
```

### As a library

Add to your `Cargo.toml`:

```toml
[dependencies]
rust_secure_dependency_audit = "0.1"
```

## Quick Start

### CLI Usage

**Scan your project:**
```bash
secure-audit scan
```

**Generate a JSON report:**
```bash
secure-audit report --format json --output audit.json
```

**Check with thresholds (for CI):**
```bash
secure-audit check --min-health-score 60 --fail-on-copyleft
```

**Scan with failure threshold:**
```bash
secure-audit scan --fail-threshold 70
```

**Ignore specific dependencies:**
```bash
secure-audit scan --ignore build-script-deps --ignore dev-only-tool
```

### Library Usage

```rust
use rust_secure_dependency_audit::{audit_project, AuditConfig, HealthStatus};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AuditConfig::default();
    let report = audit_project(Path::new("."), &config).await?;
    
    for dep in report.dependencies {
        match dep.status {
            HealthStatus::Risky => {
                eprintln!("⚠️  {} v{}: score {}", dep.name, dep.version, dep.health_score);
            }
            _ => {}
        }
    }
    
    Ok(())
}
```

## How It Works

### Health Scoring Algorithm

Each dependency receives a health score (0-100) based on weighted factors:

1. **Recency (40%)**: Days since last publish/commit
   - Updated within 30 days: 100
   - Within 3 months: 90
   - Within 6 months: 80
   - Within 1 year: 60
   - Within 2 years: 30
   - Older: 10

2. **Maintenance (30%)**: Repository activity
   - Archived repositories: 0
   - Active issues management: +25
   - Recent commits: +25

3. **Community (20%)**: Contributors and engagement
   - Number of authors/maintainers
   - GitHub stars
   - Contributor count

4. **Stability (10%)**: Version history
   - Number of published versions
   - Download count

5. **Security (15%)**: Security practices
   - **OpenSSF Scorecard**: 0-10 score mapped to 0-100
   - **Security Policy**: Presence of `SECURITY.md` (+20 points)
   - **Yanked Status**: Yanked crates receive a massive penalty (max score 10)

Scores are then categorized:
- **80-100**: Healthy 🟢
- **60-79**: Warning 🟡
- **40-59**: Stale 🟠
- **0-39**: Risky 🔴

### License Analysis

Licenses are categorized into:
- **Permissive**: MIT, Apache, BSD, ISC, etc.
- **Copyleft**: GPL, LGPL, AGPL, MPL, etc.
- **Proprietary**: Commercial, private licenses
- **Unknown**: Missing or unrecognized

You can configure:
- Allowed/forbidden license lists
- Warnings on copyleft or unknown licenses

### Footprint Estimation

Calculates a footprint risk score (0.0-1.0) based on:
- **Transitive dependency count** (40%)
- **Feature count** (30%)
- **Build dependency complexity** (30%)

Useful for embedded, mobile, or WASM projects where binary size matters.

## Configuration

### TOML Configuration File

Create a config file (e.g., `audit-config.toml`):

```toml
[scoring_weights]
recency = 0.50
maintenance = 0.30
community = 0.15
community = 0.15
stability = 0.10
security = 0.15

[staleness_thresholds]
stale_days = 180  # 6 months
risky_days = 365  # 1 year
min_maintainers = 2

[license_policy]
allowed_licenses = ["MIT", "Apache-2.0", "BSD-3-Clause"]
forbidden_licenses = ["AGPL-3.0"]
warn_on_copyleft = true
warn_on_unknown = true

[footprint_thresholds]
max_transitive_deps = 50
max_footprint_risk = 0.7

[network]
timeout_secs = 30
max_retries = 3
max_retries = 3
request_delay_ms = 100
enable_openssf = true
```

Use it:
```bash
secure-audit scan --config audit-config.toml
```

### Environment Variables

- `GITHUB_TOKEN`: GitHub personal access token (for higher API rate limits)
- `GITLAB_TOKEN`: GitLab personal access token

## CLI Reference

### Global Options
- `--project-path <PATH>`: Path to Rust project (default: current directory)
- `--config <FILE>`: Custom TOML configuration file
- `--ignore <CRATE>`: Ignore specific dependencies (repeatable)
- `--verbose`: Enable verbose logging

### Subcommands

#### `scan`
Run full audit and display summary.

Options:
- `--fail-threshold <SCORE>`: Exit with error if any dependency scores below threshold
- `--detailed`: Show detailed information for each dependency

#### `report`
Generate detailed audit report.

Options:
- `--format <FORMAT>`: Output format (`json` or `markdown`)
- `--output <FILE>`: Write to file (default: stdout)

#### `check`
Check dependencies against thresholds (for CI).

Options:
- `--min-health-score <SCORE>`: Minimum acceptable score (default: 60)
- `--fail-on-copyleft`: Fail on copyleft licenses
- `--fail-on-unknown-license`: Fail on unknown/missing licenses

## Examples

Check the `examples/` directory:
- [`basic_usage.rs`](examples/basic_usage.rs): Simple audit with default config
- [`custom_config.rs`](examples/custom_config.rs): Custom configuration and filtering

Run examples:
```bash
cargo run --example basic_usage
cargo run --example custom_config
```

## Limitations & Caveats

### Rate Limiting
- **crates.io**: Generally permissive, but may throttle excessive requests
- **GitHub**: 60 requests/hour unauthenticated, 5000/hour with token
- **GitLab**: Similar limits

**Recommendation**: Set `GITHUB_TOKEN` environment variable to increase limits.

### Heuristics Are Not Perfect
- Scoring is based on observable metrics, not code quality audits
- A high score doesn't guarantee security
- Manual review is still recommended for critical dependencies

### Network Dependency
- Requires internet access to fetch metadata
- May fail in air-gapped environments
- Use `--ignore` to skip problematic dependencies

### Not a Replacement for `cargo-audit`
This tool focuses on **maintenance risk**, not known security vulnerabilities. Use in combination with:
- [`cargo-audit`](https://github.com/rustsec/rustsec/tree/main/cargo-audit): CVE scanning
- [`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny): License and advisory checks

## Contributing

Contributions are welcome! Areas for improvement:
- Additional heuristics for health scoring
- Support for more Git platforms (Gitea, etc.)
- Persistent caching of API responses
- Integration with advisory databases

Please open an issue or pull request on [GitHub](https://github.com/emorilebo/rust_secure_dependency_audit).

## License

Licensed under either of:
- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

at your option.

## Acknowledgments

Built with:
- [`cargo_metadata`](https://crates.io/crates/cargo_metadata): Cargo project parsing
- [`reqwest`](https://crates.io/crates/reqwest): HTTP client
- [`clap`](https://crates.io/crates/clap): CLI framework
- [`tokio`](https://crates.io/crates/tokio): Async runtime
