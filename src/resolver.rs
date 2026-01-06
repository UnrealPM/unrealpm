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
