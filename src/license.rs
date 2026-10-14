//! License analysis and risk categorization
//!
//! License strings are parsed as SPDX expressions (in the lax mode crates.io
//! accepts, so `MIT/Apache-2.0` works). `OR` means the licensee may pick the
//! least restrictive option; `AND` means every term applies, so the most
//! restrictive term wins. Strings that are not valid SPDX fall back to a
//! keyword heuristic.

use crate::config::LicensePolicy;
use crate::types::LicenseRisk;
use spdx::{Expression, LicenseItem, LicenseReq, ParseMode};

/// License families treated as copyleft in addition to those the SPDX list
/// flags as copyleft (weak copyleft licenses such as EPL and CDDL).
const EXTRA_COPYLEFT_PREFIXES: [&str; 7] = [
    "epl-",
    "cddl-",
    "eupl-",
    "osl-",
    "cc-by-sa-",
    "ms-rl",
    "cpl-",
];

/// Permissive licenses that are neither OSI-approved nor FSF-libre in the
/// SPDX list but are commonly used for permissive data/code in Rust crates.
const EXTRA_PERMISSIVE: [&str; 2] = ["CDLA-Permissive-1.0", "CDLA-Permissive-2.0"];

const PROPRIETARY_KEYWORDS: [&str; 4] = [
    "proprietary",
    "commercial",
    "private",
    "all rights reserved",
];

/// Analyze license and determine risk level
pub fn analyze_license(
    license: Option<&str>,
    policy: &LicensePolicy,
) -> (LicenseRisk, Vec<String>) {
    let mut warnings = Vec::new();

    let Some(license_str) = license.map(str::trim).filter(|l| !l.is_empty()) else {
        if policy.warn_on_unknown {
            warnings.push("No license information found".to_string());
        }
        return (LicenseRisk::Unknown, warnings);
    };

    let expression = Expression::parse_mode(license_str, ParseMode::LAX).ok();

    // Forbidden: the expression cannot be satisfied without a forbidden license.
    if !policy.forbidden_licenses.is_empty() {
        let forbidden = |name: &str| {
            policy
                .forbidden_licenses
                .iter()
                .any(|pattern| matches_pattern(name, pattern))
        };
        let violates = match &expression {
            Some(expr) => !expr.evaluate(|req| !forbidden(&req_name(req))),
            None => forbidden(license_str),
        };
        if violates {
            warnings.push(format!("Uses forbidden license: {}", license_str));
            return (LicenseRisk::Proprietary, warnings);
        }
    }

    // Allowed: the expression can be satisfied using only allowed licenses.
    if !policy.allowed_licenses.is_empty() {
        let allowed = |name: &str| {
            policy
                .allowed_licenses
                .iter()
                .any(|pattern| matches_pattern(name, pattern))
        };
        let satisfied = match &expression {
            Some(expr) => expr.evaluate(|req| allowed(&req_name(req))),
            None => allowed(license_str),
        };
        if !satisfied {
            warnings.push(format!("License {} not in allowed list", license_str));
        }
    }

    let risk = match &expression {
        Some(expr) => categorize_expression(expr),
        None => categorize_by_keywords(license_str),
    };

    match risk {
        LicenseRisk::Copyleft if policy.warn_on_copyleft => {
            warnings.push(format!("Copyleft license detected: {}", license_str));
        }
        LicenseRisk::Unknown if policy.warn_on_unknown => {
            warnings.push(format!("Unknown license: {}", license_str));
        }
        LicenseRisk::Proprietary => {
            warnings.push(format!("Proprietary license detected: {}", license_str));
        }
        _ => {}
    }

    (risk, warnings)
}

/// The least restrictive risk level the licensee can achieve.
fn categorize_expression(expr: &Expression) -> LicenseRisk {
    if expr.evaluate(|req| categorize_req(req) == LicenseRisk::Permissive) {
        return LicenseRisk::Permissive;
    }
    if expr.evaluate(|req| {
        matches!(
            categorize_req(req),
            LicenseRisk::Permissive | LicenseRisk::Copyleft
        )
    }) {
        return LicenseRisk::Copyleft;
    }
    if expr
        .requirements()
        .any(|er| categorize_req(&er.req) == LicenseRisk::Proprietary)
    {
        return LicenseRisk::Proprietary;
    }
    LicenseRisk::Unknown
}

fn categorize_req(req: &LicenseReq) -> LicenseRisk {
    match &req.license {
        LicenseItem::Spdx { id, .. } => {
            let name = id.name.to_lowercase();
            if id.is_copyleft() || EXTRA_COPYLEFT_PREFIXES.iter().any(|p| name.starts_with(p)) {
                LicenseRisk::Copyleft
            } else if id.is_osi_approved()
                || id.is_fsf_free_libre()
                || EXTRA_PERMISSIVE.contains(&id.name)
            {
                LicenseRisk::Permissive
            } else {
                LicenseRisk::Unknown
            }
        }
        LicenseItem::Other { lic_ref, .. } => categorize_by_keywords(lic_ref),
    }
}

/// Fallback for strings that are not valid SPDX expressions.
fn categorize_by_keywords(license: &str) -> LicenseRisk {
    let lower = license.to_lowercase();
    if PROPRIETARY_KEYWORDS.iter().any(|k| lower.contains(k)) {
        LicenseRisk::Proprietary
    } else {
        LicenseRisk::Unknown
    }
}

fn req_name(req: &LicenseReq) -> String {
    match &req.license {
        LicenseItem::Spdx { id, .. } => id.name.to_string(),
        LicenseItem::Other { lic_ref, .. } => format!("LicenseRef-{}", lic_ref),
    }
}

