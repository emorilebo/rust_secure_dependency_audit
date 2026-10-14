//! Parser for Cargo.toml and Cargo.lock to extract dependency information

use crate::error::{AuditError, Result};
use crate::types::DependencySource;
use cargo_metadata::{CargoOpt, Metadata, MetadataCommand, Package, PackageId};
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Source ids cargo uses for crates.io (git index and sparse index).
const CRATES_IO_SOURCES: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

/// Information about a parsed dependency
#[derive(Debug, Clone)]
pub struct ParsedDependency {
    pub name: String,
    pub version: String,
    pub is_direct: bool,
    pub source: DependencySource,
    pub package_id: PackageId,
    /// License expression from the package's own manifest
    pub license: Option<String>,
    /// Repository URL from the package's own manifest
    pub repository: Option<String>,
}

/// A parsed project: its name, its dependencies, and the raw cargo metadata
/// (reused for footprint analysis so `cargo metadata` runs only once).
#[derive(Debug)]
pub struct ParsedProject {
    pub name: String,
    pub dependencies: Vec<ParsedDependency>,
    pub metadata: Metadata,
}

/// Parse a Rust project and extract all dependencies
pub fn parse_project(project_path: &Path) -> Result<ParsedProject> {
    let metadata = get_cargo_metadata(project_path)?;
    let name = project_name(&metadata);
    let dependencies = extract_dependencies(&metadata)?;
    Ok(ParsedProject {
        name,
        dependencies,
        metadata,
    })
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
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| AuditError::parse("No dependency resolution found"))?;

    // Root package, or every workspace member for a virtual workspace.
    let root_ids: HashSet<&PackageId> = match &resolve.root {
        Some(root) => std::iter::once(root).collect(),
        None => metadata.workspace_members.iter().collect(),
    };

    // Direct dependencies are the resolved edges out of the root package(s).
    // Using the resolve graph (not names) picks the right version when
    // several versions of one crate are in the tree.
    let direct_ids: HashSet<&PackageId> = resolve
        .nodes
        .iter()
        .filter(|node| root_ids.contains(&node.id))
        .flat_map(|node| node.deps.iter().map(|dep| &dep.pkg))
        .collect();

    let packages: HashMap<&PackageId, &Package> =
        metadata.packages.iter().map(|pkg| (&pkg.id, pkg)).collect();

    let dependencies = resolve
        .nodes
        .iter()
        .filter(|node| !root_ids.contains(&node.id))
        .filter_map(|node| packages.get(&node.id))
        .map(|pkg| ParsedDependency {
            name: pkg.name.clone(),
            version: pkg.version.to_string(),
            is_direct: direct_ids.contains(&pkg.id),
            source: classify_source(
                pkg.source.as_ref().map(|source| source.repr.as_str()),
                || {
                    pkg.manifest_path
                        .parent()
                        .map(|dir| dir.as_std_path().display().to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                },
            ),
            package_id: pkg.id.clone(),
            license: pkg.license.clone(),
            repository: pkg.repository.clone(),
        })
        .collect();

    Ok(dependencies)
}

/// Determine where a package comes from, given cargo's source id.
fn classify_source(source: Option<&str>, local_dir: impl FnOnce() -> String) -> DependencySource {
    let Some(source) = source else {
        // No source means a path dependency or workspace member.
        return DependencySource::Path { path: local_dir() };
    };

    if CRATES_IO_SOURCES.contains(&source) {
        DependencySource::CratesIo
    } else if let Some(git) = source.strip_prefix("git+") {
        let url = git.split(['?', '#']).next().unwrap_or(git).to_string();
        DependencySource::Git { url }
    } else if let Some(path) = source.strip_prefix("path+") {
        let path = path.strip_prefix("file://").unwrap_or(path).to_string();
        DependencySource::Path { path }
    } else {
        // Alternative registries: never look these up on crates.io, where a
        // crate with the same name may be a different crate entirely.
        DependencySource::Unknown
    }
}

