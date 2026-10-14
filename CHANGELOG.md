# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-10-14

### Breaking
- `HealthStatus` has a new `Unknown` variant for dependencies with no
  crates.io or repository metadata (previously scored ~25 and shown as
  `Risky`). Exhaustive `match`es need a new arm.
- `ComponentScores` fields are now `Option<f32>`; `None` means "not
  measured" and the component is left out of the weighted average (weights
  are renormalised) instead of counting as 0 or a fake neutral 50.
- New fields: `AuditReport::notices`, `AuditSummary::unknown`.
- Local path dependencies are no longer scored (they were always `Risky`);
  they are skipped and counted in a notice.
- `check` and `scan --fail-threshold` treat `Unknown` dependencies as
  failures, so a network outage cannot make a CI gate pass.
- Config files now reject unknown keys instead of ignoring them.

### Fixed
- **GitHub rate limiting (no token):** a `403` was treated as a rate limit
  even when it was not, and once the limit was hit every remaining
  dependency still called the API. The client now recognises GitHub's
  rate-limit signals (`x-ratelimit-remaining: 0`, `retry-after`, `429`),
  stops calling GitHub for the rest of the run, scores the remaining crates
  without repository signals, and prints one notice with the reset time and
  the `GITHUB_TOKEN` hint. A rejected token (`401`) is handled the same way.
