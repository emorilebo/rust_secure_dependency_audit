//! Integration tests for the CLI

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

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
    
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Check dependencies against thresholds"));
}

#[test]
#[ignore] // Requires network access
fn test_cli_scan_sample_project() {
    let sample_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample_project");
    
    let mut cmd = Command::cargo_bin("secure-audit").unwrap();
    cmd.arg("scan")
        .arg("--project-path")
        .arg(sample_path);
    
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
        .arg("50");
    
    // Should pass with reasonable threshold
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
