# Publishing Guide

This guide explains how to publish `rust_secure_dependency_audit` to crates.io.

## Prerequisites

1. **Crates.io Account**
   - Create an account at https://crates.io
   - Generate an API token: https://crates.io/settings/tokens

2. **Login to Cargo**
   ```bash
   cargo login <YOUR_API_TOKEN>
   ```

## Pre-Publish Checklist

Before publishing, ensure:

- [ ] All tests pass: `cargo test --all-features`
- [ ] Code is formatted: `cargo fmt --all -- --check`
- [ ] No clippy warnings: `cargo clippy --all-features -- -D warnings`
- [ ] Documentation builds: `cargo doc --no-deps`
- [ ] Version number is updated in `Cargo.toml`
- [ ] `CHANGELOG.md` is updated with release notes
- [ ] `README.md` is up to date
- [ ] Repository URL in `Cargo.toml` is correct
- [ ] LICENSE files exist (MIT and Apache-2.0)

## Version Numbering

Follow [Semantic Versioning](https://semver.org/):
- **Major** (x.0.0): Breaking changes
- **Minor** (0.x.0): New features, backward compatible
- **Patch** (0.0.x): Bug fixes, backward compatible

For initial releases (0.x.y):
- Minor version for breaking changes
- Patch version for new features and fixes

## Publishing Steps

### 1. Update Version

Edit `Cargo.toml`:
```toml
[package]
version = "0.1.1"  # Increment appropriately
```

### 2. Update Changelog

Edit `CHANGELOG.md`:
```markdown
## [0.1.1] - 2024-01-15

### Added
- New feature X

### Fixed
- Bug Y
```

### 3. Commit Changes

```bash
git add Cargo.toml CHANGELOG.md
git commit -m "chore: bump version to 0.1.1"
```

### 4. Dry Run

Test the package build:
```bash
cargo publish --dry-run
```

This will:
- Build the package
- Show what files will be included
- Validate metadata
- Check for errors

Review the output carefully!

### 5. Publish

If dry-run succeeds:
```bash
cargo publish
```

**Note**: Publishing is permanent and cannot be undone! Make sure everything is correct.

### 6. Tag Release

Create a git tag:
```bash
git tag -a v0.1.1 -m "Release version 0.1.1"
git push origin v0.1.1
git push origin main
```

### 7. Create GitHub Release

1. Go to your repository on GitHub
2. Click "Releases" → "Create a new release"
3. Select the tag you just created
4. Title: `v0.1.1`
5. Description: Copy from CHANGELOG.md
6. Attach any relevant artifacts (optional)
7. Click "Publish release"

## Post-Publish

### Verify Publication

1. Check on crates.io: https://crates.io/crates/rust_secure_dependency_audit
2. Verify documentation builds: https://docs.rs/rust_secure_dependency_audit
3. Test installation:
   ```bash
   cargo install rust_secure_dependency_audit --version 0.1.1
   ```

### Announce

Consider announcing on:
- Reddit: r/rust
- Twitter/X with #rustlang
- This Week in Rust: https://this-week-in-rust.org/
- Rust Users Forum: https://users.rust-lang.org/

## Troubleshooting

### "failed to verify package tarball"

This usually means files referenced in `Cargo.toml` (like `README.md`) are missing. Run:
```bash
cargo package --list
```
to see what files are included.

### "failed to publish to registry"

Common causes:
- Version already published (can't overwrite)
- Missing required metadata fields
- Network issues

Check `cargo publish --dry-run` output for details.

### Documentation fails to build

Test locally:
```bash
cargo doc --no-deps --open
```

Fix any warnings or errors before publishing.

### Rate limits

If you publish multiple versions quickly, crates.io may rate-limit you. Wait a few minutes between publishes.

## Unpublishing (Yanking)

You cannot delete a published version, but you can "yank" it to prevent new projects from using it:

```bash
cargo yank --vers 0.1.0
```

To un-yank:
```bash
cargo yank --vers 0.1.0 --undo
```

**Important**: Yanking doesn't affect projects already using that version.

## Best Practices

1. **Test Thoroughly**: Run all tests, including integration tests and examples
2. **Update Docs**: Keep README and rustdoc comments current
3. **Changelog**: Maintain a clear changelog for users
4. **Version Carefully**: Don't make breaking changes in patch releases
5. **CI/CD**: Set up GitHub Actions to automate checks
6. **Backwards Compatibility**: Maintain it within major versions

## Automation (Optional)

Consider using tools to streamline the process:
- [`cargo-release`](https://github.com/crate-ci/cargo-release): Automate version bumping and publishing
- GitHub Actions: Automatically publish on tagged releases

Example workflow:
```yaml
# .github/workflows/publish.yml
name: Publish to crates.io

on:
  push:
    tags:
      - 'v*'

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo publish --token ${{ secrets.CARGO_TOKEN }}
```

## Support

- crates.io docs: https://doc.rust-lang.org/cargo/reference/publishing.html
- Cargo book: https://doc.rust-lang.org/cargo/
- Help: https://users.rust-lang.org/
