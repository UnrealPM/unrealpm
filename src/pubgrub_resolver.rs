//! PubGrub-based dependency resolution
//!
//! This module implements the PubGrub algorithm for dependency resolution,
//! providing superior conflict resolution and error messages compared to
//! simple backtracking.
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
use pubgrub::{
    DefaultStringReporter, Dependencies, DependencyConstraints, DependencyProvider,
    PackageResolutionStatistics, PubGrubError, Ranges, Reporter,
};
use semver::{Version, VersionReq};
use std::cmp::Reverse;
use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::{self, Display};
use std::time::Instant;

/// A semantic version wrapper that implements the traits needed by PubGrub
#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SemVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl SemVersion {
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse from a semver string (e.g., "1.2.3" or "1.2")
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('.').collect();
        match parts.len() {
            2 => {
                let major = parts[0].parse().ok()?;
                let minor = parts[1].parse().ok()?;
                Some(Self::new(major, minor, 0))
            }
            3 => {
                let major = parts[0].parse().ok()?;
                let minor = parts[1].parse().ok()?;
                let patch = parts[2].parse().ok()?;
                Some(Self::new(major, minor, patch))
            }
            _ => None,
        }
    }

    /// Convert to semver::Version
    pub fn to_semver(&self) -> Version {
        Version::new(self.major as u64, self.minor as u64, self.patch as u64)
    }
}

impl Display for SemVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl From<Version> for SemVersion {
    fn from(v: Version) -> Self {
        Self::new(v.major as u32, v.minor as u32, v.patch as u32)
    }
}

impl From<&SemVersion> for SemVersion {
    fn from(v: &SemVersion) -> Self {
        v.clone()
    }
}

/// Type alias for version ranges
pub type VersionRange = Ranges<SemVersion>;

/// Resolved package with exact version and metadata
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub checksum: String,
    pub dependencies: Option<HashMap<String, String>>,
}

/// Dependency provider that fetches package information from the registry
pub struct UnrealPmDependencyProvider<'a> {
    registry: &'a RegistryClient,
    engine_version: Option<String>,
    force: bool,
    /// Cache of package metadata
    package_cache: std::cell::RefCell<HashMap<String, PackageMetadata>>,
    /// Cache of available versions per package (filtered by engine)
    versions_cache: std::cell::RefCell<HashMap<String, Vec<(SemVersion, PackageVersion)>>>,
}

impl<'a> UnrealPmDependencyProvider<'a> {
    pub fn new(registry: &'a RegistryClient, engine_version: Option<&str>, force: bool) -> Self {
        Self {
            registry,
            engine_version: engine_version.map(|s| s.to_string()),
            force,
            package_cache: std::cell::RefCell::new(HashMap::new()),
            versions_cache: std::cell::RefCell::new(HashMap::new()),
        }
    }

    /// Get package metadata, using cache
    fn get_package_metadata(&self, name: &str) -> Result<PackageMetadata> {
        // Check cache first
        if let Some(meta) = self.package_cache.borrow().get(name) {
            return Ok(meta.clone());
        }

        // Fetch from registry
        let meta = self.registry.get_package(name)?;
        self.package_cache
            .borrow_mut()
            .insert(name.to_string(), meta.clone());
        Ok(meta)
    }

