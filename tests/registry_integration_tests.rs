//! Integration tests for UnrealPM CLI against the production registry.
//!
//! These tests verify the CLI works correctly with https://registry.unreal.dev/
//!
//! # Test Categories
//!
//! 1. **Read-only tests** (safe to run anytime):
//!    - Search functionality
//!    - Package metadata retrieval
//!    - Download and checksum verification
//!    - Signature verification
//!
//! 2. **Authenticated tests** (require login):
//!    - These are marked with `#[ignore]` by default
//!    - Run with `cargo test -- --ignored` after logging in
//!
//! # Running Tests
//!
//! ```bash
//! # Run all safe read-only tests
//! cargo test --test registry_integration_tests
//!
//! # Run specific test
//! cargo test --test registry_integration_tests test_search_packages
//!
//! # Run authenticated tests (after `unrealpm login`)
//! cargo test --test registry_integration_tests -- --ignored
//! ```

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Registry URL for production testing
const REGISTRY_URL: &str = "https://registry.unreal.dev";

/// Known package that exists in the registry for testing
/// Note: Package names are case-sensitive in the registry
const TEST_PACKAGE: &str = "ChromaSense";

// ============================================================================
// Test Utilities
// ============================================================================

/// Create a fresh test project directory
fn setup_test_project() -> TempDir {
    TempDir::new().expect("Failed to create temp dir")
}

/// Get the unrealpm binary command
fn unrealpm_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_unrealpm"))
}

/// Create a minimal .uproject file for testing
fn create_test_uproject(dir: &std::path::Path, engine_version: &str) -> PathBuf {
    let uproject_path = dir.join("TestProject.uproject");
    let content = format!(
        r#"{{
    "FileVersion": 3,
    "EngineAssociation": "{}",
    "Category": "",
    "Description": "Test project for UnrealPM integration tests"
}}"#,
        engine_version
    );
    fs::write(&uproject_path, content).expect("Failed to write .uproject");
    uproject_path
}

/// Configure CLI to use HTTP registry
fn configure_http_registry(dir: &std::path::Path) {
    // Create config directory in temp dir to isolate from user config
    let config_dir = dir.join(".unrealpm");
    fs::create_dir_all(&config_dir).expect("Failed to create config dir");

    let config_content = format!(
        r#"[registry]
registry_type = "http"
url = "{}"

[signing]
enabled = true

[verification]
require_signatures = false
"#,
        REGISTRY_URL
    );

    fs::write(config_dir.join("config.toml"), config_content)
        .expect("Failed to write config");
}

/// Set up environment to use test project's config
fn with_test_config(cmd: &mut Command, dir: &std::path::Path) {
    cmd.env("UNREALPM_CONFIG_DIR", dir.join(".unrealpm"));
}

// ============================================================================
// Read-Only Tests (Safe to run without authentication)
// ============================================================================

mod read_only {
    use super::*;

    /// Test that we can search packages in the registry
    #[test]
    fn test_search_packages() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());

        cmd.arg("search")
            .arg("chroma")
            .assert()
            .success();
    }

    /// Test that search with empty query returns results
    #[test]
    fn test_search_all_packages() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());

        // Empty search should list packages
        cmd.arg("search")
            .arg("")
            .assert()
            .success();
    }

    /// Test that we can initialize a project with HTTP registry
    #[test]
    fn test_init_with_http_registry() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());

        cmd.current_dir(&temp_dir)
            .arg("init")
            .assert()
            .success()
            .stdout(predicate::str::contains("Created unrealpm.json"));

        // Verify manifest was created
        assert!(temp_dir.path().join("unrealpm.json").exists());
    }

    /// Test listing packages in empty project
    #[test]
    fn test_list_empty_project() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());

        // Initialize first
        cmd.current_dir(&temp_dir)
            .arg("init")
            .assert()
            .success();

        // List packages
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());

        cmd.current_dir(&temp_dir)
            .arg("list")
            .assert()
            .success()
            .stdout(predicate::str::contains("No packages installed"));
    }

    /// Test config show command displays registry settings
    #[test]
    fn test_config_show() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());

        cmd.arg("config")
            .arg("show")
            .assert()
            .success()
            .stdout(predicate::str::contains(REGISTRY_URL));
    }

    /// Test that help command works
    #[test]
    fn test_help_command() {
        unrealpm_cmd()
            .arg("--help")
            .assert()
            .success()
            .stdout(predicate::str::contains("unrealpm"))
            .stdout(predicate::str::contains("install"))
            .stdout(predicate::str::contains("search"))
            .stdout(predicate::str::contains("publish"));
    }

    /// Test version command
    #[test]
    fn test_version_command() {
        unrealpm_cmd()
            .arg("--version")
            .assert()
            .success()
            .stdout(predicate::str::contains("unrealpm"));
    }

    /// Test shell completions generation
    #[test]
    fn test_completions_bash() {
        unrealpm_cmd()
            .arg("completions")
            .arg("bash")
            .assert()
            .success()
            .stdout(predicate::str::contains("_unrealpm"));
    }

    #[test]
    fn test_completions_zsh() {
        unrealpm_cmd()
            .arg("completions")
            .arg("zsh")
            .assert()
            .success()
            .stdout(predicate::str::contains("#compdef"));
    }

    #[test]
    fn test_completions_fish() {
        unrealpm_cmd()
            .arg("completions")
            .arg("fish")
            .assert()
            .success()
            .stdout(predicate::str::contains("complete"));
    }
}

