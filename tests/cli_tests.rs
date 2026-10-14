//! Integration tests for the CLI

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

/// A project whose only dependency is a local path crate: auditing it needs
/// no network, so these tests run offline.
fn offline_project() -> tempfile::TempDir {
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
        "[package]\nname = \"offline_app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
         [dependencies]\nhelper = { path = \"helper\" }\n\n[workspace]\n",
    )
    .unwrap();
    tmp
}

fn write_config(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("audit.toml");
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn check_passes_offline_and_reports_skipped_path_dependency() {
    let project = offline_project();
    Command::cargo_bin("secure-audit")
        .unwrap()
        .args(["--quiet", "check", "--project-path"])
        .arg(project.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("All checks passed"))
        .stderr(predicate::str::contains(
            "Skipped 1 local path dependency (",
        ));
}

#[test]
fn json_report_on_stdout_stays_valid_json() {
    let project = offline_project();
    let output = Command::cargo_bin("secure-audit")
        .unwrap()
        .args(["-q", "report", "--format", "json", "-p"])
        .arg(project.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["project_name"], "offline_app");
    assert_eq!(report["summary"]["total_dependencies"], 0);
    assert_eq!(report["notices"].as_array().unwrap().len(), 1);
}

#[test]
fn report_format_accepts_md_alias_and_any_case() {
    let project = offline_project();
    for format in ["md", "Markdown", "JSON"] {
        Command::cargo_bin("secure-audit")
            .unwrap()
            .args(["-q", "report", "--format", format, "-p"])
            .arg(project.path())
            .assert()
            .success();
    }
    Command::cargo_bin("secure-audit")
        .unwrap()
        .args(["report", "--format", "xml"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("possible values: json, markdown"));
}

#[test]
fn partial_config_file_is_accepted() {
    // Regression: every section and key used to be mandatory, so the
    // README example failed with "missing field `ignored_dependencies`".
    let project = offline_project();
    let config = write_config(project.path(), "[network]\nrequest_delay_ms = 1500\n");
    Command::cargo_bin("secure-audit")
        .unwrap()
        .args(["-q", "check", "-p"])
        .arg(project.path())
        .arg("--config")
        .arg(config)
        .assert()
        .success();
}

#[test]
fn config_with_bad_weights_is_rejected() {
    let project = offline_project();
    let config = write_config(
        project.path(),
        "[scoring_weights]\nrecency = 0.9\nmaintenance = 0.9\n",
    );
    Command::cargo_bin("secure-audit")
        .unwrap()
        .args(["-q", "scan", "-p"])
        .arg(project.path())
        .arg("--config")
        .arg(config)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Scoring weights must sum to 1.0"));
}

#[test]
fn threshold_above_100_is_a_usage_error() {
    Command::cargo_bin("secure-audit")
        .unwrap()
        .args(["check", "--min-health-score", "150"])
        .assert()
        .code(2);
    Command::cargo_bin("secure-audit")
        .unwrap()
        .args(["scan", "--fail-threshold", "101"])
        .assert()
        .code(2);
}

#[test]
fn missing_project_is_an_error() {
    Command::cargo_bin("secure-audit")
        .unwrap()
        .args(["-q", "scan", "-p", "/definitely/not/a/project"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Cargo.toml not found"));
}

#[test]
fn test_cli_scan_help() {
    let mut cmd = Command::cargo_bin("secure-audit").unwrap();
    cmd.arg("scan").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Run full audit"));
}

#[test]
fn test_cli_report_help() {
    let mut cmd = Command::cargo_bin("secure-audit").unwrap();
    cmd.arg("report").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Generate detailed audit report"));
}

#[test]
fn test_cli_check_help() {
    let mut cmd = Command::cargo_bin("secure-audit").unwrap();
    cmd.arg("check").arg("--help");

    cmd.assert().success().stdout(predicate::str::contains(
        "Check dependencies against thresholds",
    ));
}

#[test]
#[ignore] // Requires network access
fn test_cli_scan_sample_project() {
    let sample_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_project");

    let mut cmd = Command::cargo_bin("secure-audit").unwrap();
    cmd.arg("scan").arg("--project-path").arg(sample_path);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Audit Summary"));
}

#[test]
#[ignore] // Requires network access
fn test_cli_report_json() {
    let sample_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_project");

    let mut cmd = Command::cargo_bin("secure-audit").unwrap();
    cmd.arg("report")
        .arg("--project-path")
        .arg(sample_path)
        .arg("--format")
        .arg("json");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("dependencies"));
}

#[test]
#[ignore] // Requires network access
fn test_cli_check_threshold() {
    let sample_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_project");

    let mut cmd = Command::cargo_bin("secure-audit").unwrap();
    cmd.arg("check")
        .arg("--project-path")
        .arg(sample_path)
        .arg("--min-health-score")
        .arg("0");

    // Every dependency can be scored when the network is available, so a
    // zero threshold passes. (Real scores depend on live data; asserting a
    // higher floor here would make the test flaky.)
    cmd.assert().success();
}

#[test]
#[ignore] // Requires network access
fn test_cli_scan_with_fail_threshold_low() {
    let sample_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_project");

    let mut cmd = Command::cargo_bin("secure-audit").unwrap();
    cmd.arg("scan")
        .arg("--project-path")
        .arg(sample_path)
        .arg("--fail-threshold")
        .arg("100"); // Unreasonably high threshold

    // Should fail with 100 threshold
    cmd.assert().failure();
}
