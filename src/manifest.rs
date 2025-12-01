//! Manifest handling for unrealpm.json and .uproject files
//!
//! This module provides types and functions for working with UnrealPM manifests
//! and Unreal Engine project files.
//!
//! # Examples
//!
//! ```no_run
//! use unrealpm::Manifest;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load existing manifest
//! let manifest = Manifest::load(".")?;
//!
//! // Create new manifest
//! let mut new_manifest = Manifest::new();
//! new_manifest.engine_version = Some("5.3".to_string());
//! new_manifest.save(".")?;
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// UnrealPM manifest file (unrealpm.json)
///
/// This struct represents the project's package manifest, which contains metadata
/// about the project and its dependencies. The manifest is stored as `unrealpm.json`
/// in the project root directory.
///
/// # Examples
///
/// ```no_run
/// use unrealpm::Manifest;
/// use std::collections::HashMap;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut manifest = Manifest::new();
/// manifest.name = Some("MyProject".to_string());
/// manifest.engine_version = Some("5.3".to_string());
///
/// let mut deps = HashMap::new();
/// deps.insert("awesome-plugin".to_string(), "^1.0.0".to_string());
/// manifest.dependencies = deps;
///
/// manifest.save(".")?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Package name (optional for projects, required for plugins)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Package version (optional for projects, required for plugins)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Package description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Unreal Engine version (e.g., "5.3", "5.4")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,

    /// Runtime dependencies
    #[serde(default)]
    pub dependencies: HashMap<String, String>,

    /// Development dependencies (not installed with --production)
    #[serde(default)]
    pub dev_dependencies: HashMap<String, String>,
}

impl Manifest {
    /// Create a new empty manifest
    pub fn new() -> Self {
        Self {
            name: None,
            version: None,
            description: None,
            engine_version: None,
            dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
        }
    }

    /// Load manifest from unrealpm.json in the given directory
    pub fn load<P: AsRef<Path>>(dir: P) -> Result<Self> {
        let manifest_path = dir.as_ref().join("unrealpm.json");

        if !manifest_path.exists() {
            return Err(Error::InvalidManifest(
                "unrealpm.json not found. Run 'unrealpm init' first.".to_string()
            ));
        }

        let content = fs::read_to_string(&manifest_path)?;
        let manifest: Manifest = serde_json::from_str(&content)?;

        Ok(manifest)
    }

    /// Save manifest to unrealpm.json in the given directory
    pub fn save<P: AsRef<Path>>(&self, dir: P) -> Result<()> {
        let manifest_path = dir.as_ref().join("unrealpm.json");
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&manifest_path, content)?;
        Ok(())
    }

    /// Check if unrealpm.json exists in the given directory
    pub fn exists<P: AsRef<Path>>(dir: P) -> bool {
        dir.as_ref().join("unrealpm.json").exists()
    }
}

impl Default for Manifest {
    fn default() -> Self {
        Self::new()
    }
}

/// Unreal Engine project file (.uproject)
///
/// Represents the structure of an Unreal Engine .uproject file.
/// This is used to extract the engine version and other metadata.
///
/// # Examples
///
/// ```no_run
/// use unrealpm::UProject;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Find .uproject file in current directory
/// let uproject_path = UProject::find(".")?;
///
/// // Load and parse it
/// let uproject = UProject::load(&uproject_path)?;
///
/// println!("Engine version: {}", uproject.engine_association);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UProject {
    #[serde(rename = "FileVersion")]
    pub file_version: i32,

    #[serde(rename = "EngineAssociation")]
    pub engine_association: String,

    #[serde(rename = "Category", skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(rename = "Plugins", default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<UProjectPlugin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UProjectPlugin {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Enabled")]
    pub enabled: bool,

    #[serde(rename = "MarketplaceURL", skip_serializing_if = "Option::is_none")]
    pub marketplace_url: Option<String>,
}

impl UProject {
    /// Find .uproject file in the given directory
    pub fn find<P: AsRef<Path>>(dir: P) -> Result<PathBuf> {
        let dir = dir.as_ref();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("uproject") {
                return Ok(path);
            }
        }

        Err(Error::NoUProjectFile)
    }

    /// Load .uproject file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let uproject: UProject = serde_json::from_str(&content)?;
        Ok(uproject)
    }

    /// Get project name from filename
    pub fn name<P: AsRef<Path>>(path: P) -> Option<String> {
        path.as_ref()
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    }
}

/// Unreal Engine plugin file (.uplugin)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UPlugin {
    #[serde(rename = "FileVersion")]
    pub file_version: i32,

    #[serde(rename = "Version", deserialize_with = "deserialize_version")]
    pub version: i32,

    #[serde(rename = "VersionName")]
    pub version_name: String,

    #[serde(rename = "FriendlyName")]
    pub friendly_name: String,

    #[serde(rename = "Description", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(rename = "Category", skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    #[serde(rename = "CreatedBy", skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,

    #[serde(rename = "CreatedByURL", skip_serializing_if = "Option::is_none")]
    pub created_by_url: Option<String>,

    #[serde(rename = "DocsURL", skip_serializing_if = "Option::is_none")]
    pub docs_url: Option<String>,

    #[serde(rename = "MarketplaceURL", skip_serializing_if = "Option::is_none")]
    pub marketplace_url: Option<String>,

    #[serde(rename = "SupportURL", skip_serializing_if = "Option::is_none")]
    pub support_url: Option<String>,

    #[serde(rename = "EngineVersion", skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,

    #[serde(rename = "CanContainContent", skip_serializing_if = "Option::is_none")]
    pub can_contain_content: Option<bool>,

    #[serde(rename = "IsBetaVersion", skip_serializing_if = "Option::is_none")]
    pub is_beta_version: Option<bool>,

    #[serde(rename = "Plugins", default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<UPluginDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UPluginDependency {
    #[serde(rename = "Name")]
    pub name: String,

    #[serde(rename = "Enabled")]
    pub enabled: bool,
}

