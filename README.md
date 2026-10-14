# rust_secure_dependency_audit — dependency health, license and supply-chain risk audit for Rust projects

[![Crates.io](https://img.shields.io/crates/v/rust_secure_dependency_audit.svg)](https://crates.io/crates/rust_secure_dependency_audit)
[![Documentation](https://docs.rs/rust_secure_dependency_audit/badge.svg)](https://docs.rs/rust_secure_dependency_audit)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/rust_secure_dependency_audit.svg)](#license)
[![Downloads](https://img.shields.io/crates/d/rust_secure_dependency_audit.svg)](https://crates.io/crates/rust_secure_dependency_audit)
[![CI](https://github.com/emorilebo/rust_secure_dependency_audit/actions/workflows/ci.yml/badge.svg)](https://github.com/emorilebo/rust_secure_dependency_audit/actions/workflows/ci.yml)

`cargo audit` tells you which of your dependencies already have a CVE. This
crate tells you which ones are quietly working on one: archived repositories,
crates whose last release predates your current laptop, and licenses that
would make your legal team put down their coffee.

`rust_secure_dependency_audit` scores every crate in your `Cargo.lock` for
**maintenance health**, **SPDX license risk** and **footprint**, using
crates.io, GitHub/GitLab and the [OpenSSF Scorecard]. It ships as a library
and as the `secure-audit` CLI, with exit codes made for CI.

It complements [`cargo-audit`] (known vulnerabilities) and [`cargo-deny`]
(advisories, bans, license policy): those answer *"is anything known to be
broken?"*; this answers *"is anyone still looking after this?"*.

[`cargo-audit`]: https://github.com/rustsec/rustsec/tree/main/cargo-audit
[`cargo-deny`]: https://github.com/EmbarkStudios/cargo-deny
[OpenSSF Scorecard]: https://securityscorecards.dev/

---

## What it does

- **Health score (0–100) and status** for every registry and git dependency:
  `Healthy`, `Warning`, `Stale`, `Risky`, or `Unknown` when there was no data
  to score.
- **Maintenance signals:** last release / last push, archived repositories,
  open-issue load, crates.io publishers, stars, contributors, yanked versions.
- **Security signals:** OpenSSF Scorecard score, or failing that, whether the
  repository (or its owner's `.github` repository) has a `SECURITY.md`.
- **License risk:** SPDX expressions are parsed properly — `OR` lets you take
  the permissive option, `AND` means the copyleft term still applies — and
  bucketed as permissive / copyleft / proprietary / unknown, with optional
  allow and deny lists.
- **Footprint risk:** transitive dependency count, feature count and build
  dependencies, for size-sensitive targets.
- **Polite networking:** crates.io is called at most once per second (its
  published policy), each crate and repository is fetched once per run, and
  a GitHub rate limit degrades the report instead of breaking it.

---

## Installation

```bash
# CLI (installs the `secure-audit` binary)
cargo install rust_secure_dependency_audit

# Library
cargo add rust_secure_dependency_audit
cargo add tokio --features macros,rt-multi-thread
```

---

## Quick start

```bash
# Audit the project in the current directory
secure-audit scan

# Every dependency, with its warnings
secure-audit scan --detailed

# Machine-readable report
secure-audit report --format json --output audit.json

# CI gate: exit 1 if anything scores below 60 or is copyleft
secure-audit check --min-health-score 60 --fail-on-copyleft
```

Real output from `secure-audit scan` on a small test project (`serde`,
`failure`, `colored`, a git and a path dependency), run without a GitHub
token after the hourly quota was already used up:

```text
=== Audit Summary ===
Project: demo_app
Total dependencies: 37

Health Status:
  ● Healthy: 12 (32.4%)
  ● Warning: 9 (24.3%)
  ● Stale: 10 (27.0%)
  ● Risky: 5 (13.5%)
  ● Unknown: 1 (2.7%)

Average health score: 62.9
License issues: 1
High footprint dependencies: 0
Note: GitHub API rate limit reached (unauthenticated (60 requests/hour) quota, resets in about 40m). 36 dependencies were scored without GitHub repository data (maintenance and community components left out). Set GITHUB_TOKEN to raise the limit (a personal access token with no scopes is enough).
Note: 1 dependency could not be scored because no crates.io or repository metadata was available (status "Unknown").
Note: Skipped 1 local path dependency (your own code, not third-party).
```

`secure-audit` needs `cargo` on `PATH` (it runs `cargo metadata`). Expect a
run to take roughly one second per unique crate name — see
[Rate limits](#github-token-and-rate-limits) for why that is a feature.

---

## GitHub token and rate limits

Unauthenticated GitHub API access allows **60 requests per hour**. Each
repository costs two API requests (repository + contributors; GitHub's
redirect for a renamed repository adds one more);
`SECURITY.md` detection uses `raw.githubusercontent.com`, which is not
counted against that quota, and crates that share a repository (the
`windows-*` family, `serde`/`serde_derive`, …) share one lookup.

Without a token, medium-sized projects will still run out. When that
happens, `secure-audit`:

1. stops calling the GitHub API for the rest of the run (no retry storm);
2. scores the remaining crates from crates.io and OpenSSF data, leaving the
   repository-only components (maintenance, community) out of the weighted
   average instead of counting them as zero;
3. tags each affected dependency with a warning and prints one notice (see
   the [Quick start](#quick-start) output above).

Scores computed without repository data are still meaningful but less
informed: an archived repository, for example, can only be detected through
GitHub. For complete results, export a token:

```bash
export GITHUB_TOKEN=ghp_...   # classic or fine-grained, no scopes needed for public repos
secure-audit scan
```

A token GitHub rejects (HTTP 401) is reported the same way rather than
failing the run. Notices go to **stderr**, so `report --format json` on
stdout stays valid JSON; they are also included in the report's `notices`
field.

---

## CLI reference

Output of `secure-audit --help`:

```text
Audit Rust project dependencies for health, security, and maintenance risks

Usage: secure-audit [OPTIONS] <COMMAND>

Commands:
  scan    Run full audit and display summary
  report  Generate detailed audit report
  check   Check dependencies against thresholds (exit code based)
  help    Print this message or the help of the given subcommand(s)

Options:
  -p, --project-path <PROJECT_PATH>   Path to the Rust project to audit [default: .]
  -c, --config <CONFIG>               Path to custom configuration file (TOML)
      --ignore <IGNORE_DEPENDENCIES>  Dependencies to ignore (can be specified multiple times)
  -v, --verbose                       Enable verbose logging
  -q, --quiet                         Suppress progress output (quiet mode, useful for CI)
  -h, --help                          Print help
  -V, --version                       Print version
```

The global options work before or after the subcommand.

### `scan`

| Option                    | Description                                                              |
| ------------------------- | ------------------------------------------------------------------------ |
| `--fail-threshold <0-100>` | Exit 1 if any dependency scores below the threshold or could not be scored |
| `--detailed`              | Print every dependency with its license, footprint and warnings          |

### `report`

| Option                  | Default    | Description                                     |
| ----------------------- | ---------- | ----------------------------------------------- |
| `-f, --format <FORMAT>` | `markdown` | `json`, `markdown` (or `md`)                    |
| `-o, --output <OUTPUT>` | stdout     | Write the report to a file instead of stdout    |

### `check`

| Option                       | Default | Description                                                         |
| ---------------------------- | ------- | ------------------------------------------------------------------- |
| `--min-health-score <0-100>` | `60`    | Fail if any dependency scores below this or could not be scored    |
| `--fail-on-copyleft`         | off     | Fail if any dependency's license is copyleft                        |
| `--fail-on-unknown-license`  | off     | Fail on missing or unrecognised licenses                            |

Dependencies with status `Unknown` (no metadata could be fetched — for
example a git dependency on a self-hosted server, or a network outage) fail
`check` and `scan --fail-threshold`: a gate that passes because the network
was down is a gate that never fails. Use `--ignore <name>` for dependencies
you have reviewed by other means.

### Exit codes

| Code | Meaning                                                                      |
| ---- | ---------------------------------------------------------------------------- |
| `0`  | Audit completed and every requested check passed                             |
| `1`  | A check or threshold failed, or the audit could not run (bad path, bad config, `cargo metadata` error, unwritable output file) |
| `2`  | Invalid command-line usage (unknown flag, threshold outside 0–100, unknown format) |

---

## How scoring works

Each dependency gets five component scores (0–100). The health score is
their weighted average **over the components that could be measured**; a
component with no input data is left out and the remaining weights are
rescaled, the same way OpenSSF Scorecard ignores inconclusive checks.

| Component       | Default weight | Measures                                                                   | Needs                     |
| --------------- | -------------- | -------------------------------------------------------------------------- | ------------------------- |
| **Recency**     | 0.35           | Days since the last repository push, else the crate's newest release        | repository or crates.io   |
| **Maintenance** | 0.25           | Archived flag, open-issue count, recent pushes                               | repository                |
| **Community**   | 0.15           | Stars, contributors, plus distinct crates.io publishers                      | repository                |
| **Stability**   | 0.10           | Number of published versions, total downloads                                | crates.io                 |
| **Security**    | 0.15           | OpenSSF Scorecard (×10); else 70 with a `SECURITY.md`, 40 without            | OpenSSF or GitHub         |

| Score    | Status    |
| -------- | --------- |
| 80 – 100 | `Healthy` |
| 60 – 79  | `Warning` |
| 40 – 59  | `Stale`   |
| 0 – 39   | `Risky`   |
| —        | `Unknown` (no crates.io or repository data; score reported as 0) |

A yanked version is capped at 10. Local path dependencies (your own code)
are skipped and counted in a notice.

### License risk

License strings come from each package's own `Cargo.toml` (so they work
offline and match the locked version), falling back to crates.io. They are
parsed as [SPDX] expressions, accepting the `MIT/Apache-2.0` style crates.io
also accepts:

- **Permissive** — OSI-approved or FSF-free licenses that are not copyleft:
  MIT, Apache-2.0, BSD-*, ISC, Zlib, BSL-1.0, Unicode-3.0, CC0-1.0, …
- **Copyleft** — GPL, LGPL, AGPL, MPL, EPL, EUPL, OSL, CDDL, CC-BY-SA, …
- **Proprietary** — `LicenseRef-*` or free text mentioning "proprietary",
  "commercial", "private" or "all rights reserved"
- **Unknown** — missing, unparseable, or not recognised as either

`MIT OR GPL-3.0-only` is permissive (you may pick MIT);
`MIT AND GPL-3.0-only` is copyleft.

[SPDX]: https://spdx.org/licenses/

### Footprint risk (0.0 – 1.0)

Transitive dependency count (40%), feature count (30%) and build-dependency
count (30%). Useful for embedded, mobile and WASM targets where every crate
shows up in the binary size and the build time.

---

## Configuration

Pass a TOML file with `--config`. Every section and key is optional;
anything you leave out keeps its default.

```toml
ignored_dependencies = ["some-internal-crate"]   # top-level keys go before any [table]

[scoring_weights]          # must sum to 1.0
recency     = 0.35
maintenance = 0.25
community   = 0.15
stability   = 0.10
security    = 0.15

[staleness_thresholds]
stale_days = 180           # recency component drops to 30/100 after this many idle days
risky_days = 365           # ...and to 10/100 after this many

[license_policy]
allowed_licenses   = ["MIT", "Apache-2.0", "BSD-3-Clause", "Unicode-3.0"]
forbidden_licenses = ["AGPL-3.0"]   # prefix match: also matches AGPL-3.0-only / -or-later
warn_on_copyleft   = true
warn_on_unknown    = true

[footprint_thresholds]
max_transitive_deps = 50
max_footprint_risk  = 0.7

[network]
timeout_secs     = 30
max_retries      = 3
request_delay_ms = 1000    # crates.io pacing; values below 1000 are raised to 1000
enable_openssf   = true
```

```bash
secure-audit scan --config audit.toml
```

Unknown keys are rejected rather than silently ignored, and so is a config
whose `scoring_weights` do not sum to 1.0 (exit code 1 either way).
`staleness_thresholds.min_maintainers` is accepted but not used by the
scorer yet.

### Environment variables

| Variable       | Effect                                                                     |
| -------------- | -------------------------------------------------------------------------- |
| `GITHUB_TOKEN` | Authenticates GitHub API calls (also read when a config file has no token) |
| `GITLAB_TOKEN` | Authenticates GitLab API calls                                             |
| `RUST_LOG`     | `tracing` filter, used together with `--verbose`                          |

Tokens are never written to reports and are redacted from `Debug` output.

---

## Library usage

```rust,no_run
use rust_secure_dependency_audit::{audit_project, AuditConfig, HealthStatus};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = AuditConfig::default(); // reads GITHUB_TOKEN / GITLAB_TOKEN
    let report = audit_project(Path::new("."), &config).await?;

    for notice in &report.notices {
        eprintln!("note: {notice}");
    }

    for dep in &report.dependencies {
        if matches!(dep.status, HealthStatus::Risky | HealthStatus::Stale) {
            println!("{} v{}: score {}", dep.name, dep.version, dep.health_score);
        }
    }

    Ok(())
}
```

`AuditConfig::builder()` and [`examples/custom_config.rs`](examples/custom_config.rs)
show custom weights, thresholds and license policy. Full API docs are on
[docs.rs](https://docs.rs/rust_secure_dependency_audit).

---

## How it works

1. Runs `cargo metadata --all-features` once to get the resolved dependency
   graph, licenses and repository URLs.
2. For each registry crate, fetches `https://crates.io/api/v1/crates/<name>`
   (one request per unique name, paced to one per second, with a
   User-Agent that links back to this repository).
3. For each GitHub repository, fetches the repository and contributor count
   from the GitHub API and checks for `SECURITY.md` on
   `raw.githubusercontent.com`; GitLab repositories use the GitLab API.
4. Fetches the OpenSSF Scorecard result for the repository, if one exists.
5. Scores, categorises licenses, estimates footprint, and summarises.

---

## Limitations

- **Heuristics, not an audit.** A high score means the public signals look
  healthy, not that the code is correct or safe. Nothing here reads your
  dependencies' source code; it has enough on its plate reading their
  `Cargo.toml`. Pair it with `cargo-audit` / `cargo-deny` and human review.
- **Network required.** crates.io, GitHub, GitLab and OpenSSF are queried
  live; there is no offline mode or response cache yet. Offline, registry
  dependencies come back `Unknown`.
- **Finished crates look stale.** Small, stable crates that rarely need a
  release (`fnv`, `scopeguard`) score low on recency by design. Treat
  `Stale`/`Risky` as "worth a look", not "rip it out".
- **Crate-level, not version-level, recency.** An old major version of an
  actively maintained crate (say `syn` 1.x) scores like the crate as a whole.
- **Repository signals need GitHub or GitLab.** Other hosts (Codeberg,
  sourcehut, self-hosted) get crates.io and OpenSSF signals only; git
  dependencies on those hosts are `Unknown`.
- **Alternative registries** are not looked up on crates.io (a crate with
  the same name there may be a different crate); they are scored from their
  repository URL if it is on GitHub/GitLab, otherwise `Unknown`.
- **`cargo metadata` may update `Cargo.lock`** if it is missing or out of
  date, exactly as `cargo build` would.
- **Big projects are slow on purpose:** about one second per unique crate
  name, because that is what crates.io asks API clients to do.

---

## Roadmap

- Local cache of API responses between runs
- Version-level staleness (flag old major versions of active crates)
- Repository signals for Codeberg / Gitea / sourcehut
- Optional RustSec advisory data alongside the health score
- A config switch to let `check` skip, rather than fail, unscored dependencies

---

## FAQ

### How do I check if my Rust dependencies are still maintained?

Run `secure-audit scan --detailed` in the project directory. `rust_secure_dependency_audit`
scores each crate in `Cargo.lock` from its last release or repository push,
archived status, issue load, contributors and OpenSSF Scorecard, and labels
it `Healthy`, `Warning`, `Stale`, `Risky` or `Unknown`. Set `GITHUB_TOKEN`
first so repository signals are available for every crate.

### How do I fail a CI build on unmaintained or copyleft Rust dependencies?

Use `secure-audit check --min-health-score 60 --fail-on-copyleft`. It exits
with code 1 if any dependency scores below the threshold, could not be
scored, or has a copyleft license, and prints each failure to stderr. Add
`--fail-on-unknown-license` to also fail on missing or unrecognised licenses.

### What is the difference between rust_secure_dependency_audit, cargo-audit and cargo-deny?

`cargo-audit` reports crates with published RustSec advisories, and
`cargo-deny` enforces advisory, ban, source and license policies.
`rust_secure_dependency_audit` measures maintenance health — whether a crate
is still actively looked after — which is a leading indicator rather than a
list of known problems. They work well together.

### Does it work without a GitHub token?

Yes. Without `GITHUB_TOKEN` the GitHub API allows 60 requests per hour; when
that runs out, `rust_secure_dependency_audit` stops calling GitHub, scores the
remaining crates from crates.io and OpenSSF data without counting the missing
repository signals as zero, and prints a notice saying how many crates were
affected. A token gives complete results.

### How are SPDX license expressions like "MIT OR Apache-2.0" handled?

They are parsed with the `spdx` crate. With `OR` the least restrictive option
counts, so `MIT OR GPL-3.0-only` is permissive; with `AND` every term
applies, so `MIT AND GPL-3.0-only` is copyleft. Allow and deny lists in the
config are evaluated against the same expression tree.

### Can I use it as a Rust library instead of a CLI?

Yes. Call `audit_project(path, &AuditConfig::default()).await` from a Tokio
runtime to get an `AuditReport` with per-dependency scores, component
scores, license risk, footprint, warnings and run-level notices; the report
is `serde`-serialisable.

---

## Contributing

Issues and pull requests are welcome at
<https://github.com/emorilebo/rust_secure_dependency_audit>. Please open an
issue first for scoring changes — every heuristic is somebody's favourite
crate's worst day. Before sending a PR:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features                # offline; network tests are #[ignore]
cargo test --all-features -- --ignored   # optional, hits crates.io/GitHub
```

---

## Related projects by the same author

- [`npm_sentinel`](https://crates.io/crates/npm_sentinel) — detect supply-chain attacks in npm packages by analysing lifecycle scripts, dependencies and registry metadata.
- [`g-recon`](https://crates.io/crates/g-recon) — fast security scanner for GitHub repositories and web reconnaissance.
- [`rust_mobile_secrets_vault`](https://crates.io/crates/rust_mobile_secrets_vault) — encrypted secrets vault for mobile-backend and embedded Rust.
- [`rust_mobile_telemetry`](https://crates.io/crates/rust_mobile_telemetry) — tracing and metrics instrumentation for mobile-backend Rust services.

---

## About the author

> Built by **[Godfrey Lebo](https://godfreylebo.dev)** — Senior Backend & Product Engineer and founder of Sether.
> I build the backend, connect the product, and stay responsible for what happens after launch —
> lately that means open-source tooling for AI security and software supply-chain security.
>
> [Website](https://godfreylebo.dev) · [GitHub](https://github.com/emorilebo) · [X / Twitter](https://x.com/emori_lebo) · [LinkedIn](https://www.linkedin.com/in/godfreylebo) · [Email](mailto:emorylebo@gmail.com)

---

## License

Dual-licensed under either of

- MIT — [LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>
- Apache 2.0 — [LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>

at your option. Contributions are accepted under the same dual license
unless you explicitly state otherwise.