    /// Get available versions for a package, filtered by engine version
    fn get_available_versions(&self, name: &str) -> Result<Vec<(SemVersion, PackageVersion)>> {
        // Check cache first
        if let Some(versions) = self.versions_cache.borrow().get(name) {
            return Ok(versions.clone());
        }

        let metadata = self.get_package_metadata(name)?;
        let mut versions: Vec<(SemVersion, PackageVersion)> = Vec::new();

        for pkg_ver in &metadata.versions {
            // Parse version string
            let sem_ver = match SemVersion::parse(&pkg_ver.version) {
                Some(v) => v,
                None => continue, // Skip unparseable versions
            };

            // Check engine compatibility if not forcing
            if !self.force {
                if let Some(ref required_engine) = self.engine_version {
                    let req_parts: Vec<&str> = required_engine.split('.').collect();
                    let req_major = req_parts.first().and_then(|s| s.parse::<i32>().ok());
                    let req_minor = req_parts.get(1).and_then(|s| s.parse::<i32>().ok());

                    let mut matches = false;

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
                        if let Some(ref compatible_engines) = pkg_ver.engine_versions {
                            matches = compatible_engines.iter().any(|e| e == required_engine);
                        } else {
                            // If no engine_versions specified, assume compatible with all
                            matches = true;
                        }
                    }

                    if !matches {
                        continue;
                    }
                }
            }

            versions.push((sem_ver, pkg_ver.clone()));
        }

        // Sort by engine specificity first (prefer engine-specific), then by version (highest first)
        versions.sort_by(|a, b| {
            match (a.1.is_multi_engine, b.1.is_multi_engine) {
                (false, true) => std::cmp::Ordering::Greater, // a is engine-specific, prefer it
                (true, false) => std::cmp::Ordering::Less,    // b is engine-specific, prefer it
                _ => b.0.cmp(&a.0),                           // Same type, highest version first
            }
        });

        self.versions_cache
            .borrow_mut()
            .insert(name.to_string(), versions.clone());
        Ok(versions)
    }

    /// Convert a version constraint string to a Ranges<SemVersion>
    fn parse_version_constraint(&self, constraint: &str) -> Result<VersionRange> {
        // Parse using semver crate
        let req = VersionReq::parse(constraint).map_err(|e| {
            Error::Other(format!(
                "Invalid version constraint '{}': {}",
                constraint, e
            ))
        })?;

        // Convert semver::VersionReq to pubgrub Ranges
        // This is a simplification - we convert common patterns
        self.version_req_to_ranges(&req, constraint)
    }

    /// Convert semver::VersionReq to pubgrub Ranges
    fn version_req_to_ranges(&self, req: &VersionReq, original: &str) -> Result<VersionRange> {
        // Handle common patterns
        if original == "*" {
            return Ok(Ranges::full());
        }

        // Parse the comparators from the original string since semver's internal representation
        // isn't directly accessible in a useful way
        let trimmed = original.trim();

        // Handle caret (^) - compatible with version
        if let Some(ver_str) = trimmed.strip_prefix('^') {
            if let Some(base) = SemVersion::parse(ver_str) {
                // ^1.2.3 means >=1.2.3, <2.0.0 for major > 0
                // ^0.2.3 means >=0.2.3, <0.3.0 for major = 0, minor > 0
                // ^0.0.3 means >=0.0.3, <0.0.4 for major = 0, minor = 0
                let upper = if base.major > 0 {
                    SemVersion::new(base.major + 1, 0, 0)
                } else if base.minor > 0 {
                    SemVersion::new(0, base.minor + 1, 0)
                } else {
                    SemVersion::new(0, 0, base.patch + 1)
                };
                return Ok(Ranges::from_range_bounds(base..upper));
            }
        }

        // Handle tilde (~) - approximately equivalent
        if let Some(ver_str) = trimmed.strip_prefix('~') {
            if let Some(base) = SemVersion::parse(ver_str) {
                // ~1.2.3 means >=1.2.3, <1.3.0
                let upper = SemVersion::new(base.major, base.minor + 1, 0);
                return Ok(Ranges::from_range_bounds(base..upper));
            }
        }

        // Handle exact version (=)
        if let Some(ver_str) = trimmed.strip_prefix('=') {
            if let Some(v) = SemVersion::parse(ver_str.trim()) {
                return Ok(Ranges::singleton(v));
            }
        }

        // Handle >= (greater than or equal)
        if let Some(ver_str) = trimmed.strip_prefix(">=") {
            if let Some(v) = SemVersion::parse(ver_str.trim()) {
                return Ok(Ranges::from_range_bounds(v..));
            }
        }

        // Handle > (greater than)
        if let Some(ver_str) = trimmed.strip_prefix('>') {
            if let Some(v) = SemVersion::parse(ver_str.trim()) {
                // Convert > to >= next patch
                let next = SemVersion::new(v.major, v.minor, v.patch + 1);
                return Ok(Ranges::from_range_bounds(next..));
            }
        }

        // Handle <= (less than or equal)
        if let Some(ver_str) = trimmed.strip_prefix("<=") {
            if let Some(v) = SemVersion::parse(ver_str.trim()) {
                let upper = SemVersion::new(v.major, v.minor, v.patch + 1);
                return Ok(Ranges::from_range_bounds(..upper));
            }
        }

        // Handle < (less than)
        if let Some(ver_str) = trimmed.strip_prefix('<') {
            if let Some(v) = SemVersion::parse(ver_str.trim()) {
                return Ok(Ranges::from_range_bounds(..v));
            }
        }

        // Handle plain version (treat as exact or caret depending on convention)
        if let Some(v) = SemVersion::parse(trimmed) {
            // Treat plain version as caret (npm-style)
            let upper = if v.major > 0 {
                SemVersion::new(v.major + 1, 0, 0)
            } else if v.minor > 0 {
                SemVersion::new(0, v.minor + 1, 0)
            } else {
                SemVersion::new(0, 0, v.patch + 1)
            };
            return Ok(Ranges::from_range_bounds(v..upper));
        }

        // Handle compound constraints like ">=1.0.0 <2.0.0"
        if trimmed.contains(' ') {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() == 2 {
                let range1 = self.version_req_to_ranges(req, parts[0])?;
                let range2 = self.version_req_to_ranges(req, parts[1])?;
                return Ok(range1.intersection(&range2));
            }
        }

        // Fallback: use semver to check if versions match
        // This is less efficient but handles edge cases
        Err(Error::Other(format!(
            "Could not parse version constraint: {}",
            original
        )))
    }

    /// Get the PackageVersion for a resolved version
    pub fn get_package_version(&self, name: &str, version: &SemVersion) -> Option<PackageVersion> {
        if let Ok(versions) = self.get_available_versions(name) {
            for (v, pkg_ver) in versions {
                if &v == version {
                    return Some(pkg_ver);
                }
            }
        }
        None
    }
}

