//! Dependency resolution with semantic versioning support
//!
//! This module provides dependency resolution functionality using the PubGrub
//! algorithm for superior conflict resolution and error messages.
//!
//! # Examples
//!
//! ```no_run
//! use unrealpm::{RegistryClient, resolve_dependencies};
//! use std::collections::HashMap;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let registry = RegistryClient::new_default()?;
//! let mut dependencies = HashMap::new();
//! dependencies.insert("awesome-plugin".to_string(), "^1.0.0".to_string());
//!
//! let resolved = resolve_dependencies(&dependencies, &registry, Some("5.3"), false, None)?;
//! println!("Resolved {} packages", resolved.len());
//! # Ok(())
//! # }
//! ```

use crate::{Error, PackageMetadata, PackageVersion, RegistryClient, ResolverConfig, Result};
use semver::{Version, VersionReq};
use std::collections::{HashMap, HashSet};

// Re-export the PubGrub-based resolver
pub use crate::pubgrub_resolver::{
    resolve_dependencies as pubgrub_resolve_dependencies, ResolvedPackage,
};

/// Find the best matching version for a version constraint
///
/// Searches for the highest version that matches the constraint and is compatible
/// with the specified engine version. Returns an error if no matching version is found.
///
/// # Arguments
///
/// * `package_metadata` - Package metadata from the registry
/// * `constraint` - Semantic version constraint (e.g., "^1.0.0", "~1.5.0", "*")
/// * `engine_version` - Optional Unreal Engine version to filter by
/// * `force` - If true, skips engine version compatibility check
///
/// # Examples
///
/// ```no_run
/// use unrealpm::{find_matching_version, RegistryClient};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let registry = RegistryClient::new_default()?;
/// let metadata = registry.get_package("awesome-plugin")?;
///
/// let version = find_matching_version(&metadata, "^1.0.0", Some("5.3"), false)?;
/// println!("Matched version: {}", version.version);
/// # Ok(())
/// # }
/// ```
pub fn find_matching_version(
    package_metadata: &PackageMetadata,
    constraint: &str,
    engine_version: Option<&str>,
    force: bool,
) -> Result<PackageVersion> {
    // Parse the version requirement
    let req = VersionReq::parse(constraint).map_err(|e| {
        Error::Other(format!(
            "Invalid version constraint '{}': {}",
            constraint, e
        ))
    })?;

    // Find all matching versions
    let mut matching_versions: Vec<_> = package_metadata
        .versions
        .iter()
        .filter_map(|pkg_ver| {
            // Normalize version (5.3 -> 5.3.0 for semver compatibility)
            let normalized_version = if pkg_ver.version.matches('.').count() == 1 {
                format!("{}.0", pkg_ver.version)
            } else {
                pkg_ver.version.clone()
            };

            // Check version constraint
            if let Ok(ver) = Version::parse(&normalized_version) {
                if !req.matches(&ver) {
                    return None;
                }
            } else {
                return None;
            }

            // Check engine version compatibility if specified (unless force is enabled)
            if !force {
                if let Some(required_engine) = engine_version {
                    // Parse required engine (e.g., "5.3" -> major=5, minor=3)
                    let req_parts: Vec<&str> = required_engine.split('.').collect();
                    let req_major = req_parts.first().and_then(|s| s.parse::<i32>().ok());
                    let req_minor = req_parts.get(1).and_then(|s| s.parse::<i32>().ok());

                    let mut matches = false;

                    // Check engine-specific version
                    if !pkg_ver.is_multi_engine {
                        // Engine-specific: Must match major.minor
                        if let (Some(pkg_major), Some(pkg_minor), Some(rm), Some(rmi)) = (
                            pkg_ver.engine_major,
                            pkg_ver.engine_minor,
                            req_major,
                            req_minor,
                        ) {
                            matches = pkg_major == rm && pkg_minor == rmi;
                        }
                    } else {
                        // Multi-engine: Check if in array
                        if let Some(compatible_engines) = &pkg_ver.engine_versions {
                            matches = compatible_engines.iter().any(|e| e == required_engine);
                        } else {
                            // If no engine_versions specified, assume compatible with all
                            matches = true;
                        }
                    }

                    if !matches {
                        return None;
                    }
                }
            }

            if let Ok(ver) = Version::parse(&pkg_ver.version) {
                Some((ver, pkg_ver.clone()))
            } else {
                None
            }
        })
        .collect();

    if matching_versions.is_empty() {
        // Build a helpful error message with available versions
        let available_versions: Vec<String> = package_metadata
            .versions
            .iter()
            .map(|v| {
                if !v.is_multi_engine {
                    // Engine-specific version
                    if let (Some(major), Some(minor)) = (v.engine_major, v.engine_minor) {
                        format!("{} (UE {}.{})", v.version, major, minor)
                    } else {
                        v.version.clone()
                    }
                } else if let Some(engines) = &v.engine_versions {
                    format!("{} (engines: {})", v.version, engines.join(", "))
                } else {
                    format!("{} (all engines)", v.version)
                }
            })
            .collect();

        let error_msg = if let Some(engine) = engine_version {
            format!(
                "No version of '{}' matches constraint '{}' for Unreal Engine {}\n\n\
                Available versions:\n  {}\n\n\
                Suggestions:\n\
                  • Check if the package supports Unreal Engine {}\n\
                  • Try a different version constraint\n\
                  • Update your engine version in the .uproject file",
                package_metadata.name,
                constraint,
                engine,
                available_versions.join("\n  "),
                engine
            )
        } else {
            format!(
                "No version of '{}' matches constraint '{}'\n\n\
                Available versions:\n  {}\n\n\
                Suggestions:\n\
                  • Try a different version constraint\n\
                  • Check the package name spelling",
                package_metadata.name,
                constraint,
                available_versions.join("\n  ")
            )
        };
        return Err(Error::DependencyResolutionFailed(error_msg));
    }

    // Sort by engine specificity first, then version
    matching_versions.sort_by(|a, b| {
        // Prefer engine-specific over multi-engine
        match (a.1.is_multi_engine, b.1.is_multi_engine) {
            (false, true) => std::cmp::Ordering::Less, // a is engine-specific, prefer it
            (true, false) => std::cmp::Ordering::Greater, // b is engine-specific, prefer it
            _ => b.0.cmp(&a.0),                        // Same type, use version (highest first)
        }
    });

    // Return the best matching version (engine-specific match or highest version)
    Ok(matching_versions[0].1.clone())
}

