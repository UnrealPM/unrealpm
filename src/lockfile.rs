//! Lockfile generation and parsing for reproducible builds
//!
//! This module handles the creation and parsing of `unrealpm.lock` files,
//! which ensure reproducible builds by locking exact package versions and checksums.
//!
//! Lockfiles use TOML format and should be committed to version control.
//!
//! # Examples
//!
//! ```no_run
//! use unrealpm::{Lockfile, LockedPackage};
//! use std::collections::HashMap;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load existing lockfile
//! if let Some(lockfile) = Lockfile::load()? {
//!     println!("Found {} packages in lockfile", lockfile.packages.len());
//! }
//!
//! // Create new lockfile
//! let mut lockfile = Lockfile::new();
//! let mut packages = HashMap::new();
//! packages.insert("awesome-plugin".to_string(), LockedPackage {
//!     version: "1.2.0".to_string(),
//!     checksum: "sha256:abc123...".to_string(),
//!     dependencies: Some(HashMap::new()),
//! });
//! lockfile.packages = packages;
//! lockfile.save()?;
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// The lockfile filename
pub const LOCKFILE_NAME: &str = "unrealpm.lock";

/// Represents the entire lockfile structure
///
/// Lockfiles contain exact versions and checksums for all installed packages,
/// ensuring reproducible builds across different machines and time periods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    /// Metadata about the lockfile
    #[serde(rename = "metadata")]
    pub metadata: LockfileMetadata,

    /// Map of package name to locked package info
    #[serde(rename = "package")]
    pub packages: HashMap<String, LockedPackage>,
}

/// Metadata about the lockfile generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockfileMetadata {
    /// Version of UnrealPM that generated this lockfile
    pub unrealpm_version: String,

    /// Timestamp when the lockfile was generated (ISO 8601 format)
    pub generated_at: String,
}

/// Information about a locked package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPackage {
    /// Exact version installed
    pub version: String,

    /// SHA256 checksum of the tarball
    pub checksum: String,

    /// Dependencies of this package (name -> version constraint)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<HashMap<String, String>>,
}

impl Lockfile {
    /// Create a new empty lockfile
    pub fn new() -> Self {
        Self {
            metadata: LockfileMetadata {
                unrealpm_version: env!("CARGO_PKG_VERSION").to_string(),
                generated_at: chrono::Utc::now().to_rfc3339(),
            },
            packages: HashMap::new(),
        }
    }

    /// Load lockfile from the current directory
    pub fn load() -> Result<Option<Self>> {
        Self::load_from(LOCKFILE_NAME)
    }

    /// Load lockfile from a specific path
    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Option<Self>> {
        let path = path.as_ref();

        if !path.exists() {
            return Ok(None);
        }

        let contents = fs::read_to_string(path)?;
        let lockfile: Lockfile = toml::from_str(&contents).map_err(|e| {
            Error::Other(format!("Failed to parse lockfile: {}", e))
        })?;

        Ok(Some(lockfile))
    }

    /// Save lockfile to the current directory
    pub fn save(&self) -> Result<()> {
        self.save_to(LOCKFILE_NAME)
    }

    /// Save lockfile to a specific path
    pub fn save_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let toml_string = toml::to_string_pretty(self)
            .map_err(|e| Error::Other(format!("Failed to serialize lockfile: {}", e)))?;

        fs::write(path.as_ref(), toml_string)?;
        Ok(())
    }

    /// Add or update a package in the lockfile
    pub fn update_package(
        &mut self,
        name: String,
        version: String,
        checksum: String,
        dependencies: Option<HashMap<String, String>>,
    ) {
        self.packages.insert(
            name,
            LockedPackage {
                version,
                checksum,
                dependencies,
            },
        );

        // Update metadata timestamp
        self.metadata.generated_at = chrono::Utc::now().to_rfc3339();
    }

    /// Remove a package from the lockfile
    pub fn remove_package(&mut self, name: &str) -> Option<LockedPackage> {
        let removed = self.packages.remove(name);

        if removed.is_some() {
            // Update metadata timestamp
            self.metadata.generated_at = chrono::Utc::now().to_rfc3339();
        }

        removed
    }

    /// Get a locked package by name
    pub fn get_package(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.get(name)
    }

    /// Check if a package is in the lockfile
    pub fn has_package(&self, name: &str) -> bool {
        self.packages.contains_key(name)
    }

    /// Get the number of packages in the lockfile
    pub fn package_count(&self) -> usize {
        self.packages.len()
    }
}

impl Default for Lockfile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lockfile_new() {
        let lockfile = Lockfile::new();
        assert_eq!(lockfile.packages.len(), 0);
        assert_eq!(lockfile.metadata.unrealpm_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn test_lockfile_update_package() {
        let mut lockfile = Lockfile::new();

        lockfile.update_package(
            "test-package".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            None,
        );

        assert_eq!(lockfile.package_count(), 1);
        assert!(lockfile.has_package("test-package"));

        let pkg = lockfile.get_package("test-package").unwrap();
        assert_eq!(pkg.version, "1.0.0");
        assert_eq!(pkg.checksum, "abc123");
    }

    #[test]
    fn test_lockfile_remove_package() {
        let mut lockfile = Lockfile::new();

        lockfile.update_package(
            "test-package".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            None,
        );

        assert!(lockfile.has_package("test-package"));

        let removed = lockfile.remove_package("test-package");
        assert!(removed.is_some());
        assert!(!lockfile.has_package("test-package"));
        assert_eq!(lockfile.package_count(), 0);
    }

    #[test]
    fn test_lockfile_serialization() {
        let mut lockfile = Lockfile::new();

        lockfile.update_package(
            "test-package".to_string(),
            "1.0.0".to_string(),
            "abc123".to_string(),
            None,
        );

        let toml_string = toml::to_string(&lockfile).unwrap();
        assert!(toml_string.contains("test-package"));
        assert!(toml_string.contains("1.0.0"));
        assert!(toml_string.contains("abc123"));
    }
}
