//! Package registry client and metadata types
//!
//! This module provides a file-based registry client for MVP.
//! Phase 2 will migrate to an HTTP-based registry with PostgreSQL backend.
//!
//! # Examples
//!
//! ```no_run
//! use unrealpm::RegistryClient;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let registry = RegistryClient::new(std::env::var("HOME").unwrap() + "/.unrealpm-registry");
//!
//! // Search for packages
//! let results = registry.search("multiplayer")?;
//! for package_name in results {
//!     let pkg = registry.get_package(&package_name)?;
//!     println!("{}: {}", pkg.name, pkg.description.unwrap_or_default());
//! }
//!
//! // Get package metadata
//! let metadata = registry.get_package("awesome-plugin")?;
//! if let Some(latest) = metadata.versions.last() {
//!     println!("Latest version: {}", latest.version);
//! }
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Package metadata stored in registry
///
/// Contains information about a package including all available versions
/// and their dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    pub name: String,
    pub description: Option<String>,
    pub versions: Vec<PackageVersion>,
}

/// Package type indicating what's included in the package
///
/// - `Source`: Only source code, requires building
/// - `Binary`: Pre-built binaries for specific platforms/engines
/// - `Hybrid`: Both source and pre-built binaries available
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PackageType {
    /// Source code only, requires building with RunUAT
    Source,
    /// Pre-built binaries only
    Binary,
    /// Both source and pre-built binaries
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageVersion {
    pub version: String,
    pub tarball: String,
    pub checksum: String,
    pub dependencies: Option<Vec<Dependency>>,
    /// Compatible Unreal Engine versions (e.g., ["5.3", "5.4"]) - for multi-engine versions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_versions: Option<Vec<String>>,
    /// Specific engine major version (e.g., 4, 5) - for engine-specific versions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_major: Option<i32>,
    /// Specific engine minor version (e.g., 27, 3) - for engine-specific versions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_minor: Option<i32>,
    /// Is this version compatible across multiple engines?
    #[serde(default = "default_multi_engine")]
    pub is_multi_engine: bool,
    /// Package type (source, binary, or hybrid)
    #[serde(default = "default_package_type")]
    pub package_type: PackageType,
    /// Pre-built binaries (for binary/hybrid packages)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binaries: Option<Vec<PrebuiltBinary>>,
    /// Ed25519 public key (hex-encoded) for signature verification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    /// Timestamp when package was signed (ISO 8601)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signed_at: Option<String>,
}

fn default_multi_engine() -> bool {
    true
}

fn default_package_type() -> PackageType {
    PackageType::Source
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrebuiltBinary {
    pub platform: String,
    pub engine: String,
    pub tarball: String,
    pub checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version: String,
}

pub enum RegistryClient {
    File(FileRegistryClient),
    Http(crate::registry_http::HttpRegistryClient),
}

pub struct FileRegistryClient {
    registry_path: PathBuf,
}

impl FileRegistryClient {
    /// Create a new file registry client
    pub fn new<P: AsRef<Path>>(registry_path: P) -> Self {
        Self {
            registry_path: registry_path.as_ref().to_path_buf(),
        }
    }
}

impl RegistryClient {
    /// Get the default local registry path (~/.unrealpm-registry)
    pub fn default_registry_path() -> Result<PathBuf> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| Error::Other("Could not find home directory".to_string()))?;

        Ok(PathBuf::from(home).join(".unrealpm-registry"))
    }

    /// Create a registry client using configuration
    pub fn from_config(config: &crate::Config) -> Result<Self> {
        match config.registry.registry_type.as_str() {
            "http" => {
                let cache_dir = Self::default_registry_path()?;
                let http_client = crate::registry_http::HttpRegistryClient::new(
                    config.registry.url.clone(),
                    cache_dir,
                    config.auth.token.clone(),
                )?;
                Ok(RegistryClient::Http(http_client))
            }
            _ => {
                // Default to file-based
                let path = Self::default_registry_path()?;
                Ok(RegistryClient::File(FileRegistryClient::new(path)))
            }
        }
    }

    /// Create a registry client using the default (file-based for backward compat)
    pub fn new_default() -> Result<Self> {
        let path = Self::default_registry_path()?;
        Ok(RegistryClient::File(FileRegistryClient::new(path)))
    }

    /// Get package metadata from registry
    pub fn get_package(&self, name: &str) -> Result<PackageMetadata> {
        match self {
            RegistryClient::File(client) => client.get_package(name),
            RegistryClient::Http(client) => client.get_package(name),
        }
    }

    /// Get path to package tarball
    pub fn get_tarball_path(&self, name: &str, version: &str) -> PathBuf {
        match self {
            RegistryClient::File(client) => client.get_tarball_path(name, version),
            RegistryClient::Http(client) => client.get_tarball_path(name, version),
        }
    }

    /// Get path to signature file
    pub fn get_signature_path(&self, name: &str, version: &str) -> PathBuf {
        match self {
            RegistryClient::File(client) => client.get_signature_path(name, version),
            RegistryClient::Http(client) => client.get_signature_path(name, version),
        }
    }

    /// Download signature file (for HTTP registries, downloads from server; for file registries, returns local path)
    pub fn download_signature(&self, name: &str, version: &str) -> Result<PathBuf> {
        match self {
            RegistryClient::File(client) => {
                // For file registry, signature is already local
                Ok(client.get_signature_path(name, version))
            }
            RegistryClient::Http(client) => {
                // For HTTP registry, download from server
                client.download_signature(name, version)
            }
        }
    }

    /// Get tarballs directory
    pub fn get_tarballs_dir(&self) -> PathBuf {
        match self {
            RegistryClient::File(client) => client.get_tarballs_dir(),
            RegistryClient::Http(client) => client.get_tarballs_dir(),
        }
    }

    /// Get signatures directory
    pub fn get_signatures_dir(&self) -> PathBuf {
        match self {
            RegistryClient::File(client) => client.get_signatures_dir(),
            RegistryClient::Http(client) => client.get_signatures_dir(),
        }
    }

    /// Get packages directory
    pub fn get_packages_dir(&self) -> PathBuf {
        match self {
            RegistryClient::File(client) => client.get_packages_dir(),
            RegistryClient::Http(client) => client.get_packages_dir(),
        }
    }

    /// Search for packages
    pub fn search(&self, query: &str) -> Result<Vec<String>> {
        match self {
            RegistryClient::File(client) => client.search(query),
            RegistryClient::Http(client) => client.search(query),
        }
    }

    /// Search for packages with full metadata
    pub fn search_packages(
        &self,
        query: &str,
    ) -> Result<Vec<crate::registry_http::ApiPackageInfo>> {
        match self {
            RegistryClient::File(client) => {
                // For file registry, get basic info from each package
                let names = client.search(query)?;
                let mut results = Vec::new();
                for name in names {
                    if let Ok(pkg) = client.get_package(&name) {
                        results.push(crate::registry_http::ApiPackageInfo {
                            name: pkg.name,
                            description: pkg.description,
                            latest_version: pkg.versions.last().map(|v| v.version.clone()),
                        });
                    }
                }
                Ok(results)
            }
            RegistryClient::Http(client) => client.search_packages(query),
        }
    }

    /// Get dependencies for a specific package version
    /// For HTTP registry, this fetches from the version detail endpoint
    /// For file registry, dependencies are already in the package metadata
    pub fn get_version_dependencies(
        &self,
        name: &str,
        version: &str,
    ) -> Result<Option<Vec<Dependency>>> {
        match self {
            RegistryClient::File(client) => {
                // For file registry, dependencies are in the package metadata
                let pkg = client.get_package(name)?;
                for v in &pkg.versions {
                    if v.version == version {
                        return Ok(v.dependencies.clone());
                    }
                }
                Ok(None)
            }
            RegistryClient::Http(client) => client.get_version_dependencies(name, version),
        }
    }
}