impl UPlugin {
    /// Find .uplugin file in the given directory
    pub fn find<P: AsRef<Path>>(dir: P) -> Result<PathBuf> {
        let dir = dir.as_ref();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("uplugin") {
                return Ok(path);
            }
        }

        Err(Error::InvalidManifest(
            "No .uplugin file found in current directory".to_string()
        ))
    }

    /// Load .uplugin file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let uplugin: UPlugin = serde_json::from_str(&content)?;
        Ok(uplugin)
    }

    /// Get plugin name from filename
    pub fn name<P: AsRef<Path>>(path: P) -> Option<String> {
        path.as_ref()
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_manifest_new() {
        let manifest = Manifest::new();
        assert!(manifest.name.is_none());
        assert!(manifest.version.is_none());
        assert!(manifest.engine_version.is_none());
        assert!(manifest.dependencies.is_empty());
        assert!(manifest.dev_dependencies.is_empty());
    }

    #[test]
    fn test_manifest_serialization() {
        let mut manifest = Manifest::new();
        manifest.name = Some("TestProject".to_string());
        manifest.engine_version = Some("5.3".to_string());

        let mut deps = HashMap::new();
        deps.insert("awesome-plugin".to_string(), "^1.0.0".to_string());
        manifest.dependencies = deps;

        // Serialize to JSON
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("TestProject"));
        assert!(json.contains("5.3"));
        assert!(json.contains("awesome-plugin"));

        // Deserialize back
        let deserialized: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, Some("TestProject".to_string()));
        assert_eq!(deserialized.engine_version, Some("5.3".to_string()));
        assert_eq!(deserialized.dependencies.len(), 1);
    }

    #[test]
    fn test_uproject_name() {
        let path = std::path::Path::new("/path/to/MyProject.uproject");
        let name = UProject::name(path);
        assert_eq!(name, Some("MyProject".to_string()));
    }

    #[test]
    fn test_uplugin_name() {
        let path = std::path::Path::new("/path/to/MyPlugin.uplugin");
        let name = UPlugin::name(path);
        assert_eq!(name, Some("MyPlugin".to_string()));
    }

    #[test]
    fn test_uproject_parse() {
        let json = r#"{
            "FileVersion": 3,
            "EngineAssociation": "5.3",
            "Description": "Test project"
        }"#;

        let uproject: UProject = serde_json::from_str(json).unwrap();
        assert_eq!(uproject.file_version, 3);
        assert_eq!(uproject.engine_association, "5.3");
        assert_eq!(uproject.description, Some("Test project".to_string()));
    }

    #[test]
    fn test_uplugin_parse() {
        let json = r#"{
            "FileVersion": 3,
            "Version": 1,
            "VersionName": "1.0.0",
            "FriendlyName": "My Plugin",
            "Description": "A test plugin",
            "Category": "Gameplay",
            "CreatedBy": "Test",
            "CreatedByURL": "",
            "DocsURL": "",
            "MarketplaceURL": "",
            "SupportURL": "",
            "EngineVersion": "5.3.0",
            "CanContainContent": true,
            "IsBetaVersion": false,
            "IsExperimentalVersion": false,
            "Installed": false,
            "Modules": []
        }"#;

        let uplugin: UPlugin = serde_json::from_str(json).unwrap();
        assert_eq!(uplugin.file_version, 3);
        assert_eq!(uplugin.version, 1);
        assert_eq!(uplugin.version_name, "1.0.0");
        assert_eq!(uplugin.friendly_name, "My Plugin");
        assert_eq!(uplugin.category, Some("Gameplay".to_string()));
    }
}

/// Custom deserializer for Version field that accepts both int and float
fn deserialize_version<'de, D>(deserializer: D) -> std::result::Result<i32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct VersionVisitor;

    impl<'de> de::Visitor<'de> for VersionVisitor {
        type Value = i32;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or float version number")
        }

        fn visit_i64<E>(self, value: i64) -> std::result::Result<i32, E>
        where
            E: de::Error,
        {
            Ok(value as i32)
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<i32, E>
        where
            E: de::Error,
        {
            Ok(value as i32)
        }

        fn visit_f64<E>(self, value: f64) -> std::result::Result<i32, E>
        where
            E: de::Error,
        {
            // Convert float to int (5.3 -> 53000, 4.27 -> 42700)
            let major = value.floor() as i32;
            let minor = ((value - major as f64) * 100.0).round() as i32;
            Ok(major * 10000 + minor * 100)
        }
    }

    deserializer.deserialize_any(VersionVisitor)
}