/// Resolve all transitive dependencies for a set of direct dependencies
///
/// Returns a map of package name to resolved version.
/// Uses PubGrub algorithm for optimal resolution and clear conflict messages.
///
/// # Arguments
///
/// * `direct_deps` - Map of package names to version constraints
/// * `registry` - Registry client to fetch package metadata
/// * `engine_version` - Optional engine version for filtering
/// * `force` - If true, bypasses engine compatibility checks
/// * `config` - Optional resolver configuration for timeouts, verbosity, etc.
pub fn resolve_dependencies(
    direct_deps: &HashMap<String, String>,
    registry: &RegistryClient,
    engine_version: Option<&str>,
    force: bool,
    config: Option<&ResolverConfig>,
) -> Result<HashMap<String, ResolvedPackage>> {
    // Delegate to PubGrub-based resolver
    pubgrub_resolve_dependencies(direct_deps, registry, engine_version, force, config)
}

/// Detect circular dependencies in a dependency graph
///
/// Returns an error if a circular dependency is found
pub fn detect_circular_deps(
    package_name: &str,
    dependencies: &HashMap<String, ResolvedPackage>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> Result<()> {
    if path.contains(&package_name.to_string()) {
        // Found a circular dependency
        let cycle_start = path.iter().position(|p| p == package_name).unwrap();
        let mut cycle: Vec<String> = path[cycle_start..].to_vec();
        cycle.push(package_name.to_string());
        return Err(Error::DependencyResolutionFailed(format!(
            "Circular dependency detected:\n\n  {}\n\n\
             This means these packages depend on each other in a loop.\n\
             One of these packages needs to remove its dependency to break the cycle.",
            cycle.join(" → ")
        )));
    }

    if visited.contains(package_name) {
        return Ok(());
    }

    visited.insert(package_name.to_string());
    path.push(package_name.to_string());

    // Check dependencies
    if let Some(package) = dependencies.get(package_name) {
        if let Some(deps) = &package.dependencies {
            for dep_name in deps.keys() {
                detect_circular_deps(dep_name, dependencies, visited, path)?;
            }
        }
    }

    path.pop();
    Ok(())
}

/// Version resolver using PubGrub algorithm
///
/// Note: This struct is kept for API compatibility but the actual resolution
/// is now done through the `resolve_dependencies` function which uses PubGrub.
pub struct Resolver;

impl Resolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::PackageType;

    /// Helper to create a test package version
    fn make_version(
        version: &str,
        engine_major: Option<i32>,
        engine_minor: Option<i32>,
        is_multi_engine: bool,
        engine_versions: Option<Vec<&str>>,
    ) -> PackageVersion {
        PackageVersion {
            version: version.to_string(),
            tarball: format!("{}.tar.gz", version),
            checksum: "abc123".to_string(),
            dependencies: None,
            engine_versions: engine_versions.map(|v| v.iter().map(|s| s.to_string()).collect()),
            engine_major,
            engine_minor,
            is_multi_engine,
            package_type: PackageType::Source,
            binaries: None,
            public_key: None,
            signed_at: None,
        }
    }

    /// Helper to create test package metadata
    fn make_metadata(name: &str, versions: Vec<PackageVersion>) -> PackageMetadata {
        PackageMetadata {
            name: name.to_string(),
            description: Some("Test package".to_string()),
            versions,
        }
    }

    // ============================================================================
    // find_matching_version tests
    // ============================================================================

    #[test]
    fn test_find_matching_version_exact() {
        let metadata = make_metadata(
            "test-pkg",
            vec![
                make_version("1.0.0", None, None, true, Some(vec!["5.3", "5.4"])),
                make_version("1.1.0", None, None, true, Some(vec!["5.3", "5.4"])),
                make_version("2.0.0", None, None, true, Some(vec!["5.3", "5.4"])),
            ],
        );

        let result = find_matching_version(&metadata, "=1.1.0", None, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version, "1.1.0");
    }

    #[test]
    fn test_find_matching_version_caret() {
        let metadata = make_metadata(
            "test-pkg",
            vec![
                make_version("1.0.0", None, None, true, Some(vec!["5.3"])),
                make_version("1.5.0", None, None, true, Some(vec!["5.3"])),
                make_version("1.9.0", None, None, true, Some(vec!["5.3"])),
                make_version("2.0.0", None, None, true, Some(vec!["5.3"])),
            ],
        );

        // ^1.0.0 should match >= 1.0.0, < 2.0.0 (highest is 1.9.0)
        let result = find_matching_version(&metadata, "^1.0.0", None, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version, "1.9.0");
    }

    #[test]
    fn test_find_matching_version_tilde() {
        let metadata = make_metadata(
            "test-pkg",
            vec![
                make_version("1.2.0", None, None, true, Some(vec!["5.3"])),
                make_version("1.2.5", None, None, true, Some(vec!["5.3"])),
                make_version("1.3.0", None, None, true, Some(vec!["5.3"])),
            ],
        );

        // ~1.2.0 should match >= 1.2.0, < 1.3.0 (highest is 1.2.5)
        let result = find_matching_version(&metadata, "~1.2.0", None, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version, "1.2.5");
    }

    #[test]
    fn test_find_matching_version_wildcard() {
        let metadata = make_metadata(
            "test-pkg",
            vec![
                make_version("1.0.0", None, None, true, Some(vec!["5.3"])),
                make_version("5.0.0", None, None, true, Some(vec!["5.3"])),
            ],
        );

        // * should match any version (highest is 5.0.0)
        let result = find_matching_version(&metadata, "*", None, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version, "5.0.0");
    }

    #[test]
    fn test_find_matching_version_engine_specific() {
        let metadata = make_metadata(
            "test-pkg",
            vec![
                make_version("1.0.0", Some(5), Some(3), false, None),
                make_version("1.0.0", Some(5), Some(4), false, None),
            ],
        );

        // Should only match 5.3 version
        let result = find_matching_version(&metadata, "^1.0.0", Some("5.3"), false);
        assert!(result.is_ok());
        let version = result.unwrap();
        assert_eq!(version.engine_major, Some(5));
        assert_eq!(version.engine_minor, Some(3));
    }

    #[test]
    fn test_find_matching_version_multi_engine() {
        let metadata = make_metadata(
            "test-pkg",
            vec![make_version(
                "1.0.0",
                None,
                None,
                true,
                Some(vec!["5.3", "5.4", "5.5"]),
            )],
        );

        // Should match for engine 5.4
        let result = find_matching_version(&metadata, "^1.0.0", Some("5.4"), false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version, "1.0.0");

        // Should NOT match for engine 5.2 (not in list)
        let result = find_matching_version(&metadata, "^1.0.0", Some("5.2"), false);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_matching_version_prefers_engine_specific() {
        let metadata = make_metadata(
            "test-pkg",
            vec![
                // Multi-engine version
                make_version("1.0.0", None, None, true, Some(vec!["5.3", "5.4"])),
                // Engine-specific version (should be preferred)
                make_version("1.0.0", Some(5), Some(3), false, None),
            ],
        );

        let result = find_matching_version(&metadata, "^1.0.0", Some("5.3"), false);
        assert!(result.is_ok());
        let version = result.unwrap();
        // Should prefer engine-specific over multi-engine
        assert!(!version.is_multi_engine);
    }

    #[test]
    fn test_find_matching_version_force_ignores_engine() {
        let metadata = make_metadata(
            "test-pkg",
            vec![make_version("1.0.0", Some(5), Some(3), false, None)],
        );

        // Without force: should fail for 5.4
        let result = find_matching_version(&metadata, "^1.0.0", Some("5.4"), false);
        assert!(result.is_err());

        // With force: should succeed
        let result = find_matching_version(&metadata, "^1.0.0", Some("5.4"), true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_find_matching_version_no_match() {
        let metadata = make_metadata(
            "test-pkg",
            vec![
                make_version("1.0.0", None, None, true, Some(vec!["5.3"])),
                make_version("1.1.0", None, None, true, Some(vec!["5.3"])),
            ],
        );

        // No version >= 2.0.0 exists
        let result = find_matching_version(&metadata, ">=2.0.0", None, false);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("No version"));
        assert!(err_msg.contains("test-pkg"));
    }

    #[test]
    fn test_find_matching_version_invalid_constraint() {
        let metadata = make_metadata(
            "test-pkg",
            vec![make_version("1.0.0", None, None, true, Some(vec!["5.3"]))],
        );

        let result = find_matching_version(&metadata, "invalid-constraint", None, false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid version"));
    }

    #[test]
    fn test_find_matching_version_highest_selected() {
        let metadata = make_metadata(
            "test-pkg",
            vec![
                make_version("1.0.0", None, None, true, Some(vec!["5.3"])),
                make_version("1.1.0", None, None, true, Some(vec!["5.3"])),
                make_version("1.2.0", None, None, true, Some(vec!["5.3"])),
            ],
        );

        // Should pick highest matching version
        let result = find_matching_version(&metadata, "^1.0.0", None, false);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().version, "1.2.0");
    }

    // ============================================================================
    // detect_circular_deps tests
    // ============================================================================

    #[test]
    fn test_detect_circular_deps_no_cycle() {
        // A -> B -> C (no cycle)
        let mut deps = HashMap::new();

        deps.insert(
            "A".to_string(),
            ResolvedPackage {
                name: "A".to_string(),
                version: "1.0.0".to_string(),
                checksum: "abc".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("B".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );
        deps.insert(
            "B".to_string(),
            ResolvedPackage {
                name: "B".to_string(),
                version: "1.0.0".to_string(),
                checksum: "def".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("C".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );
        deps.insert(
            "C".to_string(),
            ResolvedPackage {
                name: "C".to_string(),
                version: "1.0.0".to_string(),
                checksum: "ghi".to_string(),
                dependencies: None,
            },
        );

        let mut visited = HashSet::new();
        let mut path = Vec::new();

        let result = detect_circular_deps("A", &deps, &mut visited, &mut path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_circular_deps_direct_cycle() {
        // A -> B -> A (direct cycle)
        let mut deps = HashMap::new();

        deps.insert(
            "A".to_string(),
            ResolvedPackage {
                name: "A".to_string(),
                version: "1.0.0".to_string(),
                checksum: "abc".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("B".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );
        deps.insert(
            "B".to_string(),
            ResolvedPackage {
                name: "B".to_string(),
                version: "1.0.0".to_string(),
                checksum: "def".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("A".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );

        let mut visited = HashSet::new();
        let mut path = Vec::new();

        let result = detect_circular_deps("A", &deps, &mut visited, &mut path);
        assert!(result.is_err());

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("Circular dependency"));
        assert!(err_msg.contains("A") && err_msg.contains("B"));
    }

    #[test]
    fn test_detect_circular_deps_indirect_cycle() {
        // A -> B -> C -> A (indirect cycle)
        let mut deps = HashMap::new();

        deps.insert(
            "A".to_string(),
            ResolvedPackage {
                name: "A".to_string(),
                version: "1.0.0".to_string(),
                checksum: "abc".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("B".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );
        deps.insert(
            "B".to_string(),
            ResolvedPackage {
                name: "B".to_string(),
                version: "1.0.0".to_string(),
                checksum: "def".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("C".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );
        deps.insert(
            "C".to_string(),
            ResolvedPackage {
                name: "C".to_string(),
                version: "1.0.0".to_string(),
                checksum: "ghi".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("A".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );

        let mut visited = HashSet::new();
        let mut path = Vec::new();

        let result = detect_circular_deps("A", &deps, &mut visited, &mut path);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Circular dependency"));
    }

    #[test]
    fn test_detect_circular_deps_diamond_no_cycle() {
        // Diamond: A -> B, A -> C, B -> D, C -> D (no cycle)
        let mut deps = HashMap::new();

        deps.insert(
            "A".to_string(),
            ResolvedPackage {
                name: "A".to_string(),
                version: "1.0.0".to_string(),
                checksum: "abc".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("B".to_string(), "^1.0.0".to_string());
                    d.insert("C".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );
        deps.insert(
            "B".to_string(),
            ResolvedPackage {
                name: "B".to_string(),
                version: "1.0.0".to_string(),
                checksum: "def".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("D".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );
        deps.insert(
            "C".to_string(),
            ResolvedPackage {
                name: "C".to_string(),
                version: "1.0.0".to_string(),
                checksum: "ghi".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("D".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );
        deps.insert(
            "D".to_string(),
            ResolvedPackage {
                name: "D".to_string(),
                version: "1.0.0".to_string(),
                checksum: "jkl".to_string(),
                dependencies: None,
            },
        );

        let mut visited = HashSet::new();
        let mut path = Vec::new();

        let result = detect_circular_deps("A", &deps, &mut visited, &mut path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_circular_deps_empty_deps() {
        let deps = HashMap::new();
        let mut visited = HashSet::new();
        let mut path = Vec::new();

        // Package not in deps - should succeed (no dependencies to check)
        let result = detect_circular_deps("A", &deps, &mut visited, &mut path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_detect_circular_deps_self_dependency() {
        // A -> A (self-dependency)
        let mut deps = HashMap::new();

        deps.insert(
            "A".to_string(),
            ResolvedPackage {
                name: "A".to_string(),
                version: "1.0.0".to_string(),
                checksum: "abc".to_string(),
                dependencies: Some({
                    let mut d = HashMap::new();
                    d.insert("A".to_string(), "^1.0.0".to_string());
                    d
                }),
            },
        );

        let mut visited = HashSet::new();
        let mut path = Vec::new();

        let result = detect_circular_deps("A", &deps, &mut visited, &mut path);
        assert!(result.is_err());

        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Circular dependency"));
        assert!(err_msg.contains("A → A"));
    }

    // ============================================================================
    // Resolver struct tests
    // ============================================================================

    #[test]
    fn test_resolver_new() {
        let resolver = Resolver::new();
        // Just verify it creates without panic
        let _ = resolver;
    }

    #[test]
    fn test_resolver_default() {
        let _resolver: Resolver = Default::default();
    }
}
