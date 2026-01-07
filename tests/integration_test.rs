use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Registry URL for production testing
const REGISTRY_URL: &str = "https://registry.unreal.dev";

/// Helper to create a test project directory
fn setup_test_project() -> TempDir {
    TempDir::new().expect("Failed to create temp dir")
}

/// Helper to get the binary command
fn unrealpm_cmd() -> Command {
    Command::new(env!("CARGO_BIN_EXE_unrealpm"))
}

/// Configure CLI to use HTTP registry
fn configure_http_registry(dir: &std::path::Path) {
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
strict_verification = false
"#,
        REGISTRY_URL
    );

    fs::write(config_dir.join("config.toml"), config_content).expect("Failed to write config");
}

/// Set up environment to use test project's config
fn with_test_config(cmd: &mut Command, dir: &std::path::Path) {
    cmd.env("UNREALPM_CONFIG_DIR", dir.join(".unrealpm"));
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
    let temp_dir = setup_test_project();
    configure_http_registry(temp_dir.path());

    // Search for packages on the HTTP registry
    // Using lowercase to match case-insensitive search
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.arg("search").arg("chroma").assert().success();
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
    configure_http_registry(temp_dir.path());

    // Create a .uproject to detect engine version
    fs::write(
        temp_dir.path().join("TestProject.uproject"),
        r#"{"FileVersion": 3, "EngineAssociation": "5.4"}"#,
    )
    .unwrap();

    // Initialize project
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir).arg("init").assert().success();

    // Install ChromaSense (a real package on the registry)
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir)
        .arg("install")
        .arg("ChromaSense@^0.1.0")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Successfully installed ChromaSense",
        ));

    // Verify plugin was installed
    let plugin_path = temp_dir.path().join("Plugins/ChromaSense");
    assert!(plugin_path.exists(), "Plugin should be installed");

    // Verify lockfile was created
    let lockfile_path = temp_dir.path().join("unrealpm.lock");
    assert!(lockfile_path.exists(), "Lockfile should be created");

    // Verify lockfile contains the package
    let lockfile_content = fs::read_to_string(lockfile_path).unwrap();
    assert!(lockfile_content.contains("ChromaSense"));
}

/// Test transitive dependency installation
///
/// Tests installing a package with transitive dependencies.
/// DebugTools@1.2.0 depends on UnrealUtilsCore@*, so installing DebugTools
/// should automatically pull in UnrealUtilsCore as well.
#[test]
fn test_install_with_transitive_dependencies() {
    let temp_dir = setup_test_project();
    configure_http_registry(temp_dir.path());

    // Create a .uproject to detect engine version
    fs::write(
        temp_dir.path().join("TestProject.uproject"),
        r#"{"FileVersion": 3, "EngineAssociation": "5.4"}"#,
    )
    .unwrap();

    // Initialize project
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir).arg("init").assert().success();

    // Install DebugTools which has a dependency on UnrealUtilsCore
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir)
        .arg("install")
        .arg("DebugTools@^1.2.0")
        .assert()
        .success();

    // Verify both packages were installed (DebugTools and its dependency UnrealUtilsCore)
    assert!(
        temp_dir.path().join("Plugins/DebugTools").exists(),
        "DebugTools should be installed"
    );
    assert!(
        temp_dir.path().join("Plugins/UnrealUtilsCore").exists(),
        "UnrealUtilsCore should be installed as a transitive dependency"
    );

    // Verify lockfile contains both packages
    let lockfile_content =
        fs::read_to_string(temp_dir.path().join("unrealpm.lock")).expect("lockfile should exist");
    assert!(
        lockfile_content.contains("DebugTools"),
        "lockfile should contain DebugTools"
    );
    assert!(
        lockfile_content.contains("UnrealUtilsCore"),
        "lockfile should contain UnrealUtilsCore"
    );
}

#[test]
fn test_uninstall_command() {
    let temp_dir = setup_test_project();
    configure_http_registry(temp_dir.path());

    // Create a .uproject to detect engine version
    fs::write(
        temp_dir.path().join("TestProject.uproject"),
        r#"{"FileVersion": 3, "EngineAssociation": "5.4"}"#,
    )
    .unwrap();

    // Initialize and install a package
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir).arg("init").assert().success();

    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir)
        .arg("install")
        .arg("ChromaSense@^0.1.0")
        .assert()
        .success();

    // Verify it's installed
    let plugin_path = temp_dir.path().join("Plugins/ChromaSense");
    assert!(plugin_path.exists());

    // Uninstall it
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir)
        .arg("uninstall")
        .arg("ChromaSense")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Successfully uninstalled ChromaSense",
        ));

    // Verify it was removed
    assert!(!plugin_path.exists(), "Plugin should be removed");
}

#[test]
fn test_lockfile_reproducibility() {
    let temp_dir = setup_test_project();
    configure_http_registry(temp_dir.path());

    // Create a .uproject to detect engine version
    fs::write(
        temp_dir.path().join("TestProject.uproject"),
        r#"{"FileVersion": 3, "EngineAssociation": "5.4"}"#,
    )
    .unwrap();

    // Initialize project
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir).arg("init").assert().success();

    // Install a package
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir)
        .arg("install")
        .arg("ChromaSense@^0.1.0")
        .assert()
        .success();

    // Read lockfile
    let lockfile_path = temp_dir.path().join("unrealpm.lock");
    let lockfile_content = fs::read_to_string(&lockfile_path).unwrap();

    // Verify lockfile has the package
    assert!(lockfile_content.contains("ChromaSense"));
    assert!(lockfile_content.contains("checksum"));

    // Remove the installed package
    fs::remove_dir_all(temp_dir.path().join("Plugins")).unwrap();

    // Reinstall from lockfile (should use exact versions)
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir).arg("install").assert().success();

    // Verify lockfile still contains the package
    let new_lockfile_content = fs::read_to_string(&lockfile_path).unwrap();
    assert!(new_lockfile_content.contains("ChromaSense"));
}

#[test]
fn test_checksum_verification() {
    let temp_dir = setup_test_project();
    configure_http_registry(temp_dir.path());

    // Create a .uproject to detect engine version
    fs::write(
        temp_dir.path().join("TestProject.uproject"),
        r#"{"FileVersion": 3, "EngineAssociation": "5.4"}"#,
    )
    .unwrap();

    // Initialize project
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir).arg("init").assert().success();

    // Install package (will verify checksum automatically)
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());
    cmd.current_dir(&temp_dir)
        .arg("install")
        .arg("ChromaSense@^0.1.0")
        .assert()
        .success();

    // If we got here, checksum verification passed
    // (otherwise the install would have failed)
    assert!(temp_dir.path().join("Plugins/ChromaSense").exists());
}