/// Name of the root package, or of the workspace directory for a virtual
/// workspace.
fn project_name(metadata: &Metadata) -> String {
    metadata
        .root_package()
        .map(|pkg| pkg.name.clone())
        .or_else(|| {
            metadata
                .workspace_root
                .file_name()
                .map(|name| name.to_string())
        })
        .unwrap_or_else(|| "workspace".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_dir() -> String {
        unreachable!("only used for sourceless packages")
    }

    #[test]
    fn crates_io_sources_are_recognised() {
        for source in CRATES_IO_SOURCES {
            assert!(matches!(
                classify_source(Some(source), no_dir),
                DependencySource::CratesIo
            ));
        }
    }

    #[test]
    fn alternative_registries_are_not_crates_io() {
        for source in [
            "registry+https://my-company.example/index",
            "sparse+https://cargo.cloudsmith.io/acme/main/",
        ] {
            assert!(matches!(
                classify_source(Some(source), no_dir),
                DependencySource::Unknown
            ));
        }
    }

    #[test]
    fn git_source_drops_query_and_fragment() {
        let source = classify_source(
            Some("git+https://github.com/o/r?branch=main#0123abcd"),
            no_dir,
        );
        assert!(matches!(source, DependencySource::Git { url } if url == "https://github.com/o/r"));

        let source = classify_source(Some("git+https://github.com/o/r#0123abcd"), no_dir);
        assert!(matches!(source, DependencySource::Git { url } if url == "https://github.com/o/r"));
    }

    fn write_crate(root: &Path, dir: &str, name: &str, version: &str, deps: &str) {
        let crate_dir = root.join(dir);
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(crate_dir.join("src/lib.rs"), "").unwrap();
        std::fs::write(
            crate_dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n\
                 license = \"MIT\"\n\n[dependencies]\n{deps}"
            ),
        )
        .unwrap();
    }

    #[test]
    fn direct_dependencies_follow_resolved_versions() {
        // root -> helper 2.0.0 (direct)
        // root -> other -> helper 1.0.0 (transitive, same name)
        // The old name-based lookup took the first package named `helper`
        // in cargo's sorted package list (1.0.0) and marked it direct.
        // The path crates live next to (not inside) the app directory, so
        // cargo does not treat them as workspace members, which may not
        // share a name.
        let tmp = tempfile::tempdir().unwrap();
        write_crate(tmp.path(), "helper_v1", "helper", "1.0.0", "");
        write_crate(tmp.path(), "helper_v2", "helper", "2.0.0", "");
        write_crate(
            tmp.path(),
            "other",
            "other",
            "0.1.0",
            "helper = { path = \"../helper_v1\" }\n",
        );
        let root = tmp.path().join("app");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"root_app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nhelper = { path = \"../helper_v2\" }\nother = { path = \"../other\" }\n\n\
             [workspace]\n",
        )
        .unwrap();

        let project = parse_project(&root).unwrap();
        assert_eq!(project.name, "root_app");

        let direct: Vec<(String, String)> = project
            .dependencies
            .iter()
            .filter(|dep| dep.is_direct)
            .map(|dep| (dep.name.clone(), dep.version.clone()))
            .collect();
        assert!(direct.contains(&("helper".to_string(), "2.0.0".to_string())));
        assert!(direct.contains(&("other".to_string(), "0.1.0".to_string())));
        assert!(!direct.contains(&("helper".to_string(), "1.0.0".to_string())));

        let helper_v1 = project
            .dependencies
            .iter()
            .find(|dep| dep.name == "helper" && dep.version == "1.0.0")
            .unwrap();
        assert!(matches!(helper_v1.source, DependencySource::Path { .. }));
        assert_eq!(helper_v1.license.as_deref(), Some("MIT"));
    }

    #[test]
    fn sourceless_package_is_a_path_dependency() {
        let source = classify_source(None, || "/work/helper".to_string());
        assert!(matches!(source, DependencySource::Path { path } if path == "/work/helper"));
    }
}