impl FileRegistryClient {
    pub fn get_package(&self, name: &str) -> Result<PackageMetadata> {
        let package_file = self
            .registry_path
            .join("packages")
            .join(format!("{}.json", name));

        if !package_file.exists() {
            // Try to find similar package names for suggestions
            let similar = self.find_similar_packages(name);

            let mut error_msg = format!("Package '{}' not found in registry", name);

            if !similar.is_empty() {
                error_msg.push_str("\n\nDid you mean one of these?\n  ");
                error_msg.push_str(&similar.join("\n  "));
            }

            error_msg.push_str("\n\nSuggestions:\n");
            error_msg.push_str("  • Check the package name spelling\n");
            error_msg.push_str("  • Search for packages: unrealpm search <query>\n");
            error_msg.push_str("  • Visit the package registry for available packages");

            return Err(Error::PackageNotFound(error_msg));
        }

        let content = fs::read_to_string(&package_file)?;
        let metadata: PackageMetadata = serde_json::from_str(&content)?;

        Ok(metadata)
    }

    /// Find packages with similar names using simple edit distance
    fn find_similar_packages(&self, query: &str) -> Vec<String> {
        let packages_dir = self.registry_path.join("packages");

        if !packages_dir.exists() {
            return Vec::new();
        }

        let mut similar = Vec::new();

        if let Ok(entries) = fs::read_dir(packages_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        // Simple similarity check: substring match or low edit distance
                        if name.contains(query)
                            || query.contains(name)
                            || self.levenshtein_distance(query, name) <= 3
                        {
                            similar.push(name.to_string());
                        }
                    }
                }
            }
        }

        similar.sort();
        similar.truncate(5); // Show max 5 suggestions
        similar
    }

    /// Calculate Levenshtein distance between two strings
    fn levenshtein_distance(&self, s1: &str, s2: &str) -> usize {
        let len1 = s1.chars().count();
        let len2 = s2.chars().count();
        let mut matrix = vec![vec![0; len2 + 1]; len1 + 1];

        for (i, row) in matrix.iter_mut().enumerate().take(len1 + 1) {
            row[0] = i;
        }
        for (j, val) in matrix[0].iter_mut().enumerate().take(len2 + 1) {
            *val = j;
        }

        for (i, c1) in s1.chars().enumerate() {
            for (j, c2) in s2.chars().enumerate() {
                let cost = if c1 == c2 { 0 } else { 1 };
                matrix[i + 1][j + 1] = std::cmp::min(
                    std::cmp::min(matrix[i][j + 1] + 1, matrix[i + 1][j] + 1),
                    matrix[i][j] + cost,
                );
            }
        }

        matrix[len1][len2]
    }

    /// Get path to package tarball
    pub fn get_tarball_path(&self, name: &str, version: &str) -> PathBuf {
        self.registry_path
            .join("tarballs")
            .join(format!("{}-{}.tar.gz", name, version))
    }

    /// Get the tarballs directory path
    pub fn get_tarballs_dir(&self) -> PathBuf {
        self.registry_path.join("tarballs")
    }

    /// Get the packages directory path
    pub fn get_packages_dir(&self) -> PathBuf {
        self.registry_path.join("packages")
    }

    /// Get the signatures directory path
    pub fn get_signatures_dir(&self) -> PathBuf {
        self.registry_path.join("signatures")
    }

    /// Get path to package signature file
    pub fn get_signature_path(&self, name: &str, version: &str) -> PathBuf {
        self.registry_path
            .join("signatures")
            .join(format!("{}-{}.sig", name, version))
    }

    /// Search for packages (simple substring search for MVP)
    pub fn search(&self, query: &str) -> Result<Vec<String>> {
        let packages_dir = self.registry_path.join("packages");

        if !packages_dir.exists() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();

        for entry in fs::read_dir(packages_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    if name.to_lowercase().contains(&query.to_lowercase()) {
                        results.push(name.to_string());
                    }
                }
            }
        }

        Ok(results)
    }

    /// Initialize registry directory structure
    pub fn init_registry(&self) -> Result<()> {
        fs::create_dir_all(self.registry_path.join("packages"))?;
        fs::create_dir_all(self.registry_path.join("tarballs"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_package_type_serialization() {
        // Test PackageType serialization
        let source = PackageType::Source;
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, "\"source\"");

        let binary = PackageType::Binary;
        let json = serde_json::to_string(&binary).unwrap();
        assert_eq!(json, "\"binary\"");

        let hybrid = PackageType::Hybrid;
        let json = serde_json::to_string(&hybrid).unwrap();
        assert_eq!(json, "\"hybrid\"");
    }

    #[test]
    fn test_package_type_deserialization() {
        let source: PackageType = serde_json::from_str("\"source\"").unwrap();
        assert_eq!(source, PackageType::Source);

        let binary: PackageType = serde_json::from_str("\"binary\"").unwrap();
        assert_eq!(binary, PackageType::Binary);

        let hybrid: PackageType = serde_json::from_str("\"hybrid\"").unwrap();
        assert_eq!(hybrid, PackageType::Hybrid);
    }

    #[test]
    fn test_package_metadata_parse() {
        let json = r#"{
            "name": "awesome-plugin",
            "description": "An awesome plugin",
            "versions": [
                {
                    "version": "1.0.0",
                    "tarball": "awesome-plugin-1.0.0.tar.gz",
                    "checksum": "sha256:abc123",
                    "package_type": "source"
                }
            ]
        }"#;

        let metadata: PackageMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.name, "awesome-plugin");
        assert_eq!(metadata.description, Some("An awesome plugin".to_string()));
        assert_eq!(metadata.versions.len(), 1);
        assert_eq!(metadata.versions[0].version, "1.0.0");
        assert_eq!(metadata.versions[0].package_type, PackageType::Source);
    }

    #[test]
    fn test_dependency_parse() {
        let json = r#"{
            "name": "dep-plugin",
            "version": "^1.0.0"
        }"#;

        let dep: Dependency = serde_json::from_str(json).unwrap();
        assert_eq!(dep.name, "dep-plugin");
        assert_eq!(dep.version, "^1.0.0");
    }

    #[test]
    fn test_prebuilt_binary_parse() {
        let json = r#"{
            "platform": "Win64",
            "engine": "5.3",
            "tarball": "awesome-plugin-win64-5.3.tar.gz",
            "checksum": "sha256:xyz789"
        }"#;

        let binary: PrebuiltBinary = serde_json::from_str(json).unwrap();
        assert_eq!(binary.platform, "Win64");
        assert_eq!(binary.engine, "5.3");
        assert_eq!(binary.tarball, "awesome-plugin-win64-5.3.tar.gz");
        assert_eq!(binary.checksum, "sha256:xyz789");
    }

    #[test]
    fn test_default_package_type() {
        let pkg_type = default_package_type();
        assert_eq!(pkg_type, PackageType::Source);
    }
}