/// Policy entries match a license id case-insensitively as a prefix, so
/// `GPL-3.0` matches `GPL-3.0-only` but not `LGPL-3.0-only`.
fn matches_pattern(license_name: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    !pattern.is_empty()
        && license_name
            .to_lowercase()
            .starts_with(&pattern.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn risk(license: &str) -> LicenseRisk {
        analyze_license(Some(license), &LicensePolicy::default()).0
    }

    #[test]
    fn test_categorize_mit() {
        assert_eq!(risk("MIT"), LicenseRisk::Permissive);
        assert_eq!(risk("MIT OR Apache-2.0"), LicenseRisk::Permissive);
        assert_eq!(risk("MIT/Apache-2.0"), LicenseRisk::Permissive);
        assert_eq!(risk("Apache-2.0 / MIT"), LicenseRisk::Permissive);
    }

    #[test]
    fn test_categorize_apache() {
        assert_eq!(risk("Apache-2.0"), LicenseRisk::Permissive);
        assert_eq!(
            risk("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT"),
            LicenseRisk::Permissive
        );
    }

    #[test]
    fn test_categorize_gpl() {
        assert_eq!(risk("GPL-3.0"), LicenseRisk::Copyleft);
        assert_eq!(risk("LGPL-2.1"), LicenseRisk::Copyleft);
        assert_eq!(risk("AGPL-3.0"), LicenseRisk::Copyleft);
        assert_eq!(risk("MPL-2.0"), LicenseRisk::Copyleft);
        assert_eq!(risk("EPL-2.0"), LicenseRisk::Copyleft);
    }

    #[test]
    fn licenses_common_in_rust_lockfiles_are_permissive() {
        // These were reported as "Unknown" by the old substring matcher;
        // the ICU crates pulled in by `url`/`reqwest` are all Unicode-3.0.
        for license in [
            "Unicode-3.0",
            "BSL-1.0",
            "CDLA-Permissive-2.0",
            "CC0-1.0",
            "Zlib",
            "0BSD",
            "(MIT OR Apache-2.0) AND Unicode-3.0",
            "(Apache-2.0 OR MIT) AND BSD-3-Clause",
            "Apache-2.0 AND ISC",
        ] {
            assert_eq!(risk(license), LicenseRisk::Permissive, "{license}");
        }
    }

    #[test]
    fn and_with_copyleft_is_copyleft() {
        // Old matcher: contains "mit" -> Permissive.
        assert_eq!(risk("MIT AND GPL-3.0-only"), LicenseRisk::Copyleft);
        // OR lets you pick the permissive option.
        assert_eq!(risk("MIT OR GPL-3.0-only"), LicenseRisk::Permissive);
        assert_eq!(
            risk("MIT OR Apache-2.0 OR LGPL-2.1-or-later"),
            LicenseRisk::Permissive
        );
    }

    #[test]
    fn test_categorize_unknown() {
        assert_eq!(risk("CustomLicense"), LicenseRisk::Unknown);
        assert_eq!(risk("BUSL-1.1"), LicenseRisk::Unknown);
        assert_eq!(
            analyze_license(None, &LicensePolicy::default()).0,
            LicenseRisk::Unknown
        );
    }

    #[test]
    fn proprietary_keywords_are_detected() {
        assert_eq!(risk("LicenseRef-Proprietary"), LicenseRisk::Proprietary);
        assert_eq!(risk("All rights reserved"), LicenseRisk::Proprietary);
    }

    #[test]
    fn test_license_policy_patterns() {
        assert!(matches_pattern("MIT", "MIT"));
        assert!(matches_pattern("Apache-2.0", "Apache"));
        assert!(matches_pattern("GPL-3.0-only", "GPL-3.0"));
        assert!(!matches_pattern("LGPL-3.0-only", "GPL-3.0"));
        assert!(!matches_pattern("GPL-3.0-only", "MIT"));
    }

    #[test]
    fn forbidden_license_respects_or_choice() {
        let policy = LicensePolicy {
            forbidden_licenses: HashSet::from(["GPL-3.0".to_string()]),
            ..LicensePolicy::default()
        };

        let (risk, warnings) = analyze_license(Some("GPL-3.0-only"), &policy);
        assert_eq!(risk, LicenseRisk::Proprietary);
        assert!(warnings.iter().any(|w| w.contains("forbidden")));

        // You can take the MIT option, so nothing forbidden is required.
        let (_, warnings) = analyze_license(Some("MIT OR GPL-3.0-only"), &policy);
        assert!(!warnings.iter().any(|w| w.contains("forbidden")));

        // LGPL is a different license, not a substring hit on GPL.
        let (_, warnings) = analyze_license(Some("LGPL-3.0-only"), &policy);
        assert!(!warnings.iter().any(|w| w.contains("forbidden")));
    }

    #[test]
    fn allowed_list_is_checked_against_choices() {
        let policy = LicensePolicy {
            allowed_licenses: HashSet::from(["MIT".to_string(), "Apache-2.0".to_string()]),
            ..LicensePolicy::default()
        };

        let (_, warnings) = analyze_license(Some("MIT OR Apache-2.0"), &policy);
        assert!(warnings.is_empty(), "{warnings:?}");

        let (_, warnings) = analyze_license(Some("(MIT OR Apache-2.0) AND Unicode-3.0"), &policy);
        assert!(warnings.iter().any(|w| w.contains("not in allowed list")));
    }

    #[test]
    fn test_analyze_with_policy() {
        let policy = LicensePolicy {
            warn_on_copyleft: true,
            ..LicensePolicy::default()
        };

        let (risk, warnings) = analyze_license(Some("GPL-3.0"), &policy);
        assert_eq!(risk, LicenseRisk::Copyleft);
        assert!(!warnings.is_empty());
    }
}
