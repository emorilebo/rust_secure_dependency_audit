//! License analysis and risk categorization

use crate::config::LicensePolicy;
use crate::types::LicenseRisk;

/// Analyze license and determine risk level
pub fn analyze_license(
    license: Option<&str>,
    policy: &LicensePolicy,
) -> (LicenseRisk, Vec<String>) {
    let mut warnings = Vec::new();
    
    let Some(license_str) = license else {
        if policy.warn_on_unknown {
            warnings.push("No license information found".to_string());
        }
        return (LicenseRisk::Unknown, warnings);
    };
    
    // Check forbidden licenses
    if !policy.forbidden_licenses.is_empty() {
        for forbidden in &policy.forbidden_licenses {
            if license_matches(license_str, forbidden) {
                warnings.push(format!("Uses forbidden license: {}", license_str));
                return (LicenseRisk::Proprietary, warnings);
            }
        }
    }
    
    // Check allowed licenses (if allowlist is configured)
    if !policy.allowed_licenses.is_empty() {
        let mut is_allowed = false;
        for allowed in &policy.allowed_licenses {
            if license_matches(license_str, allowed) {
                is_allowed = true;
                break;
            }
        }
        if !is_allowed {
            warnings.push(format!("License {} not in allowed list", license_str));
        }
    }
    
    // Categorize license
    let risk = categorize_license(license_str);
    
    // Generate warnings based on policy
    match risk {
        LicenseRisk::Copyleft => {
            if policy.warn_on_copyleft {
                warnings.push(format!("Copyleft license detected: {}", license_str));
            }
        }
        LicenseRisk::Unknown => {
            if policy.warn_on_unknown {
                warnings.push(format!("Unknown license: {}", license_str));
            }
        }
        LicenseRisk::Proprietary => {
            warnings.push(format!("Proprietary license detected: {}", license_str));
        }
        _ => {}
    }
    
    (risk, warnings)
}

/// Categorize a license into risk levels
fn categorize_license(license: &str) -> LicenseRisk {
    let license_lower = license.to_lowercase();
    
    // Check for permissive licenses
    if is_permissive(&license_lower) {
        return LicenseRisk::Permissive;
    }
    
    // Check for copyleft licenses
    if is_copyleft(&license_lower) {
        return LicenseRisk::Copyleft;
    }
    
    // Check for proprietary/restrictive
    if is_proprietary(&license_lower) {
        return LicenseRisk::Proprietary;
    }
    
    LicenseRisk::Unknown
}

/// Check if license is permissive
fn is_permissive(license: &str) -> bool {
    let permissive = [
        "mit",
        "apache",
        "bsd",
        "isc",
        "0bsd",
        "unlicense",
        "cc0",
        "wtfpl",
        "zlib",
        "boost",
    ];
    
    permissive.iter().any(|&p| license.contains(p))
}

/// Check if license is copyleft
fn is_copyleft(license: &str) -> bool {
    let copyleft = [
        "gpl",
        "lgpl",
        "agpl",
        "mpl",
        "eupl",
        "osl",
        "ms-pl",
        "cddl",
        "epl",
        "cc-by-sa",
    ];
    
    copyleft.iter().any(|&c| license.contains(c))
}

/// Check if license is proprietary/restrictive
fn is_proprietary(license: &str) -> bool {
    let proprietary = [
        "proprietary",
        "commercial",
        "private",
        "all rights reserved",
    ];
    
    proprietary.iter().any(|&p| license.contains(p))
}

/// Check if two licenses match (case-insensitive, handles OR/AND)
fn license_matches(license: &str, pattern: &str) -> bool {
    let license_lower = license.to_lowercase();
    let pattern_lower = pattern.to_lowercase();
    
    // Handle SPDX expressions with OR/AND
    if license_lower.contains(" or ") || license_lower.contains(" and ") {
        // For OR expressions, check if pattern matches any part
        if license_lower.contains(" or ") {
            return license_lower.split(" or ")
                .any(|part| part.trim().contains(&pattern_lower));
        }
        // For AND expressions, check if pattern matches any part
        if license_lower.contains(" and ") {
            return license_lower.split(" and ")
                .any(|part| part.trim().contains(&pattern_lower));
        }
    }
    
    license_lower.contains(&pattern_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorize_mit() {
        assert_eq!(categorize_license("MIT"), LicenseRisk::Permissive);
        assert_eq!(categorize_license("MIT OR Apache-2.0"), LicenseRisk::Permissive);
    }

    #[test]
    fn test_categorize_apache() {
        assert_eq!(categorize_license("Apache-2.0"), LicenseRisk::Permissive);
    }

    #[test]
    fn test_categorize_gpl() {
        assert_eq!(categorize_license("GPL-3.0"), LicenseRisk::Copyleft);
        assert_eq!(categorize_license("LGPL-2.1"), LicenseRisk::Copyleft);
        assert_eq!(categorize_license("AGPL-3.0"), LicenseRisk::Copyleft);
    }

    #[test]
    fn test_categorize_unknown() {
        assert_eq!(categorize_license("CustomLicense"), LicenseRisk::Unknown);
    }

    #[test]
    fn test_license_matches() {
        assert!(license_matches("MIT", "MIT"));
        assert!(license_matches("MIT OR Apache-2.0", "MIT"));
        assert!(license_matches("MIT OR Apache-2.0", "Apache"));
        assert!(!license_matches("GPL-3.0", "MIT"));
    }

    #[test]
    fn test_analyze_with_policy() {
        let mut policy = LicensePolicy::default();
        policy.warn_on_copyleft = true;
        
        let (risk, warnings) = analyze_license(Some("GPL-3.0"), &policy);
        assert_eq!(risk, LicenseRisk::Copyleft);
        assert!(!warnings.is_empty());
    }
}
