# rust_secure_dependency_audit

[![Crates.io](https://img.shields.io/crates/v/rust_secure_dependency_audit.svg)](https://crates.io/crates/rust_secure_dependency_audit)
[![Documentation](https://docs.rs/rust_secure_dependency_audit/badge.svg)](https://docs.rs/rust_secure_dependency_audit)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/rust_secure_dependency_audit.svg)](#license)

Audit the dependencies of a Rust project for **maintenance risk, license
compliance, and supply-chain health** — usable both as a library and as a
`secure-audit` CLI for CI pipelines.

It is intentionally complementary to [`cargo-audit`] (CVE scanning) and
[`cargo-deny`] (advisories / bans): this crate focuses on *how healthy*
your dependencies are, not just whether they currently have a published
advisory.

[`cargo-audit`]: https://github.com/rustsec/rustsec/tree/main/cargo-audit
[`cargo-deny`]: https://github.com/EmbarkStudios/cargo-deny

---

## What it tells you

For every dependency in your `Cargo.lock`, the audit produces:

| Signal             | Sourced from                                              |
| ------------------ | --------------------------------------------------------- |
| **Health score**   | crates.io publish history + GitHub/GitLab repo activity   |
| **Status**         | `Healthy` / `Warning` / `Stale` / `Risky`                 |
| **License risk**   | SPDX expression → permissive / copyleft / proprietary     |
| **Footprint risk** | transitive dep count, feature count, build-deps           |
| **Security signals** | [OpenSSF Scorecard], `SECURITY.md`, yanked status      |

[OpenSSF Scorecard]: https://securityscorecards.dev/

You get a CLI summary, a JSON/Markdown report, and CI-friendly exit codes.

---

## Installation

### As a CLI

```bash
cargo install rust_secure_dependency_audit
```

This installs the `secure-audit` binary.

### As a library

```toml
[dependencies]
rust_secure_dependency_audit = "0.2"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

---

## Quick start

### CLI

```bash
# Audit the current directory and print a summary
secure-audit scan

# Show per-dependency detail
secure-audit scan --detailed

# Write a JSON report for downstream tooling
secure-audit report --format json --output audit.json

# CI gate: fail the build if anything scores below 60
secure-audit check --min-health-score 60 --fail-on-copyleft

# Skip dependencies you don't want to score (repeatable)
secure-audit scan --ignore some-build-tool --ignore another-dev-only-dep
```

> **Tip:** export `GITHUB_TOKEN` (and optionally `GITLAB_TOKEN`) before
> running to raise API rate limits. Without a token, GitHub allows only
> 60 requests/hour, which is not enough for medium-sized projects.

### Library

```rust
use rust_secure_dependency_audit::{audit_project, AuditConfig, HealthStatus};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AuditConfig::default();
    let report = audit_project(Path::new("."), &config).await?;

    for dep in &report.dependencies {
        if matches!(dep.status, HealthStatus::Risky | HealthStatus::Stale) {
            eprintln!("⚠  {} v{} — score {}", dep.name, dep.version, dep.health_score);
        }
    }

    Ok(())
}
```

See [`examples/basic_usage.rs`](examples/basic_usage.rs) and
[`examples/custom_config.rs`](examples/custom_config.rs) for more.

---

## CLI reference

`secure-audit` accepts these global options (usable with any subcommand):

| Option              | Default | Description                                           |
| ------------------- | ------- | ----------------------------------------------------- |
| `-p, --project-path` | `.`     | Path to the Rust project to audit                     |
| `-c, --config`      | —       | Path to a TOML config file (see [Configuration](#configuration)) |
| `--ignore <NAME>`   | —       | Skip a dependency (repeatable)                        |
| `-v, --verbose`     | `false` | Verbose logging                                       |
| `-q, --quiet`       | `false` | Suppress the progress spinner (good for CI)           |

### Subcommands

#### `scan`

Run an audit and print a colourised summary.

| Option               | Description                                                |
| -------------------- | ---------------------------------------------------------- |
| `--fail-threshold N` | Exit code 1 if any dependency scores below `N` (0–100)     |
| `--detailed`         | Print every dependency, not just the summary               |

#### `report`

Generate a report file.

| Option                | Default      | Description                  |
| --------------------- | ------------ | ---------------------------- |
| `-f, --format <fmt>`  | `markdown`   | `json` or `markdown` (`md`)  |
| `-o, --output <file>` | stdout       | Write the report to a file   |

#### `check`

CI gate. Exits with code 1 if any check fails.

| Option                       | Default | Description                                |
| ---------------------------- | ------- | ------------------------------------------ |
| `--min-health-score N`       | `60`    | Minimum acceptable health score            |
| `--fail-on-copyleft`         | `false` | Fail if any dependency uses a copyleft licence |
| `--fail-on-unknown-license`  | `false` | Fail on missing / unrecognised licences    |

---

## How scoring works

Each dependency gets a 0–100 **health score**, a weighted average of five
component scores. **The defaults sum to 1.0:**

| Component       | Default weight | What it measures                                                |
| --------------- | -------------- | --------------------------------------------------------------- |
| **Recency**     | 0.35           | Days since the last publish / commit                            |
| **Maintenance** | 0.25           | Repo activity, archive status, open-issues load                 |
| **Community**   | 0.15           | Authors, stars, contributor count                               |
| **Stability**   | 0.10           | Number of published versions, download count                    |
| **Security**    | 0.15           | OpenSSF Scorecard (preferred); else `SECURITY.md` heuristic     |

The score then maps to a status:

| Score    | Status      |
| -------- | ----------- |
| 80 – 100 | 🟢 Healthy  |
| 60 – 79  | 🟡 Warning  |
| 40 – 59  | 🟠 Stale    |
| 0 – 39   | 🔴 Risky    |

Yanked crates are capped at 10 regardless of any other signal.

### License risk buckets

Licences are normalised through [SPDX] and bucketed:

- **Permissive** — MIT, Apache, BSD, ISC, Zlib, Unlicense, …
- **Copyleft** — GPL, LGPL, AGPL, MPL, EUPL, OSL, EPL, CDDL, CC-BY-SA
- **Proprietary** — anything matching "proprietary", "commercial", "all rights reserved"
- **Unknown** — missing or unrecognised expressions

[SPDX]: https://spdx.org/licenses/

### Footprint risk (0.0 – 1.0)

Useful for embedded, mobile, or WASM targets where binary size matters.
Calculated from:

- transitive dependency count (40%)
- feature count (30%)
- build-script dependency count (30%)

---

## Configuration

Pass a TOML config file with `--config`. All keys are optional — anything
you omit falls back to defaults.

```toml
[scoring_weights]
recency     = 0.35
maintenance = 0.25
community   = 0.15
stability   = 0.10
security    = 0.15

[staleness_thresholds]
stale_days      = 180   # >180 days idle → "stale"
risky_days      = 365   # >365 days idle → "risky"
min_maintainers = 2

[license_policy]
allowed_licenses   = ["MIT", "Apache-2.0", "BSD-3-Clause"]
forbidden_licenses = ["AGPL-3.0"]
warn_on_copyleft   = true
warn_on_unknown    = true

[footprint_thresholds]
max_transitive_deps = 50
max_footprint_risk  = 0.7

[network]
timeout_secs     = 30
max_retries      = 3
request_delay_ms = 100
enable_openssf   = true
```

Then:

```bash
secure-audit scan --config audit-config.toml
```

If `scoring_weights` doesn't sum to exactly 1.0, call
`ScoringWeights::validate()` from the library to surface the error, or
`ScoringWeights::normalize()` to rescale.

### Environment variables

| Variable        | Effect                                                |
| --------------- | ----------------------------------------------------- |
| `GITHUB_TOKEN`  | Authenticated GitHub API calls (5000 req/h)           |
| `GITLAB_TOKEN`  | Authenticated GitLab API calls                        |
| `RUST_LOG`      | Standard `tracing` filter (combine with `--verbose`)  |

---

## Caveats

- **Network required.** Scoring relies on crates.io, GitHub, GitLab, and
  OpenSSF APIs. Air-gapped environments aren't supported.
- **Heuristics, not audits.** A high score means *signals look healthy*,
  not that the code is secure. Pair this tool with `cargo-audit` and
  manual review for anything critical.
- **Rate limits.** Set `GITHUB_TOKEN` for non-trivial projects.

---

## Contributing

PRs welcome. Areas where help is especially useful:

- Additional heuristics (e.g. release cadence, breaking-change frequency)
- More Git platforms (Gitea, Codeberg, sourcehut)
- Local caching of API responses
- Integration with `cargo-audit` / `cargo-deny` advisory data

Please open an issue first for non-trivial changes:
<https://github.com/emorilebo/rust_secure_dependency_audit/issues>

---

## License

Dual-licensed under either of

- MIT — [LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>
- Apache 2.0 — [LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>

at your option. Contributions are accepted under the same dual licence
unless you explicitly state otherwise.
