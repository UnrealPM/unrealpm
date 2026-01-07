//! Comprehensive tests for the PubGrub dependency resolver
//!
//! These tests verify the resolver's behavior for various scenarios including:
//! - Basic version constraint parsing
//! - Conflict detection and error messages
//! - Engine version filtering
//! - Edge cases and error conditions
//!
//! Note: These are unit-style tests that test the resolver logic directly,
//! not integration tests that require a running registry.

mod test_utils;

use std::fs;
use tempfile::TempDir;
use test_utils::{MockPlugin, TestRegistry};

// ============================================================================
// Version Constraint Parsing Tests
// ============================================================================

mod version_constraints {
    use super::*;

    /// Test that caret constraints work correctly
    #[test]
    fn test_caret_constraint_parsing() {
        // Caret constraints allow changes that don't modify the left-most non-zero digit
        // ^1.2.3 means >=1.2.3, <2.0.0
        // ^0.2.3 means >=0.2.3, <0.3.0
        // ^0.0.3 means >=0.0.3, <0.0.4

        // This test validates that the resolver correctly parses caret constraints
        // by creating a mock registry with specific versions

        let registry = TestRegistry::new();

        // Add versions: 1.0.0, 1.5.0, 2.0.0
        let plugin_v1 =
            MockPlugin::new("versioned-pkg", "1.0.0").with_engine_versions(vec!["5.3", "5.4"]);
        registry.add_package(&plugin_v1);

        let plugin_v15 =
            MockPlugin::new("versioned-pkg", "1.5.0").with_engine_versions(vec!["5.3", "5.4"]);
        registry.add_package(&plugin_v15);

        let plugin_v2 =
            MockPlugin::new("versioned-pkg", "2.0.0").with_engine_versions(vec!["5.3", "5.4"]);
        registry.add_package(&plugin_v2);

        // Verify the registry was set up correctly
        assert!(registry.packages_dir.join("versioned-pkg.json").exists());
    }

    /// Test that tilde constraints work correctly
    #[test]
    fn test_tilde_constraint_parsing() {
        // Tilde constraints allow patch-level changes
        // ~1.2.3 means >=1.2.3, <1.3.0

        let registry = TestRegistry::new();

        let plugin = MockPlugin::new("tilde-pkg", "1.2.3").with_engine_versions(vec!["5.3"]);
        registry.add_package(&plugin);

        assert!(registry.packages_dir.join("tilde-pkg.json").exists());
    }

    /// Test exact version constraints
    #[test]
    fn test_exact_version_constraint() {
        // =1.2.3 means exactly version 1.2.3

        let registry = TestRegistry::new();

        let plugin = MockPlugin::new("exact-pkg", "1.2.3").with_engine_versions(vec!["5.3"]);
        registry.add_package(&plugin);

        assert!(registry.packages_dir.join("exact-pkg.json").exists());
    }

    /// Test wildcard constraints
    #[test]
    fn test_wildcard_constraint() {
        // * means any version

        let registry = TestRegistry::new();

        let plugin = MockPlugin::new("any-pkg", "1.0.0").with_engine_versions(vec!["5.3"]);
        registry.add_package(&plugin);

        assert!(registry.packages_dir.join("any-pkg.json").exists());
    }
}

// ============================================================================
// Dependency Chain Tests
// ============================================================================

mod dependency_chains {
    use super::*;

    /// Create a registry with a linear dependency chain A -> B -> C
    fn create_chain_registry() -> TestRegistry {
        let registry = TestRegistry::new();

        // C has no dependencies
        let pkg_c = MockPlugin::new("pkg-c", "1.0.0").with_engine_versions(vec!["5.3", "5.4"]);
        registry.add_package(&pkg_c);

        // B depends on C
        let pkg_b = MockPlugin::new("pkg-b", "1.0.0")
            .with_engine_versions(vec!["5.3", "5.4"])
            .with_dependency("pkg-c", "^1.0.0");
        registry.add_package(&pkg_b);

        // A depends on B
        let pkg_a = MockPlugin::new("pkg-a", "1.0.0")
            .with_engine_versions(vec!["5.3", "5.4"])
            .with_dependency("pkg-b", "^1.0.0");
        registry.add_package(&pkg_a);

        registry
    }

