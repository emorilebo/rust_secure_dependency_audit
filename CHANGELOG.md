# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/emorilebo/rust_secure_dependency_audit/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/emorilebo/rust_secure_dependency_audit/compare/v0.2.0...v0.2.1
[0.1.2]: https://github.com/emorilebo/rust_secure_dependency_audit/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/emorilebo/rust_secure_dependency_audit/releases/tag/v0.1.1