- **GitHub request volume:** up to six API calls per dependency are now at
  most two per *repository*: lookups are cached per `owner/repo`, and
  `SECURITY.md` detection uses `raw.githubusercontent.com` (including the
  owner's `.github` repository) instead of the community-profile and
  contents API.
- **crates.io politeness:** requests were issued about ten per second; they
  are now paced to at most one per second as the crates.io data-access
  policy requires, cached per crate name, retried on `429`/`5xx` honouring
  `Retry-After`, and sent with a User-Agent that links to this repository.
  `request_delay_ms` now defaults to `1000`.
- **Contributor count was always 1:** the `Link` header parser matched the
  `page=` inside `per_page=`.
- **Maintainer count was always 0:** crates.io no longer returns `authors`;
  the count now uses distinct crates.io publishers.
- **Unknown signals were penalised:** a failed contributors or
  `SECURITY.md` lookup was recorded as "0 contributors" / "no policy".
- **Config files:** every section and key was mandatory, so the README
  example failed with `missing field 'ignored_dependencies'`; all keys now
  default. `GITHUB_TOKEN` is honoured when a config file has a `[network]`
  section without a token. The CLI rejects scoring weights that do not sum
  to 1.0.
- **License analysis** now parses SPDX expressions (the `spdx` dependency was
  declared but unused): `AND` with a copyleft license is copyleft, `OR` lets
  the permissive option win, and Unicode-3.0, BSL-1.0, CC0-1.0 and
  CDLA-Permissive-2.0 are permissive (the 18 `Unicode-3.0` crates in this
  crate's own tree, the ICU stack behind `url`, were "Unknown" before). Allow/deny lists match license ids by prefix, so
  `GPL-3.0` no longer matches `LGPL-3.0`. Licenses are read from the local
  manifest first, so they work for git/path dependencies and offline.
- **Direct dependencies** were detected by name, marking the wrong version
  as direct when several versions of a crate are in the tree.
- **Alternative registries** were looked up on crates.io, where a crate of
  the same name may be a different crate; they are now `Unknown` unless
  their repository URL can be scored.
- **OpenSSF lookups** failed for repository URLs ending in `.git` or
  pointing at a sub-directory.
- **Panics:** a `GITHUB_TOKEN`/`GITLAB_TOKEN` with a trailing newline or
  other non-header characters panicked inside a task and silently dropped
  the dependency from the report.
- Git dependencies are scored from their GitHub/GitLab source URL; git
  source URLs no longer keep the `#commit` fragment.
- `--fail-threshold` and `--min-health-score` reject values above 100.
- `cargo metadata` runs once (with all features) instead of three times,
  and footprint analysis uses the same resolution as the dependency list.
- The self-audit unit test no longer runs on every `cargo test`; it made
  hundreds of network requests and exhausted the unauthenticated GitHub
  quota. Network tests are `#[ignore]`; new offline tests use mock servers.

### Changed
- Recency without repository data uses the crate's newest release rather
  than the locked version's release date, matching what the GitHub path
  measures (repository-level activity).
- `report --help` lists the accepted formats; `--format` is case-insensitive.
- Notices are printed to stderr, so `report --format json` on stdout stays
  valid JSON. Markdown reports include a Notices section.
- Tokens are redacted from `NetworkConfig`'s `Debug` output and never
  serialized.
- Re-exported `AuditSummary`, `ComponentScores`, `DependencyMetrics`,
  `DependencySource`, `RepositoryMetrics` and `AuditConfigBuilder`.
- `anyhow` moved to dev-dependencies (only used in doc examples).
- README rewritten: rate-limit behaviour, exit codes, FAQ, limitations.

## [0.2.2] - 2026-05-19

### Fixed
- **Packaging:** the published tarball no longer ships `error.log` or
  `PUBLISH.md`. The `.github/` workflows and `tests/fixtures/` are now
  also excluded, keeping the crate lean.
- **GitHub security-policy detection:** `check_security_policy` was
  looking up the wrong JSON key on GitHub's community-profile endpoint
  (`security_note` instead of `security`) and was issuing the same
  community-profile request three times inside a loop. It now makes one
  community-profile call and falls back to a direct `SECURITY.md` lookup
  if needed.
- **CLI argument handling:** `--project-path`, `--config`, `--ignore`,
  `--verbose`, and `--quiet` are now declared as `global` clap arguments,
  so they can be passed either before or after a subcommand
  (e.g. `secure-audit scan --project-path ./foo` now works).
- **CLI summary:** `display_summary` no longer divides by zero when a
  project has no dependencies — percentages render as `0.0%` instead of
  `NaN%`.

### Changed
- Bumped `rust-version` to `1.74` to formalise the minimum supported
  toolchain.
- Annotated deserialization-only structs in `metadata::crates_io` and
  `metadata::openssf` with `#[allow(dead_code)]`, keeping the upstream
  schema captured without producing warnings.
- `AuditConfig::default` is now `#[derive(Default)]` instead of a manual
  impl; behaviour is unchanged.
- Cleaned up clippy lints (`collapsible_match`, `single_match`,
  `field_reassign_with_default`, unused imports) — `cargo clippy --all-targets -- -D warnings` is now clean.

### Docs
- **README rewritten** for clarity: corrected the scoring weights table
  (the old listing summed to 115%; defaults are now correctly shown as
  35/25/15/10/15), fixed the duplicate keys in the sample TOML config,
  updated the install snippet to track the `0.2` line, and reorganised
  sections so the CLI reference and library usage are easier to find.
- CHANGELOG entry for 0.1.1 corrected — weights never summed to 1.0 in
  the old form; the table now matches what the code actually computes.

## [0.2.1] - 2025-12-08

### Added
- New `--quiet` / `-q` flag to suppress progress spinner output (useful for CI/CD pipelines)

### Fixed
- Clippy warnings: removed unused imports in `parser.rs`
- Clippy warnings: replaced `len() > 0` with `!is_empty()` in `audit.rs`
- Fixed hardcoded version string in OpenSSF client user agent (now uses dynamic `CARGO_PKG_VERSION`)
- Added `#[allow(unused_imports)]` for intentionally exported but unused `OpenSSFClient`

### Improved
- Code quality improvements throughout the codebase

## [0.1.2] - 2025-11-26

### Fixed
- Compilation error in `ApiError` struct definition
- Ambiguous numeric type in scoring algorithm
- Clippy lints in `scoring.rs` (`Option::map` manual implementation)
- Async execution in library doctests
- Updated documentation URLs to correct username

## [0.1.1] - 2024-01-15

### Added
- Initial release
- Core audit functionality for Rust dependencies
- Health scoring algorithm with configurable weights
  - Recency score (40% weight)
  - Maintenance score (30% weight)
  - Community score (20% weight)
  - Stability score (10% weight)
- License analysis and categorization
  - Permissive, copyleft, proprietary, unknown classifications
  - Configurable license policies
- Footprint risk estimation
  - Transitive dependency counting
  - Feature and build dependency analysis
- Metadata fetching from multiple sources
  - crates.io API integration
  - GitHub API integration
  - GitLab API integration
- CLI tool (`secure-audit`) with three subcommands
  - `scan`: Full audit with summary
  - `report`: Generate JSON or Markdown reports
  - `check`: CI-friendly threshold checking
- Library API for programmatic usage
- Configurable thresholds and weights
- Support for ignoring specific dependencies
- Retry logic and rate limit handling
- Comprehensive documentation
- Example programs demonstrating usage
- Integration tests

[Unreleased]: https://github.com/emorilebo/rust_secure_dependency_audit/compare/v0.2.2...HEAD
[0.2.2]: https://github.com/emorilebo/rust_secure_dependency_audit/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/emorilebo/rust_secure_dependency_audit/compare/v0.2.0...v0.2.1
[0.1.2]: https://github.com/emorilebo/rust_secure_dependency_audit/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/emorilebo/rust_secure_dependency_audit/releases/tag/v0.1.1
