//! Test utilities and helpers for UnrealPM integration tests.
//!
//! This module provides common utilities for setting up test environments,
//! creating test fixtures, and asserting test results.

use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Production registry URL
pub const PRODUCTION_REGISTRY: &str = "https://registry.unreal.dev";

/// Local development registry URL
pub const LOCAL_REGISTRY: &str = "http://localhost:3000";

/// Test project configuration
pub struct TestProject {
    pub temp_dir: TempDir,
    pub project_path: PathBuf,
    pub config_dir: PathBuf,
}

impl TestProject {
    /// Create a new isolated test project
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let project_path = temp_dir.path().to_path_buf();
        let config_dir = project_path.join(".unrealpm");

        fs::create_dir_all(&config_dir).expect("Failed to create config directory");

        Self {
            temp_dir,
            project_path,
            config_dir,
        }
    }

    /// Create with a specific engine version
    pub fn with_engine(engine_version: &str) -> Self {
        let project = Self::new();
        project.create_uproject(engine_version);
        project
    }

    /// Create .uproject file
    pub fn create_uproject(&self, engine_version: &str) -> PathBuf {
        let uproject_path = self.project_path.join("TestProject.uproject");
        let content = format!(
            r#"{{
    "FileVersion": 3,
    "EngineAssociation": "{}",
    "Category": "",
    "Description": "Test project for UnrealPM integration tests",
    "Modules": [],
    "Plugins": []
}}"#,
            engine_version
        );
        fs::write(&uproject_path, content).expect("Failed to write .uproject");
        uproject_path
    }

    /// Configure to use HTTP registry
    pub fn configure_http_registry(&self, registry_url: &str) {
        let config = format!(
            r#"[registry]
registry_type = "http"
url = "{}"

[signing]
enabled = true

[verification]
require_signatures = false

[build]
platforms = ["Win64"]
configuration = "Development"
"#,
            registry_url
        );
        fs::write(self.config_dir.join("config.toml"), config).expect("Failed to write config");
    }

    /// Configure to use file-based registry
    pub fn configure_file_registry(&self, registry_path: &Path) {
        let config = format!(
            r#"[registry]
registry_type = "file"
path = "{}"

[signing]
enabled = false

[verification]
require_signatures = false
"#,
            registry_path.display()
        );
        fs::write(self.config_dir.join("config.toml"), config).expect("Failed to write config");
    }

    /// Get path to the project directory
    pub fn path(&self) -> &Path {
        &self.project_path
    }

    /// Get path to config directory
    pub fn config_path(&self) -> &Path {
        &self.config_dir
    }

    /// Check if manifest exists
    pub fn has_manifest(&self) -> bool {
        self.project_path.join("unrealpm.json").exists()
    }

    /// Read manifest content
    pub fn read_manifest(&self) -> String {
        fs::read_to_string(self.project_path.join("unrealpm.json"))
            .expect("Failed to read manifest")
    }

    /// Check if lockfile exists
    pub fn has_lockfile(&self) -> bool {
        self.project_path.join("unrealpm.lock").exists()
    }

    /// Read lockfile content
    pub fn read_lockfile(&self) -> String {
        fs::read_to_string(self.project_path.join("unrealpm.lock"))
            .expect("Failed to read lockfile")
    }

    /// Check if a plugin is installed
    pub fn has_plugin(&self, name: &str) -> bool {
        self.project_path.join("Plugins").join(name).exists()
    }

    /// Get plugins directory
    pub fn plugins_dir(&self) -> PathBuf {
        self.project_path.join("Plugins")
    }

    /// List installed plugins
    pub fn list_plugins(&self) -> Vec<String> {
        let plugins_dir = self.plugins_dir();
        if !plugins_dir.exists() {
            return vec![];
        }
        fs::read_dir(plugins_dir)
            .expect("Failed to read plugins directory")
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    if e.path().is_dir() {
                        e.file_name().to_str().map(String::from)
                    } else {
                        None
                    }
                })
            })
            .collect()
    }
}

impl Default for TestProject {
    fn default() -> Self {
        Self::new()
    }
}

/// Test fixture for a mock plugin
pub struct MockPlugin {
    pub name: String,
    pub version: String,
    pub engine_versions: Vec<String>,
    pub dependencies: Vec<(String, String)>,
}