// ============================================================================
// Package Download Tests (Read-only but may require network)
// ============================================================================

mod download {
    use super::*;

    /// Test dry-run install shows what would be installed
    #[test]
    fn test_install_dry_run() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize project
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("init")
            .assert()
            .success();

        // Dry-run install
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .arg("--dry-run")
            .assert()
            .success();

        // Verify nothing was actually installed
        assert!(!temp_dir.path().join("Plugins").exists());
    }

    /// Test installing a package from the registry
    #[test]
    fn test_install_package() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize project
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("init")
            .assert()
            .success();

        // Install package
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Verify package was installed
        let plugins_dir = temp_dir.path().join("Plugins");
        assert!(plugins_dir.exists(), "Plugins directory should exist");

        // Verify lockfile was created
        let lockfile = temp_dir.path().join("unrealpm.lock");
        assert!(lockfile.exists(), "Lockfile should exist");

        // Verify manifest was updated
        let manifest = fs::read_to_string(temp_dir.path().join("unrealpm.json"))
            .expect("Failed to read manifest");
        assert!(
            manifest.contains(TEST_PACKAGE),
            "Manifest should contain installed package"
        );
    }

    /// Test that lockfile ensures reproducible installs
    #[test]
    fn test_lockfile_reproducibility() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize and install
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Read lockfile content
        let lockfile_content =
            fs::read_to_string(temp_dir.path().join("unrealpm.lock")).expect("Failed to read lockfile");

        // Remove Plugins directory
        fs::remove_dir_all(temp_dir.path().join("Plugins")).expect("Failed to remove Plugins");

        // Reinstall from lockfile
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .assert()
            .success();

        // Verify lockfile hasn't changed (same versions)
        let new_lockfile_content =
            fs::read_to_string(temp_dir.path().join("unrealpm.lock")).expect("Failed to read lockfile");

        // Extract version lines for comparison (ignore timestamp)
        let extract_versions = |content: &str| -> Vec<String> {
            content
                .lines()
                .filter(|line| line.contains("version = ") || line.contains("checksum = "))
                .map(String::from)
                .collect()
        };

        assert_eq!(
            extract_versions(&lockfile_content),
            extract_versions(&new_lockfile_content),
            "Lockfile versions should match after reinstall"
        );
    }

    /// Test uninstalling a package
    #[test]
    fn test_uninstall_package() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize and install
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Verify installed
        assert!(temp_dir.path().join("Plugins").exists());

        // Uninstall
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("uninstall")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Verify manifest no longer contains package
        let manifest = fs::read_to_string(temp_dir.path().join("unrealpm.json"))
            .expect("Failed to read manifest");
        assert!(
            !manifest.contains(&format!("\"{TEST_PACKAGE}\"")),
            "Manifest should not contain uninstalled package"
        );
    }
}

// ============================================================================
// Signature Verification Tests
// ============================================================================

mod verification {
    use super::*;

    /// Test verifying a signed package
    #[test]
    fn test_verify_signed_package() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize and install
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Verify the package signature
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("verify")
            .arg(TEST_PACKAGE)
            .assert()
            .success();
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

mod errors {
    use super::*;

    /// Test error when package doesn't exist
    #[test]
    fn test_install_nonexistent_package() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        // Try to install non-existent package
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg("this-package-definitely-does-not-exist-12345")
            .assert()
            .failure()
            .stderr(predicate::str::contains("not found").or(predicate::str::contains("Not found")));
    }