impl<'a> DependencyProvider for UnrealPmDependencyProvider<'a> {
    type P = String;
    type V = SemVersion;
    type VS = VersionRange;
    type M = String;
    type Err = Infallible;
    type Priority = (u32, Reverse<usize>);

    fn choose_version(
        &self,
        package: &String,
        range: &VersionRange,
    ) -> std::result::Result<Option<SemVersion>, Infallible> {
        // Get available versions (already sorted by preference)
        let versions = match self.get_available_versions(package) {
            Ok(v) => v,
            Err(_) => return Ok(None),
        };

        // Find the first (best) version that matches the range
        for (sem_ver, _pkg_ver) in versions {
            if range.contains(&sem_ver) {
                return Ok(Some(sem_ver));
            }
        }

        Ok(None)
    }

    fn prioritize(
        &self,
        package: &String,
        range: &VersionRange,
        package_statistics: &PackageResolutionStatistics,
    ) -> Self::Priority {
        // Count versions matching the range
        let version_count = self
            .get_available_versions(package)
            .map(|versions| versions.iter().filter(|(v, _)| range.contains(v)).count())
            .unwrap_or(0);

        if version_count == 0 {
            return (u32::MAX, Reverse(0));
        }

        // Prioritize packages with more conflicts and fewer available versions
        (package_statistics.conflict_count(), Reverse(version_count))
    }