impl MockPlugin {
    pub fn new(name: &str, version: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            engine_versions: vec!["5.3".to_string(), "5.4".to_string()],
            dependencies: vec![],
        }
    }

    pub fn with_engine_versions(mut self, versions: Vec<&str>) -> Self {
        self.engine_versions = versions.into_iter().map(String::from).collect();
        self
    }

    pub fn with_dependency(mut self, name: &str, version: &str) -> Self {
        self.dependencies
            .push((name.to_string(), version.to_string()));
        self
    }

    /// Create .uplugin content
    pub fn uplugin_content(&self) -> String {
        format!(
            r#"{{
    "FileVersion": 3,
    "Version": 1,
    "VersionName": "{}",
    "FriendlyName": "{}",
    "Description": "Test plugin",
    "Category": "Testing",
    "CreatedBy": "UnrealPM Tests",
    "CanContainContent": true,
    "IsBetaVersion": false,
    "IsExperimentalVersion": false,
    "Installed": false,
    "Modules": [
        {{
            "Name": "{}",
            "Type": "Runtime",
            "LoadingPhase": "Default"
        }}
    ]
}}"#,
            self.version, self.name, self.name
        )
    }

    /// Create the plugin in a directory
    pub fn create_in(&self, dir: &Path) {
        let plugin_dir = dir.join(&self.name);
        fs::create_dir_all(&plugin_dir).expect("Failed to create plugin directory");

        // Create .uplugin
        fs::write(
            plugin_dir.join(format!("{}.uplugin", self.name)),
            self.uplugin_content(),
        )
        .expect("Failed to write .uplugin");

        // Create Source directory
        let source_dir = plugin_dir.join("Source").join(&self.name);
        fs::create_dir_all(&source_dir).expect("Failed to create source directory");

        // Create a minimal module file
        let module_cpp = format!(
            r#"#include "{}.h"
#include "Modules/ModuleManager.h"

IMPLEMENT_MODULE(FDefaultModuleImpl, {})
"#,
            self.name, self.name
        );
        fs::write(source_dir.join(format!("{}.cpp", self.name)), module_cpp)
            .expect("Failed to write module cpp");

        let module_h = r#"#pragma once

#include "CoreMinimal.h"
"#;
        fs::write(source_dir.join(format!("{}.h", self.name)), module_h)
            .expect("Failed to write module header");

        // Create Build.cs
        let build_cs = format!(
            r#"using UnrealBuildTool;

public class {} : ModuleRules
{{
    public {}(ReadOnlyTargetRules Target) : base(Target)
    {{
        PCHUsage = ModuleRules.PCHUsageMode.UseExplicitOrSharedPCHs;
        PublicDependencyModuleNames.AddRange(new string[] {{ "Core", "CoreUObject", "Engine" }});
    }}
}}
"#,
            self.name, self.name
        );
        fs::write(source_dir.join(format!("{}.Build.cs", self.name)), build_cs)
            .expect("Failed to write Build.cs");
    }
}

/// Helper to create a file-based test registry
pub struct TestRegistry {
    pub temp_dir: TempDir,
    pub packages_dir: PathBuf,
    pub tarballs_dir: PathBuf,
    pub signatures_dir: PathBuf,
}

impl TestRegistry {
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let root = temp_dir.path();

        let packages_dir = root.join("packages");
        let tarballs_dir = root.join("tarballs");
        let signatures_dir = root.join("signatures");

        fs::create_dir_all(&packages_dir).expect("Failed to create packages dir");
        fs::create_dir_all(&tarballs_dir).expect("Failed to create tarballs dir");
        fs::create_dir_all(&signatures_dir).expect("Failed to create signatures dir");

        Self {
            temp_dir,
            packages_dir,
            tarballs_dir,
            signatures_dir,
        }
    }

    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Add a package to the test registry
    pub fn add_package(&self, plugin: &MockPlugin) {
        let metadata = format!(
            r#"{{
    "name": "{}",
    "description": "Test package",
    "versions": [
        {{
            "version": "{}",
            "tarball": "{}-{}.tar.gz",
            "checksum": "0000000000000000000000000000000000000000000000000000000000000000",
            "engine_versions": {:?},
            "package_type": "source"
        }}
    ]
}}"#,
            plugin.name, plugin.version, plugin.name, plugin.version, plugin.engine_versions
        );

        fs::write(
            self.packages_dir.join(format!("{}.json", plugin.name)),
            metadata,
        )
        .expect("Failed to write package metadata");
    }
}

impl Default for TestRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Assertions for test results
pub mod assertions {
    use std::path::Path;

    /// Assert that a file contains a specific string
    pub fn file_contains(path: &Path, expected: &str) {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("Failed to read file: {:?}", path));
        assert!(
            content.contains(expected),
            "File {:?} should contain '{}', but content was:\n{}",
            path,
            expected,
            content
        );
    }

    /// Assert that a file does not contain a specific string
    pub fn file_not_contains(path: &Path, unexpected: &str) {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("Failed to read file: {:?}", path));
        assert!(
            !content.contains(unexpected),
            "File {:?} should not contain '{}', but content was:\n{}",
            path,
            unexpected,
            content
        );
    }

    /// Assert directory exists
    pub fn dir_exists(path: &Path) {
        assert!(
            path.exists() && path.is_dir(),
            "Directory should exist: {:?}",
            path
        );
    }

    /// Assert directory does not exist
    pub fn dir_not_exists(path: &Path) {
        assert!(!path.exists(), "Directory should not exist: {:?}", path);
    }

    /// Assert file exists
    pub fn file_exists(path: &Path) {
        assert!(
            path.exists() && path.is_file(),
            "File should exist: {:?}",
            path
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_creation() {
        let project = TestProject::new();
        assert!(project.path().exists());
        assert!(project.config_path().exists());
    }

    #[test]
    fn test_project_with_engine() {
        let project = TestProject::with_engine("5.4");
        let uproject = project.path().join("TestProject.uproject");
        assert!(uproject.exists());

        let content = fs::read_to_string(uproject).unwrap();
        assert!(content.contains("5.4"));
    }

    #[test]
    fn test_mock_plugin() {
        let plugin = MockPlugin::new("TestPlugin", "1.0.0")
            .with_engine_versions(vec!["5.3", "5.4"])
            .with_dependency("OtherPlugin", "^1.0.0");

        let temp_dir = TempDir::new().unwrap();
        plugin.create_in(temp_dir.path());

        assert!(temp_dir.path().join("TestPlugin").exists());
        assert!(temp_dir
            .path()
            .join("TestPlugin/TestPlugin.uplugin")
            .exists());
    }

    #[test]
    fn test_registry_creation() {
        let registry = TestRegistry::new();
        assert!(registry.packages_dir.exists());
        assert!(registry.tarballs_dir.exists());
        assert!(registry.signatures_dir.exists());
    }
}