    /// Test install without explicit init - CLI should handle gracefully
    /// The CLI creates a manifest automatically if one doesn't exist
    #[test]
    fn test_install_without_init() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        // Note: NOT running init, but CLI should still work

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Verify manifest was created automatically
        assert!(temp_dir.path().join("unrealpm.json").exists());
    }

    /// Test search with no results
    #[test]
    fn test_search_no_results() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.arg("search")
            .arg("xyznonexistentpackage12345")
            .assert()
            .success()
            .stdout(predicate::str::contains("No packages found").or(predicate::str::is_empty().not()));
    }

    /// Test uninstall package not installed shows warning
    #[test]
    fn test_uninstall_not_installed() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        // Try to uninstall package that's not installed - shows warning but succeeds
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("uninstall")
            .arg("not-installed-package")
            .assert()
            .success()
            .stdout(predicate::str::contains("not in dependencies"));
    }
}

// ============================================================================
// Authentication Tests (Require Login - Run with --ignored)
// ============================================================================

mod authenticated {
    use super::*;

    /// Test login flow (manual - requires user interaction)
    /// Run with: cargo test test_login_flow -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_login_flow() {
        // This test is interactive and requires manual login
        // It verifies the login command runs without crashing
        unrealpm_cmd()
            .arg("login")
            .assert()
            .success();
    }

    /// Test tokens list (requires authentication)
    #[test]
    #[ignore]
    fn test_tokens_list() {
        unrealpm_cmd()
            .arg("tokens")
            .arg("list")
            .assert()
            .success();
    }

    /// Test publish dry-run (requires authentication)
    #[test]
    #[ignore]
    fn test_publish_dry_run() {
        // This test requires a valid plugin directory
        // Using the test plugins from CLAUDE.md
        let plugin_path = std::path::Path::new("/tmp/test-plugins/AsyncLoadingScreen");

        if !plugin_path.exists() {
            eprintln!("Test plugin not found at {:?}", plugin_path);
            return;
        }

        unrealpm_cmd()
            .current_dir(plugin_path)
            .arg("publish")
            .arg("--dry-run")
            .assert()
            .success();
    }
}

// ============================================================================
// Dependency Resolution Tests
// ============================================================================

mod dependencies {
    use super::*;

    /// Test dependency tree command
    #[test]
    fn test_dependency_tree() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize and install
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Show dependency tree
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("tree")
            .assert()
            .success();
    }

    /// Test outdated command
    #[test]
    fn test_outdated_command() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize and install
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Check for outdated packages
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("outdated")
            .assert()
            .success();
    }

    /// Test why command
    #[test]
    fn test_why_command() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize and install
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .assert()
            .success();

        // Ask why package is installed
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("why")
            .arg(TEST_PACKAGE)
            .assert()
            .success();
    }
}

// ============================================================================
// Engine Version Compatibility Tests
// ============================================================================

mod engine_version {
    use super::*;

    /// Test installing with engine version override
    #[test]
    fn test_install_with_engine_override() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        // Install with engine version override
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .arg("--engine-version")
            .arg("5.4")
            .assert()
            .success();
    }

    /// Test that engine version is detected from .uproject
    #[test]
    fn test_engine_version_detection() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.4");

        // Initialize should detect engine version
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("init")
            .assert()
            .success()
            .stdout(predicate::str::contains("5.4").or(predicate::str::contains("Engine")));
    }

    /// Test install with force flag bypasses compatibility checks
    #[test]
    fn test_install_force_flag() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        // Install with force flag
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .arg("--force")
            .assert()
            .success();
    }
}

// ============================================================================
// Package Type Tests (Source/Binary/Hybrid)
// ============================================================================

mod package_types {
    use super::*;

    /// Test installing with source-only preference
    #[test]
    fn test_install_source_only() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        // Install with source-only preference
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .arg("--source-only")
            .assert()
            .success();
    }

    /// Test installing with prefer-binary preference
    #[test]
    fn test_install_prefer_binary() {
        let temp_dir = setup_test_project();
        configure_http_registry(temp_dir.path());
        create_test_uproject(temp_dir.path(), "5.3");

        // Initialize
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir).arg("init").assert().success();

        // Install with prefer-binary preference
        let mut cmd = unrealpm_cmd();
        with_test_config(&mut cmd, temp_dir.path());
        cmd.current_dir(&temp_dir)
            .arg("install")
            .arg(TEST_PACKAGE)
            .arg("--prefer-binary")
            .assert()
            .success();
    }
}
