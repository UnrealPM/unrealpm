use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper to create a test project directory
fn setup_test_project() -> TempDir {
    TempDir::new().expect("Failed to create temp dir")
}

/// Helper to get the binary command
fn unrealpm_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_unrealpm"))
}

#[test]
fn test_init_command() {
    let temp_dir = setup_test_project();

    // Run init command
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Created unrealpm.json"));

    // Verify unrealpm.json was created
    let manifest_path = temp_dir.path().join("unrealpm.json");
    assert!(manifest_path.exists(), "unrealpm.json should be created");
}

#[test]
fn test_search_command() {
    unrealpm_cmd()
        .arg("search")
        .arg("plugin")
        .assert()
        .success()
        .stdout(predicate::str::contains("awesome-plugin"));
}

#[test]
fn test_list_empty() {
    let temp_dir = setup_test_project();

    // Initialize project
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("init")
        .assert()
        .success();

    // List packages (should be empty)
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No packages installed"));
}

#[test]
fn test_install_single_package() {
    let temp_dir = setup_test_project();

    // Initialize project
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("init")
        .assert()
        .success();

    // Install base-utils (no dependencies)
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("install")
        .arg("base-utils@^1.0.0")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Successfully installed base-utils",
        ));

    // Verify plugin was installed
    let plugin_path = temp_dir.path().join("Plugins/base-utils");
    assert!(plugin_path.exists(), "Plugin should be installed");

    // Verify lockfile was created
    let lockfile_path = temp_dir.path().join("unrealpm.lock");
    assert!(lockfile_path.exists(), "Lockfile should be created");

    // Verify lockfile contains the package
    let lockfile_content = fs::read_to_string(lockfile_path).unwrap();
    assert!(lockfile_content.contains("base-utils"));
    assert!(lockfile_content.contains("1.0.0"));
}

#[test]
fn test_install_with_transitive_dependencies() {
    let temp_dir = setup_test_project();

    // Initialize project
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("init")
        .assert()
        .success();

    // Install multiplayer-toolkit (has transitive dependencies)
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("install")
        .arg("multiplayer-toolkit@^2.0.0")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Successfully installed multiplayer-toolkit",
        ));

    // Run install to get all transitive dependencies
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("install")
        .assert()
        .success()
        .stdout(predicate::str::contains("Finished installing dependencies"));

    // Verify all three packages were installed (multiplayer-toolkit + its transitive deps)
    assert!(temp_dir.path().join("Plugins/multiplayer-toolkit").exists());
    assert!(temp_dir.path().join("Plugins/awesome-plugin").exists());
    assert!(temp_dir.path().join("Plugins/base-utils").exists());

    // Verify lockfile contains all three packages
    let lockfile_path = temp_dir.path().join("unrealpm.lock");
    let lockfile_content = fs::read_to_string(lockfile_path).unwrap();
    assert!(lockfile_content.contains("multiplayer-toolkit"));
    assert!(lockfile_content.contains("awesome-plugin"));
    assert!(lockfile_content.contains("base-utils"));
}

#[test]
fn test_uninstall_command() {
    let temp_dir = setup_test_project();

    // Initialize and install a package
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("init")
        .assert()
        .success();

    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("install")
        .arg("base-utils@^1.0.0")
        .assert()
        .success();

    // Verify it's installed
    let plugin_path = temp_dir.path().join("Plugins/base-utils");
    assert!(plugin_path.exists());

    // Uninstall it
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("uninstall")
        .arg("base-utils")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Successfully uninstalled base-utils",
        ));

    // Verify it was removed
    assert!(!plugin_path.exists(), "Plugin should be removed");
}

#[test]
fn test_lockfile_reproducibility() {
    let temp_dir = setup_test_project();

    // Initialize project
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("init")
        .assert()
        .success();

    // Install a package
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("install")
        .arg("base-utils@^1.0.0")
        .assert()
        .success();

    // Read lockfile
    let lockfile_path = temp_dir.path().join("unrealpm.lock");
    let _lockfile_content = fs::read_to_string(&lockfile_path).unwrap();

    // Remove the installed package
    fs::remove_dir_all(temp_dir.path().join("Plugins")).unwrap();

    // Reinstall from lockfile (should use exact versions)
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("install")
        .assert()
        .success();

    // Verify lockfile hasn't changed (except timestamp)
    let new_lockfile_content = fs::read_to_string(&lockfile_path).unwrap();

    // Extract version and checksum (ignore timestamp)
    assert!(new_lockfile_content.contains("version = \"1.0.0\""));
    assert!(new_lockfile_content.contains(
        "checksum = \"00adf0997d0926e6965a852b834fe144abddb8e54ebc47cd540abe639e966241\""
    ));
}

#[test]
fn test_checksum_verification() {
    let temp_dir = setup_test_project();

    // Initialize project
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("init")
        .assert()
        .success();

    // Install package (will verify checksum automatically)
    unrealpm_cmd()
        .current_dir(&temp_dir)
        .arg("install")
        .arg("base-utils@^1.0.0")
        .assert()
        .success();

    // If we got here, checksum verification passed
    // (otherwise the install would have failed)
    assert!(temp_dir.path().join("Plugins/base-utils").exists());
}