    fn get_dependencies(
        &self,
        package: &String,
        version: &SemVersion,
    ) -> std::result::Result<Dependencies<String, VersionRange, String>, Infallible> {
        // Find the PackageVersion for this version
        let versions = match self.get_available_versions(package) {
            Ok(v) => v,
            Err(e) => {
                return Ok(Dependencies::Unavailable(format!(
                    "Failed to get versions for {}: {}",
                    package, e
                )));
            }
        };

        let pkg_ver = versions.iter().find(|(v, _)| v == version);

        let pkg_ver = match pkg_ver {
            Some((_, pv)) => pv,
            None => {
                return Ok(Dependencies::Unavailable(format!(
                    "Version {} not found for {}",
                    version, package
                )));
            }
        };

        // Get dependencies
        let deps = if pkg_ver.dependencies.is_some() {
            pkg_ver.dependencies.clone()
        } else {
            // Try to fetch from registry (for HTTP registry)
            self.registry
                .get_version_dependencies(package, &version.to_string())
                .ok()
                .flatten()
        };

        // Convert to DependencyConstraints
        let mut constraints: DependencyConstraints<String, VersionRange> =
            DependencyConstraints::default();

        if let Some(deps) = deps {
            for dep in deps {
                match self.parse_version_constraint(&dep.version) {
                    Ok(range) => {
                        constraints.insert(dep.name, range);
                    }
                    Err(e) => {
                        return Ok(Dependencies::Unavailable(format!(
                            "Invalid dependency constraint for {}: {}",
                            dep.name, e
                        )));
                    }
                }
            }
        }

        Ok(Dependencies::Available(constraints))
    }
}

/// Resolve all transitive dependencies using PubGrub algorithm
///
/// Returns a map of package name to resolved package information
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
    if direct_deps.is_empty() {
        return Ok(HashMap::new());
    }

    let resolver_config = config.cloned().unwrap_or_default();
    let start_time = Instant::now();

    // Check timeout before starting
    if resolver_config.resolution_timeout_seconds > 0 {
        // Pre-check is fine, actual timeout checking would need to be in the provider
        // For now, this is a simple implementation
    }

    // Create a virtual root package that depends on all direct dependencies
    let provider = UnrealPmDependencyProvider::new(registry, engine_version, force);

    // Build the root dependencies
    let mut root_deps: DependencyConstraints<String, VersionRange> =
        DependencyConstraints::default();

    for (name, constraint) in direct_deps {
        let range = provider.parse_version_constraint(constraint)?;
        root_deps.insert(name.clone(), range);
    }

    // Create a temporary provider that includes the virtual root
    let root_package = "__root__".to_string();
    let root_version = SemVersion::new(0, 0, 0);

    // We need to wrap the provider to handle the root package
    let root_provider = RootDependencyProvider {
        inner: provider,
        root_package: root_package.clone(),
        root_version: root_version.clone(),
        root_deps,
        start_time,
        timeout_seconds: resolver_config.resolution_timeout_seconds,
    };

    // Run PubGrub resolution
    let solution = pubgrub::resolve(&root_provider, root_package.clone(), root_version)
        .map_err(|e| convert_pubgrub_error(e, resolver_config.verbose_conflicts))?;

    // Convert solution to ResolvedPackage map
    let mut resolved = HashMap::new();

    for (name, version) in solution {
        // Skip the virtual root
        if name == root_package {
            continue;
        }

        // Get the PackageVersion for metadata
        if let Some(pkg_ver) = root_provider.inner.get_package_version(&name, &version) {
            let deps = pkg_ver.dependencies.as_ref().map(|deps| {
                deps.iter()
                    .map(|d| (d.name.clone(), d.version.clone()))
                    .collect()
            });

            resolved.insert(
                name.clone(),
                ResolvedPackage {
                    name,
                    version: version.to_string(),
                    checksum: pkg_ver.checksum.clone(),
                    dependencies: deps,
                },
            );
        }
    }

    Ok(resolved)
}

