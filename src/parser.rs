//! Parser for Cargo.toml and Cargo.lock to extract dependency information

use crate::error::{AuditError, Result};
use crate::types::DependencySource;
use cargo_metadata::{CargoOpt, Metadata, MetadataCommand, Package, PackageId};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Information about a parsed dependency
#[derive(Debug, Clone)]
pub struct ParsedDependency {
    pub name: String,
    pub version: String,
    pub is_direct: bool,
    pub source: DependencySource,
    pub package_id: PackageId,
}

/// Parse a Rust project and extract all dependencies
pub fn parse_project(project_path: &Path) -> Result<Vec<ParsedDependency>> {
    let metadata = get_cargo_metadata(project_path)?;
    extract_dependencies(&metadata)
}

/// Get cargo metadata for a project
fn get_cargo_metadata(project_path: &Path) -> Result<Metadata> {
    let manifest_path = project_path.join("Cargo.toml");
    
    if !manifest_path.exists() {
        return Err(AuditError::parse(format!(
            "Cargo.toml not found at {}",
            manifest_path.display()
        )));
    }

    let metadata = MetadataCommand::new()
        .manifest_path(&manifest_path)
        .features(CargoOpt::AllFeatures)
        .exec()?;

    Ok(metadata)
}

/// Extract all dependencies from cargo metadata
fn extract_dependencies(metadata: &Metadata) -> Result<Vec<ParsedDependency>> {
    let mut dependencies = Vec::new();
    
    // Get the root package(s) - handle workspace projects
    let root_packages: Vec<&Package> = if let Some(resolve) = &metadata.resolve {
        resolve
            .root
            .as_ref()
            .and_then(|root_id| metadata.packages.iter().find(|p| &p.id == root_id))
            .map(|p| vec![p])
            .unwrap_or_else(|| {
                // Workspace: get all workspace members
                metadata
                    .workspace_members
                    .iter()
                    .filter_map(|id| metadata.packages.iter().find(|p| &p.id == id))
                    .collect()
            })
    } else {
        return Err(AuditError::parse("No dependency resolution found"));
    };

    // Get direct dependencies from root packages
    let mut direct_deps: HashSet<PackageId> = HashSet::new();
    for root_pkg in &root_packages {
        for dep in &root_pkg.dependencies {
            // Find the package that matches this dependency
            if let Some(pkg) = metadata.packages.iter().find(|p| p.name == dep.name) {
                direct_deps.insert(pkg.id.clone());
            }
        }
    }

    // Get all packages from resolve graph
    if let Some(resolve) = &metadata.resolve {
        let root_ids: HashSet<_> = root_packages.iter().map(|p| &p.id).collect();
        
        for node in &resolve.nodes {
            // Skip root packages themselves
            if root_ids.contains(&node.id) {
                continue;
            }

            if let Some(pkg) = metadata.packages.iter().find(|p| p.id == node.id) {
                let is_direct = direct_deps.contains(&pkg.id);
                let source = determine_source(pkg);

                dependencies.push(ParsedDependency {
                    name: pkg.name.clone(),
                    version: pkg.version.to_string(),
                    is_direct,
                    source,
                    package_id: pkg.id.clone(),
                });
            }
        }
    }

    Ok(dependencies)
}

/// Determine the source of a package
fn determine_source(package: &Package) -> DependencySource {
    if let Some(source) = &package.source {
        let source_str = source.repr.as_str();
        
        if source_str.starts_with("registry+") {
            DependencySource::CratesIo
        } else if source_str.starts_with("git+") {
            // Extract git URL
            let url = source_str
                .strip_prefix("git+")
                .and_then(|s| s.split('?').next())
                .unwrap_or(source_str)
                .to_string();
            DependencySource::Git { url }
        } else if source_str.starts_with("path+") {
            let path = source_str
                .strip_prefix("path+file://")
                .or_else(|| source_str.strip_prefix("path+"))
                .unwrap_or(source_str)
                .to_string();
            DependencySource::Path { path }
        } else {
            DependencySource::Unknown
        }
    } else {
        // No source usually means it's a path dependency or workspace member
        DependencySource::Path {
            path: package.manifest_path.parent()
                .map(|p| p.as_std_path().display().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
        }
    }
}

/// Get the name of the project from its Cargo.toml
pub fn get_project_name(project_path: &Path) -> Result<String> {
    let metadata = get_cargo_metadata(project_path)?;
    
    if let Some(resolve) = &metadata.resolve {
        if let Some(root_id) = &resolve.root {
            if let Some(root_pkg) = metadata.packages.iter().find(|p| &p.id == root_id) {
                return Ok(root_pkg.name.clone());
            }
        }
    }
    
    // Fallback: use workspace name or first package
    if let Some(first_pkg) = metadata.packages.first() {
        Ok(first_pkg.name.clone())
    } else {
        Err(AuditError::parse("Could not determine project name"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_determine_source() {
        // Test would require creating mock Package instances
        // For now, this is a placeholder
    }
}
