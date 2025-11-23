//! Footprint estimation for dependencies

use crate::config::FootprintThresholds;
use cargo_metadata::{DependencyKind, Metadata, Package, PackageId};
use std::collections::{BTreeMap, HashSet};

/// Estimate footprint risk for a dependency
pub fn estimate_footprint(
    package_id: &PackageId,
    metadata: &Metadata,
    thresholds: &FootprintThresholds,
) -> (f32, Vec<String>) {
    let mut warnings = Vec::new();
    
    // Count transitive dependencies
    let transitive_count = count_transitive_deps(package_id, metadata);
    
    // Get package details
    let package = metadata.packages.iter()
        .find(|p| &p.id == package_id);
    
    // Calculate footprint score (0.0 = low footprint, 1.0 = high footprint)
    let mut footprint_score = 0.0;
    
    // Factor 1: Transitive dependency count (40% weight)
    let dep_score = calculate_dep_count_score(transitive_count);
    footprint_score += dep_score * 0.4;
    
    // Factor 2: Feature count (30% weight)
    if let Some(pkg) = package {
        let feature_score = calculate_feature_score(&pkg.features);
        footprint_score += feature_score * 0.3;
        
        // Factor 3: Build dependencies (30% weight)
        let build_dep_score = calculate_build_dep_score(pkg);
        footprint_score += build_dep_score * 0.3;
    }
    
    // Generate warnings
    if let Some(max_transitive) = thresholds.max_transitive_deps {
        if transitive_count > max_transitive {
            warnings.push(format!(
                "High number of transitive dependencies: {} (threshold: {})",
                transitive_count, max_transitive
            ));
        }
    }
    
    if let Some(max_footprint) = thresholds.max_footprint_risk {
        if footprint_score > max_footprint {
            warnings.push(format!(
                "High footprint risk: {:.2} (threshold: {:.2})",
                footprint_score, max_footprint
            ));
        }
    }
    
    (footprint_score, warnings)
}

/// Count transitive dependencies for a package
fn count_transitive_deps(package_id: &PackageId, metadata: &Metadata) -> u32 {
    let Some(resolve) = &metadata.resolve else {
        return 0;
    };
    
    let mut visited = HashSet::new();
    let mut to_visit = vec![package_id.clone()];
    
    while let Some(current_id) = to_visit.pop() {
        if !visited.insert(current_id.clone()) {
            continue;
        }
        
        if let Some(node) = resolve.nodes.iter().find(|n| n.id == current_id) {
            for dep in &node.deps {
                to_visit.push(dep.pkg.clone());
            }
        }
    }
    
    // Subtract 1 to not count the package itself
    visited.len().saturating_sub(1) as u32
}

/// Calculate score based on dependency count
fn calculate_dep_count_score(count: u32) -> f32 {
    match count {
        0..=5 => 0.1,
        6..=10 => 0.2,
        11..=20 => 0.4,
        21..=50 => 0.6,
        51..=100 => 0.8,
        _ => 1.0,
    }
}

/// Calculate score based on feature count
fn calculate_feature_score(features: &BTreeMap<String, Vec<String>>) -> f32 {
    let feature_count = features.len();
    
    match feature_count {
        0..=3 => 0.1,
        4..=8 => 0.3,
        9..=15 => 0.5,
        16..=30 => 0.7,
        _ => 1.0,
    }
}

/// Calculate score based on build dependencies
fn calculate_build_dep_score(package: &Package) -> f32 {
    let build_deps_count = package.dependencies.iter()
        .filter(|dep| matches!(dep.kind, DependencyKind::Build))
        .count();
    
    match build_deps_count {
        0 => 0.0,
        1..=2 => 0.3,
        3..=5 => 0.6,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dep_count_score() {
        assert!(calculate_dep_count_score(3) < 0.2);
        assert!(calculate_dep_count_score(15) > 0.3);
        assert!(calculate_dep_count_score(150) > 0.9);
    }

    #[test]
    fn test_feature_score() {
        let mut features = BTreeMap::new();
        
        assert!(calculate_feature_score(&features) < 0.2);
        
        for i in 0..10 {
            features.insert(format!("feature{}", i), vec![]);
        }
        
        assert!(calculate_feature_score(&features) > 0.4);
    }
}