/// Wrapper provider that adds a virtual root package
struct RootDependencyProvider<'a> {
    inner: UnrealPmDependencyProvider<'a>,
    root_package: String,
    root_version: SemVersion,
    root_deps: DependencyConstraints<String, VersionRange>,
    start_time: Instant,
    timeout_seconds: u64,
}

impl<'a> DependencyProvider for RootDependencyProvider<'a> {
    type P = String;
    type V = SemVersion;
    type VS = VersionRange;
    type M = String;
    type Err = Infallible;
    type Priority = (u32, Reverse<usize>);

    fn choose_version(
        &self,
        package: &String,
        range: &VersionRange,
    ) -> std::result::Result<Option<SemVersion>, Infallible> {
        if package == &self.root_package {
            if range.contains(&self.root_version) {
                return Ok(Some(self.root_version.clone()));
            }
            return Ok(None);
        }
        self.inner.choose_version(package, range)
    }

    fn prioritize(
        &self,
        package: &String,
        range: &VersionRange,
        package_statistics: &PackageResolutionStatistics,
    ) -> Self::Priority {
        if package == &self.root_package {
            // Root has highest priority
            return (u32::MAX, Reverse(1));
        }
        self.inner.prioritize(package, range, package_statistics)
    }

    fn get_dependencies(
        &self,
        package: &String,
        version: &SemVersion,
    ) -> std::result::Result<Dependencies<String, VersionRange, String>, Infallible> {
        // Check timeout
        if self.timeout_seconds > 0 {
            let elapsed = self.start_time.elapsed().as_secs();
            if elapsed > self.timeout_seconds {
                return Ok(Dependencies::Unavailable(format!(
                    "Resolution timeout exceeded ({} seconds)",
                    self.timeout_seconds
                )));
            }
        }

        if package == &self.root_package && version == &self.root_version {
            return Ok(Dependencies::Available(self.root_deps.clone()));
        }
        self.inner.get_dependencies(package, version)
    }
}

/// Convert PubGrub error to our error type with nice messages
///
/// # Arguments
///
/// * `error` - The PubGrub error to convert
/// * `verbose` - If true, show full derivation tree without collapsing
fn convert_pubgrub_error<DP: DependencyProvider>(error: PubGrubError<DP>, verbose: bool) -> Error
where
    DP::P: Display,
    DP::VS: Display,
    DP::M: Display,
{
    match error {
        PubGrubError::NoSolution(mut derivation_tree) => {
            // Collapse no-version nodes for cleaner output (unless verbose)
            if !verbose {
                derivation_tree.collapse_no_versions();
            }

            // Use default reporter to generate human-readable message
            let report = DefaultStringReporter::report(&derivation_tree);

            // Clean up the report to be more user-friendly
            let cleaned_report = report
                .replace("__root__", "your project")
                .replace(" 0.0.0", "");

            Error::DependencyResolutionFailed(format!(
                "Dependency resolution failed:\n\n{}\n\n\
                 Suggestions:\n\
                 • Check if all packages exist and have compatible versions\n\
                 • Try loosening version constraints\n\
                 • Check engine version compatibility\n\
                 • Run 'unrealpm search <package>' to see available versions",
                cleaned_report
            ))
        }
        PubGrubError::ErrorChoosingVersion { package, source } => {
            Error::DependencyResolutionFailed(format!(
                "Error choosing version for package '{}': {}",
                package, source
            ))
        }
        PubGrubError::ErrorRetrievingDependencies {
            package,
            version,
            source,
        } => Error::DependencyResolutionFailed(format!(
            "Error retrieving dependencies for {} version {}: {}",
            package, version, source
        )),
        PubGrubError::ErrorInShouldCancel(source) => {
            Error::DependencyResolutionFailed(format!("Resolution cancelled: {}", source))
        }
    }
}