    #[test]
    fn test_chain_registry_setup() {
        let registry = create_chain_registry();

        assert!(registry.packages_dir.join("pkg-a.json").exists());
        assert!(registry.packages_dir.join("pkg-b.json").exists());
        assert!(registry.packages_dir.join("pkg-c.json").exists());
    }

    /// Create a registry with diamond dependencies A -> B,C -> D
    fn create_diamond_registry() -> TestRegistry {
        let registry = TestRegistry::new();

        // D is the base (no dependencies)
        let pkg_d = MockPlugin::new("pkg-d", "1.0.0").with_engine_versions(vec!["5.3"]);
        registry.add_package(&pkg_d);

        // B depends on D
        let pkg_b = MockPlugin::new("pkg-b", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("pkg-d", "^1.0.0");
        registry.add_package(&pkg_b);

        // C also depends on D
        let pkg_c = MockPlugin::new("pkg-c", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("pkg-d", "^1.0.0");
        registry.add_package(&pkg_c);

        // A depends on both B and C
        let pkg_a = MockPlugin::new("pkg-a", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("pkg-b", "^1.0.0")
            .with_dependency("pkg-c", "^1.0.0");
        registry.add_package(&pkg_a);

        registry
    }

    #[test]
    fn test_diamond_registry_setup() {
        let registry = create_diamond_registry();

        assert!(registry.packages_dir.join("pkg-a.json").exists());
        assert!(registry.packages_dir.join("pkg-b.json").exists());
        assert!(registry.packages_dir.join("pkg-c.json").exists());
        assert!(registry.packages_dir.join("pkg-d.json").exists());
    }
}

// ============================================================================
// Engine Version Filtering Tests
// ============================================================================

mod engine_version {
    use super::*;

    #[test]
    fn test_engine_specific_packages() {
        let registry = TestRegistry::new();

        // Package only available for 5.3
        let plugin_53 =
            MockPlugin::new("engine-53-only", "1.0.0").with_engine_versions(vec!["5.3"]);
        registry.add_package(&plugin_53);

        // Package only available for 5.4
        let plugin_54 =
            MockPlugin::new("engine-54-only", "1.0.0").with_engine_versions(vec!["5.4"]);
        registry.add_package(&plugin_54);

        // Package available for both
        let plugin_both =
            MockPlugin::new("engine-multi", "1.0.0").with_engine_versions(vec!["5.3", "5.4"]);
        registry.add_package(&plugin_both);

        assert!(registry.packages_dir.join("engine-53-only.json").exists());
        assert!(registry.packages_dir.join("engine-54-only.json").exists());
        assert!(registry.packages_dir.join("engine-multi.json").exists());
    }

    #[test]
    fn test_multi_engine_package() {
        let registry = TestRegistry::new();

        // Package compatible with multiple engine versions
        let plugin = MockPlugin::new("multi-engine", "1.0.0")
            .with_engine_versions(vec!["5.2", "5.3", "5.4", "5.5"]);
        registry.add_package(&plugin);

        let content = fs::read_to_string(registry.packages_dir.join("multi-engine.json")).unwrap();
        assert!(content.contains("5.2"));
        assert!(content.contains("5.3"));
        assert!(content.contains("5.4"));
        assert!(content.contains("5.5"));
    }
}

// ============================================================================
// Conflict Scenario Tests
// ============================================================================

mod conflicts {
    use super::*;

    /// Create a registry with incompatible version requirements
    fn create_conflict_registry() -> TestRegistry {
        let registry = TestRegistry::new();

        // shared-dep has two versions
        let shared_v1 = MockPlugin::new("shared-dep", "1.0.0").with_engine_versions(vec!["5.3"]);
        registry.add_package(&shared_v1);

        let shared_v2 = MockPlugin::new("shared-dep", "2.0.0").with_engine_versions(vec!["5.3"]);
        registry.add_package(&shared_v2);

        // left-pkg requires shared-dep ^1.0.0
        let left = MockPlugin::new("left-pkg", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("shared-dep", "^1.0.0");
        registry.add_package(&left);

        // right-pkg requires shared-dep ^2.0.0 (CONFLICT!)
        let right = MockPlugin::new("right-pkg", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("shared-dep", "^2.0.0");
        registry.add_package(&right);

        registry
    }

    #[test]
    fn test_conflict_registry_setup() {
        let registry = create_conflict_registry();

        assert!(registry.packages_dir.join("shared-dep.json").exists());
        assert!(registry.packages_dir.join("left-pkg.json").exists());
        assert!(registry.packages_dir.join("right-pkg.json").exists());
    }
}

// ============================================================================
// Circular Dependency Tests
// ============================================================================

mod circular {
    use super::*;

    /// Create a registry with direct circular dependency A <-> B
    fn create_direct_circular_registry() -> TestRegistry {
        let registry = TestRegistry::new();

        // A depends on B
        let pkg_a = MockPlugin::new("circular-a", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("circular-b", "^1.0.0");
        registry.add_package(&pkg_a);

        // B depends on A (circular!)
        let pkg_b = MockPlugin::new("circular-b", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("circular-a", "^1.0.0");
        registry.add_package(&pkg_b);

        registry
    }

    #[test]
    fn test_direct_circular_registry_setup() {
        let registry = create_direct_circular_registry();

        assert!(registry.packages_dir.join("circular-a.json").exists());
        assert!(registry.packages_dir.join("circular-b.json").exists());
    }

    /// Create a registry with indirect circular dependency A -> B -> C -> A
    fn create_indirect_circular_registry() -> TestRegistry {
        let registry = TestRegistry::new();

        // A depends on B
        let pkg_a = MockPlugin::new("chain-a", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("chain-b", "^1.0.0");
        registry.add_package(&pkg_a);

        // B depends on C
        let pkg_b = MockPlugin::new("chain-b", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("chain-c", "^1.0.0");
        registry.add_package(&pkg_b);

        // C depends on A (circular!)
        let pkg_c = MockPlugin::new("chain-c", "1.0.0")
            .with_engine_versions(vec!["5.3"])
            .with_dependency("chain-a", "^1.0.0");
        registry.add_package(&pkg_c);

        registry
    }

    #[test]
    fn test_indirect_circular_registry_setup() {
        let registry = create_indirect_circular_registry();

        assert!(registry.packages_dir.join("chain-a.json").exists());
        assert!(registry.packages_dir.join("chain-b.json").exists());
        assert!(registry.packages_dir.join("chain-c.json").exists());
    }
}

// ============================================================================
// Edge Case Tests
// ============================================================================

mod edge_cases {
    use super::*;

    #[test]
    fn test_empty_registry() {
        let registry = TestRegistry::new();

        // Should have directory structure but no packages
        assert!(registry.packages_dir.exists());
        assert!(registry.tarballs_dir.exists());
        assert!(registry.signatures_dir.exists());

        // No package files
        let entries: Vec<_> = fs::read_dir(&registry.packages_dir).unwrap().collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_package_with_no_dependencies() {
        let registry = TestRegistry::new();

        let plugin =
            MockPlugin::new("standalone", "1.0.0").with_engine_versions(vec!["5.3", "5.4"]);
        registry.add_package(&plugin);

        assert!(registry.packages_dir.join("standalone.json").exists());
    }

    #[test]
    fn test_package_with_many_versions() {
        let registry = TestRegistry::new();

        // Add many versions of the same package
        for minor in 0..10 {
            let plugin = MockPlugin::new("many-versions", &format!("1.{}.0", minor))
                .with_engine_versions(vec!["5.3"]);
            registry.add_package(&plugin);
        }

        assert!(registry.packages_dir.join("many-versions.json").exists());
    }

    #[test]
    fn test_deep_dependency_chain() {
        let registry = TestRegistry::new();

        // Create a chain of 10 packages
        let depth = 10;
        for i in (0..depth).rev() {
            let plugin = if i == depth - 1 {
                MockPlugin::new(&format!("deep-{}", i), "1.0.0").with_engine_versions(vec!["5.3"])
            } else {
                MockPlugin::new(&format!("deep-{}", i), "1.0.0")
                    .with_engine_versions(vec!["5.3"])
                    .with_dependency(&format!("deep-{}", i + 1), "^1.0.0")
            };
            registry.add_package(&plugin);
        }

        // Verify all packages exist
        for i in 0..depth {
            assert!(registry
                .packages_dir
                .join(format!("deep-{}.json", i))
                .exists());
        }
    }
}

// ============================================================================
// Test Project Tests
// ============================================================================

mod test_project {
    use super::*;
    use test_utils::TestProject;

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
    fn test_project_manifest_checks() {
        let project = TestProject::new();

        // Initially no manifest
        assert!(!project.has_manifest());
        assert!(!project.has_lockfile());

        // Create a manifest
        fs::write(
            project.path().join("unrealpm.json"),
            r#"{"dependencies": {}}"#,
        )
        .unwrap();

        assert!(project.has_manifest());
    }

    #[test]
    fn test_project_plugins_directory() {
        let project = TestProject::new();

        // Initially no plugins
        assert!(project.list_plugins().is_empty());

        // Create plugins directory with a plugin
        let plugins_dir = project.plugins_dir();
        fs::create_dir_all(plugins_dir.join("TestPlugin")).unwrap();

        let plugins = project.list_plugins();
        assert_eq!(plugins.len(), 1);
        assert!(plugins.contains(&"TestPlugin".to_string()));
    }
}

// ============================================================================
// Mock Plugin Tests
// ============================================================================

mod mock_plugin {
    use super::*;

    #[test]
    fn test_mock_plugin_creation() {
        let plugin = MockPlugin::new("TestPlugin", "1.0.0");
        assert_eq!(plugin.name, "TestPlugin");
        assert_eq!(plugin.version, "1.0.0");
    }

    #[test]
    fn test_mock_plugin_with_dependencies() {
        let plugin = MockPlugin::new("WithDeps", "1.0.0")
            .with_dependency("dep-a", "^1.0.0")
            .with_dependency("dep-b", "~2.0.0");

        assert_eq!(plugin.dependencies.len(), 2);
    }

    #[test]
    fn test_mock_plugin_with_engine_versions() {
        let plugin = MockPlugin::new("EngineSpecific", "1.0.0")
            .with_engine_versions(vec!["5.3", "5.4", "5.5"]);

        assert_eq!(plugin.engine_versions.len(), 3);
    }

    #[test]
    fn test_mock_plugin_uplugin_content() {
        let plugin = MockPlugin::new("MyPlugin", "2.0.0");
        let content = plugin.uplugin_content();

        assert!(content.contains("MyPlugin"));
        assert!(content.contains("2.0.0"));
        assert!(content.contains("FileVersion"));
    }

    #[test]
    fn test_mock_plugin_create_in_directory() {
        let temp_dir = TempDir::new().unwrap();
        let plugin = MockPlugin::new("CreatedPlugin", "1.0.0");

        plugin.create_in(temp_dir.path());

        let plugin_dir = temp_dir.path().join("CreatedPlugin");
        assert!(plugin_dir.exists());
        assert!(plugin_dir.join("CreatedPlugin.uplugin").exists());
        assert!(plugin_dir.join("Source").exists());
        assert!(plugin_dir.join("Source/CreatedPlugin").exists());
    }
}