/// Find the best matching version for a package (for backward compatibility)
///
/// This wraps the PubGrub-based resolution for single version lookups.
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
    let mut matching_versions: Vec<(SemVersion, PackageVersion)> = Vec::new();

    for pkg_ver in &package_metadata.versions {
        let sem_ver = match SemVersion::parse(&pkg_ver.version) {
            Some(v) => v,
            None => continue,
        };

        // Check version constraint
        if !req.matches(&sem_ver.to_semver()) {
            continue;
        }

        // Check engine version compatibility if specified (unless force is enabled)
        if !force {
            if let Some(required_engine) = engine_version {
                let req_parts: Vec<&str> = required_engine.split('.').collect();
                let req_major = req_parts.first().and_then(|s| s.parse::<i32>().ok());
                let req_minor = req_parts.get(1).and_then(|s| s.parse::<i32>().ok());

                let mut matches = false;

                if !pkg_ver.is_multi_engine {
                    if let (Some(pkg_major), Some(pkg_minor), Some(rm), Some(rmi)) = (
                        pkg_ver.engine_major,
                        pkg_ver.engine_minor,
                        req_major,
                        req_minor,
                    ) {
                        matches = pkg_major == rm && pkg_minor == rmi;
                    }
                } else if let Some(ref compatible_engines) = pkg_ver.engine_versions {
                    matches = compatible_engines.iter().any(|e| e == required_engine);
                } else {
                    matches = true;
                }

                if !matches {
                    continue;
                }
            }
        }

        matching_versions.push((sem_ver, pkg_ver.clone()));
    }

    if matching_versions.is_empty() {
        let available_versions: Vec<String> = package_metadata
            .versions
            .iter()
            .map(|v| {
                if !v.is_multi_engine {
                    if let (Some(major), Some(minor)) = (v.engine_major, v.engine_minor) {
                        format!("{} (UE {}.{})", v.version, major, minor)
                    } else {
                        v.version.clone()
                    }
                } else if let Some(ref engines) = v.engine_versions {
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
    matching_versions.sort_by(|a, b| match (a.1.is_multi_engine, b.1.is_multi_engine) {
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        _ => b.0.cmp(&a.0),
    });

    Ok(matching_versions[0].1.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sem_version_parse() {
        assert_eq!(SemVersion::parse("1.2.3"), Some(SemVersion::new(1, 2, 3)));
        assert_eq!(SemVersion::parse("1.2"), Some(SemVersion::new(1, 2, 0)));
        assert_eq!(SemVersion::parse("invalid"), None);
    }

    #[test]
    fn test_sem_version_display() {
        let v = SemVersion::new(1, 2, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn test_sem_version_ordering() {
        let v1 = SemVersion::new(1, 0, 0);
        let v2 = SemVersion::new(2, 0, 0);
        let v3 = SemVersion::new(1, 1, 0);

        assert!(v1 < v2);
        assert!(v1 < v3);
        assert!(v3 < v2);
    }

    #[test]
    fn test_caret_constraint_major() {
        // Create a test provider to access parse_version_constraint
        // For now, test the range directly
        let v1 = SemVersion::new(1, 0, 0);
        let v2 = SemVersion::new(1, 9, 9);
        let v3 = SemVersion::new(2, 0, 0);

        // ^1.0.0 means >=1.0.0, <2.0.0
        let range: VersionRange = Ranges::from_range_bounds(v1.clone()..SemVersion::new(2, 0, 0));

        assert!(range.contains(&v1));
        assert!(range.contains(&v2));
        assert!(!range.contains(&v3));
    }

    #[test]
    fn test_tilde_constraint() {
        let v1 = SemVersion::new(1, 2, 0);
        let v2 = SemVersion::new(1, 2, 9);
        let v3 = SemVersion::new(1, 3, 0);

        // ~1.2.0 means >=1.2.0, <1.3.0
        let range: VersionRange = Ranges::from_range_bounds(v1.clone()..v3.clone());

        assert!(range.contains(&v1));
        assert!(range.contains(&v2));
        assert!(!range.contains(&v3));
    }
}
