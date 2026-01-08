//! Package installation and checksum verification
//!
//! This module handles extracting package tarballs to the Plugins/ directory
//! and verifying checksums. MVP uses simple file copying; Phase 2 will implement
//! content-addressable storage (CAS) for deduplication.
//!
//! # Examples
//!
//! ```no_run
//! use unrealpm::{install_package, verify_checksum};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Verify checksum before installing
//! verify_checksum("package.tar.gz", "sha256:abc123...", None)?;
//!
//! // Install package with progress callback
//! let installed_path = install_package("package.tar.gz", ".", "my-plugin", None)?;
//! println!("Installed to: {:?}", installed_path);
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tar::Archive;

/// Progress callback for installation/verification operations
///
/// Called with:
/// - `message`: Description of current operation (e.g., "Extracting package...")
/// - `current`: Current progress (0-100 for percentage, or bytes processed)
/// - `total`: Total work (100 for percentage, or total bytes)
pub type ProgressCallback = Arc<dyn Fn(&str, u64, u64) + Send + Sync>;

/// Install a package from a tarball to the target directory
///
/// Extracts the package tarball to `{target_dir}/Plugins/{package_name}/`.
/// Creates the Plugins directory if it doesn't exist.
///
/// The installer handles tarballs where the root folder name doesn't match
/// the package name by detecting the `.uplugin` file and renaming the folder.
///
/// # Arguments
///
/// * `tarball_path` - Path to the .tar.gz package file
/// * `target_dir` - Project root directory
/// * `package_name` - Name of the package being installed
/// * `progress` - Optional callback for progress updates
///
/// # Returns
///
/// The path where the package was installed
pub fn install_package<P: AsRef<Path>>(
    tarball_path: P,
    target_dir: P,
    package_name: &str,
    progress: Option<ProgressCallback>,
) -> Result<PathBuf> {
    let tarball_path = tarball_path.as_ref();
    let target_dir = target_dir.as_ref();

    if !tarball_path.exists() {
        return Err(Error::Other(format!(
            "Package tarball not found: {}",
            tarball_path.display()
        )));
    }

    // Create Plugins directory if it doesn't exist
    let plugins_dir = target_dir.join("Plugins");
    fs::create_dir_all(&plugins_dir)?;

    // Before extracting, check for existing installation by searching for the .uplugin file
    // The .uplugin filename is the canonical identifier for a plugin
    let uplugin_name = format!("{}.uplugin", package_name);
    let mut existing_plugin_dir: Option<PathBuf> = None;
    let mut backup_dir: Option<PathBuf> = None;

    if let Ok(entries) = fs::read_dir(&plugins_dir) {
        'outer: for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Check if this directory contains the matching .uplugin file (case-insensitive)
                if let Ok(dir_entries) = fs::read_dir(&path) {
                    for dir_entry in dir_entries.flatten() {
                        let file_path = dir_entry.path();
                        if file_path.is_file() {
                            if let Some(file_name) = file_path.file_name() {
                                if file_name
                                    .to_string_lossy()
                                    .eq_ignore_ascii_case(&uplugin_name)
                                {
                                    existing_plugin_dir = Some(path);
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // If existing installation found, back it up before installing
    if let Some(ref existing_dir) = existing_plugin_dir {
        let backup_path = plugins_dir.join(format!("{}.unrealpm_backup", package_name));

        // Remove any stale backup from a previous failed install
        if backup_path.exists() {
            let _ = fs::remove_dir_all(&backup_path);
        }

        if let Some(ref cb) = progress {
            cb(&format!("Backing up existing {}...", package_name), 0, 100);
        }

        fs::rename(existing_dir, &backup_path).map_err(|e| {
            Error::Other(format!(
                "Failed to backup existing plugin '{}' to '{}': {}",
                existing_dir.display(),
                backup_path.display(),
                e
            ))
        })?;

        backup_dir = Some(backup_path);
    }

    // Report extraction start
    if let Some(ref cb) = progress {
        cb(&format!("Extracting {}...", package_name), 0, 100);
    }

    // Helper closure to restore backup on failure
    let restore_backup = |backup: &Option<PathBuf>, original: &Option<PathBuf>| {
        if let (Some(backup_path), Some(original_path)) = (backup, original) {
            if backup_path.exists() {
                // Try to restore the backup
                let _ = fs::rename(backup_path, original_path);
            }
        }
    };

    // Open and extract the tarball
    let tar_gz = match File::open(tarball_path) {
        Ok(f) => f,
        Err(e) => {
            restore_backup(&backup_dir, &existing_plugin_dir);
            return Err(e.into());
        }
    };
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);

    // Extract to Plugins directory
    if let Err(e) = archive.unpack(&plugins_dir) {
        restore_backup(&backup_dir, &existing_plugin_dir);
        return Err(e.into());
    }

    // Report extraction complete
    if let Some(ref cb) = progress {
        cb(&format!("Extracted {}", package_name), 100, 100);
    }

    let installed_path = plugins_dir.join(package_name);

    // Check if the expected path exists
    if installed_path.exists() {
        // Success - remove backup
        if let Some(ref backup_path) = backup_dir {
            let _ = fs::remove_dir_all(backup_path);
        }
        return Ok(installed_path);
    }

    // The tarball's root folder might have a different name than the package.
    // Find the actual extracted directory by looking for the .uplugin file.
    let extracted_dir = match find_extracted_plugin_dir(&plugins_dir, package_name) {
        Ok(dir) => dir,
        Err(e) => {
            restore_backup(&backup_dir, &existing_plugin_dir);
            return Err(e);
        }
    };

    // If the extracted directory has a different name, rename it to match package_name
    if extracted_dir != installed_path {
        // Remove any existing directory with the target name (shouldn't happen, but be safe)
        if installed_path.exists() {
            if let Err(e) = fs::remove_dir_all(&installed_path) {
                restore_backup(&backup_dir, &existing_plugin_dir);
                return Err(e.into());
            }
        }

        // Rename the extracted directory to the expected name
        if let Err(e) = fs::rename(&extracted_dir, &installed_path) {
            restore_backup(&backup_dir, &existing_plugin_dir);
            return Err(Error::Other(format!(
                "Failed to rename plugin directory from '{}' to '{}': {}",
                extracted_dir.display(),
                installed_path.display(),
                e
            )));
        }
    }

    if installed_path.exists() {
        // Success - remove backup
        if let Some(ref backup_path) = backup_dir {
            let _ = fs::remove_dir_all(backup_path);
        }
        Ok(installed_path)
    } else {
        restore_backup(&backup_dir, &existing_plugin_dir);
        Err(Error::Other(format!(
            "Package extraction succeeded but plugin directory not found: {}",
            installed_path.display()
        )))
    }
}

/// Find the extracted plugin directory by searching for .uplugin files
///
/// This handles cases where the tarball's root folder name doesn't match
/// the package name (e.g., tarball contains `chroma-sense/` but package is `ChromaSense`)
fn find_extracted_plugin_dir(plugins_dir: &Path, package_name: &str) -> Result<PathBuf> {
    // First, try case-insensitive match for the package name
    if let Ok(entries) = fs::read_dir(plugins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let dir_name = path.file_name().unwrap_or_default().to_string_lossy();

                // Case-insensitive match
                if dir_name.eq_ignore_ascii_case(package_name) {
                    return Ok(path);
                }

                // Check if this directory contains a .uplugin file
                // The .uplugin filename might match the package name
                let uplugin_path = path.join(format!("{}.uplugin", package_name));
                if uplugin_path.exists() {
                    return Ok(path);
                }
            }
        }
    }

    // Second pass: find any directory with a .uplugin file
    // This is a fallback for packages with unconventional naming
    if let Ok(entries) = fs::read_dir(plugins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Check for any .uplugin file in this directory
                if let Ok(dir_entries) = fs::read_dir(&path) {
                    for dir_entry in dir_entries.flatten() {
                        let file_path = dir_entry.path();
                        if file_path.is_file() {
                            if let Some(ext) = file_path.extension() {
                                if ext == "uplugin" {
                                    return Ok(path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Err(Error::Other(format!(
        "Could not find extracted plugin directory for '{}' in {}",
        package_name,
        plugins_dir.display()
    )))
}

/// Verify package checksum using SHA256
///
/// # Arguments
///
/// * `tarball_path` - Path to the .tar.gz package file
/// * `expected_checksum` - Expected SHA256 checksum (hex string)
/// * `progress` - Optional callback for progress updates
pub fn verify_checksum<P: AsRef<Path>>(
    tarball_path: P,
    expected_checksum: &str,
    progress: Option<ProgressCallback>,
) -> Result<()> {
    let tarball_path = tarball_path.as_ref();

    if expected_checksum.is_empty() {
        return Err(Error::Other("Empty checksum".to_string()));
    }

    // Report verification start
    if let Some(ref cb) = progress {
        cb("Verifying checksum...", 0, 100);
    }

    // Get file size for progress reporting
    let file_size = fs::metadata(tarball_path)?.len();

    // Read the tarball file
    let mut file = File::open(tarball_path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 8192]; // 8KB buffer for reading
    let mut bytes_processed: u64 = 0;

    // Compute SHA256 hash
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
        bytes_processed += bytes_read as u64;

        // Report progress
        if let Some(ref cb) = progress {
            cb("Verifying checksum...", bytes_processed, file_size);
        }
    }

    // Get the computed hash as a hex string
    let computed_hash = format!("{:x}", hasher.finalize());

    // Compare with expected checksum (case-insensitive)
    if computed_hash.eq_ignore_ascii_case(expected_checksum) {
        if let Some(ref cb) = progress {
            cb("Checksum verified", file_size, file_size);
        }
        Ok(())
    } else {
        Err(Error::Other(format!(
            "Checksum mismatch!\nExpected: {}\nComputed: {}",
            expected_checksum, computed_hash
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::sync::atomic::{AtomicU32, Ordering};
    use tar::Builder;
    use tempfile::TempDir;

    /// Create a test tarball with a plugin structure
    fn create_test_tarball(dir: &Path, plugin_name: &str, tarball_name: &str) -> PathBuf {
        let tarball_path = dir.join(format!("{}.tar.gz", tarball_name));
        let tar_gz = File::create(&tarball_path).unwrap();
        let enc = GzEncoder::new(tar_gz, Compression::default());
        let mut builder = Builder::new(enc);

        // Create plugin directory structure in tarball
        let plugin_dir = format!("{}/", plugin_name);
        let uplugin_content = format!(
            r#"{{
    "FileVersion": 3,
    "VersionName": "1.0.0",
    "FriendlyName": "{}",
    "Description": "Test plugin"
}}"#,
            plugin_name
        );

        // Add .uplugin file
        let uplugin_path = format!("{}{}.uplugin", plugin_dir, plugin_name);
        let mut header = tar::Header::new_gnu();
        header.set_path(&uplugin_path).unwrap();
        header.set_size(uplugin_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, &uplugin_path, uplugin_content.as_bytes())
            .unwrap();

        // Add a source file
        let source_content = b"// Test source file\n";
        let source_path = format!("{}Source/{}.cpp", plugin_dir, plugin_name);
        let mut header = tar::Header::new_gnu();
        header.set_path(&source_path).unwrap();
        header.set_size(source_content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, &source_path, &source_content[..])
            .unwrap();

        builder.finish().unwrap();
        tarball_path
    }

    /// Compute SHA256 hash of a file
    fn compute_sha256(path: &Path) -> String {
        let mut file = File::open(path).unwrap();
        let mut hasher = Sha256::new();
        let mut buffer = vec![0; 8192];
        loop {
            let bytes_read = file.read(&mut buffer).unwrap();
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }
        format!("{:x}", hasher.finalize())
    }

    // ============================================================================
    // verify_checksum tests
    // ============================================================================

    #[test]
    fn test_verify_checksum_valid() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.tar.gz");

        // Create a test file
        let content = b"Hello, World!";
        fs::write(&test_file, content).unwrap();

        // Compute expected checksum
        let expected = compute_sha256(&test_file);

        // Verify should succeed
        let result = verify_checksum(&test_file, &expected, None);
        assert!(result.is_ok(), "Valid checksum should pass verification");
    }

    #[test]
    fn test_verify_checksum_case_insensitive() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.tar.gz");

        let content = b"Test content";
        fs::write(&test_file, content).unwrap();

        let expected = compute_sha256(&test_file);

        // Test uppercase
        let result = verify_checksum(&test_file, &expected.to_uppercase(), None);
        assert!(result.is_ok(), "Uppercase checksum should pass");

        // Test lowercase
        let result = verify_checksum(&test_file, &expected.to_lowercase(), None);
        assert!(result.is_ok(), "Lowercase checksum should pass");
    }

    #[test]
    fn test_verify_checksum_invalid() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.tar.gz");

        fs::write(&test_file, b"Test content").unwrap();

        let invalid_checksum = "0".repeat(64); // All zeros - definitely wrong

        let result = verify_checksum(&test_file, &invalid_checksum, None);
        assert!(result.is_err(), "Invalid checksum should fail");

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Checksum mismatch"),
            "Error should mention checksum mismatch"
        );
    }

    #[test]
    fn test_verify_checksum_empty() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.tar.gz");
        fs::write(&test_file, b"content").unwrap();

        let result = verify_checksum(&test_file, "", None);
        assert!(result.is_err(), "Empty checksum should fail");
        assert!(result.unwrap_err().to_string().contains("Empty checksum"));
    }

    #[test]
    fn test_verify_checksum_file_not_found() {
        let result = verify_checksum("/nonexistent/file.tar.gz", "abc123", None);
        assert!(result.is_err(), "Missing file should fail");
    }

    #[test]
    fn test_verify_checksum_with_progress() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.tar.gz");

        // Create a larger file to ensure progress is called
        let content = vec![0u8; 32768]; // 32KB
        fs::write(&test_file, &content).unwrap();

        let expected = compute_sha256(&test_file);

        let progress_count = Arc::new(AtomicU32::new(0));
        let progress_count_clone = progress_count.clone();

        let progress: ProgressCallback = Arc::new(move |_msg, _current, _total| {
            progress_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        let result = verify_checksum(&test_file, &expected, Some(progress));
        assert!(result.is_ok());
        assert!(
            progress_count.load(Ordering::SeqCst) > 0,
            "Progress callback should be called"
        );
    }

    // ============================================================================
    // install_package tests
    // ============================================================================

    #[test]
    fn test_install_package_basic() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();

        // Create a test tarball
        let tarball = create_test_tarball(temp_dir.path(), "TestPlugin", "TestPlugin");

        // Install the package
        let result = install_package(&tarball, &project_dir, "TestPlugin", None);
        assert!(result.is_ok(), "Installation should succeed: {:?}", result);

        let installed_path = result.unwrap();
        assert!(installed_path.exists(), "Plugin directory should exist");
        assert!(
            installed_path.join("TestPlugin.uplugin").exists(),
            ".uplugin file should exist"
        );
    }

    #[test]
    fn test_install_package_creates_plugins_dir() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();

        // Plugins directory should not exist yet
        assert!(!project_dir.join("Plugins").exists());

        let tarball = create_test_tarball(temp_dir.path(), "MyPlugin", "MyPlugin");
        let result = install_package(&tarball, &project_dir, "MyPlugin", None);

        assert!(result.is_ok());
        assert!(
            project_dir.join("Plugins").exists(),
            "Plugins directory should be created"
        );
    }

    #[test]
    fn test_install_package_tarball_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = temp_dir.path().join("nonexistent.tar.gz");
        let target = temp_dir.path().to_path_buf();

        let result = install_package(&nonexistent, &target, "Plugin", None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("tarball not found"));
    }

    #[test]
    fn test_install_package_reinstall() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();

        let tarball = create_test_tarball(temp_dir.path(), "ReinstallPlugin", "ReinstallPlugin");

        // First install
        let result1 = install_package(&tarball, &project_dir, "ReinstallPlugin", None);
        assert!(result1.is_ok());

        // Second install (reinstall)
        let result2 = install_package(&tarball, &project_dir, "ReinstallPlugin", None);
        assert!(result2.is_ok(), "Reinstall should succeed: {:?}", result2);

        // Verify the plugin still exists
        let installed = result2.unwrap();
        assert!(installed.exists());
        assert!(installed.join("ReinstallPlugin.uplugin").exists());

        // Verify no backup directory remains
        assert!(!project_dir
            .join("Plugins")
            .join("ReinstallPlugin.unrealpm_backup")
            .exists());
    }

    #[test]
    fn test_install_package_with_progress() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();

        let tarball = create_test_tarball(temp_dir.path(), "ProgressPlugin", "ProgressPlugin");

        let progress_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
        let progress_messages_clone = progress_messages.clone();

        let progress: ProgressCallback = Arc::new(move |msg, _current, _total| {
            progress_messages_clone
                .lock()
                .unwrap()
                .push(msg.to_string());
        });

        let result = install_package(&tarball, &project_dir, "ProgressPlugin", Some(progress));
        assert!(result.is_ok());

        let messages = progress_messages.lock().unwrap();
        assert!(!messages.is_empty(), "Progress should be reported");
        assert!(
            messages.iter().any(|m| m.contains("Extracting")),
            "Should report extraction"
        );
    }

    // ============================================================================
    // find_extracted_plugin_dir tests
    // ============================================================================

    #[test]
    fn test_find_extracted_plugin_dir_exact_match() {
        let temp_dir = TempDir::new().unwrap();
        let plugins_dir = temp_dir.path();

        // Create plugin directory with exact name
        let plugin_dir = plugins_dir.join("ExactPlugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("ExactPlugin.uplugin"), "{}").unwrap();

        let result = find_extracted_plugin_dir(plugins_dir, "ExactPlugin");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), plugin_dir);
    }

    #[test]
    fn test_find_extracted_plugin_dir_case_insensitive() {
        let temp_dir = TempDir::new().unwrap();
        let plugins_dir = temp_dir.path();

        // Create plugin directory with different case
        let plugin_dir = plugins_dir.join("myplugin");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("MyPlugin.uplugin"), "{}").unwrap();

        let result = find_extracted_plugin_dir(plugins_dir, "MyPlugin");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), plugin_dir);
    }

    #[test]
    fn test_find_extracted_plugin_dir_by_uplugin_file() {
        let temp_dir = TempDir::new().unwrap();
        let plugins_dir = temp_dir.path();

        // Create plugin directory with different name but matching .uplugin
        let plugin_dir = plugins_dir.join("different-name");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("TargetPlugin.uplugin"), "{}").unwrap();

        let result = find_extracted_plugin_dir(plugins_dir, "TargetPlugin");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), plugin_dir);
    }

    #[test]
    fn test_find_extracted_plugin_dir_fallback_any_uplugin() {
        let temp_dir = TempDir::new().unwrap();
        let plugins_dir = temp_dir.path();

        // Create plugin directory with completely different name and uplugin
        let plugin_dir = plugins_dir.join("random-folder");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(plugin_dir.join("SomeOther.uplugin"), "{}").unwrap();

        let result = find_extracted_plugin_dir(plugins_dir, "WantedPlugin");
        assert!(result.is_ok());
        // Should find the directory with any .uplugin file as fallback
        assert_eq!(result.unwrap(), plugin_dir);
    }

    #[test]
    fn test_find_extracted_plugin_dir_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let plugins_dir = temp_dir.path();

        // Create directory without any .uplugin file
        let no_plugin_dir = plugins_dir.join("not-a-plugin");
        fs::create_dir_all(&no_plugin_dir).unwrap();
        fs::write(no_plugin_dir.join("readme.txt"), "Not a plugin").unwrap();

        let result = find_extracted_plugin_dir(plugins_dir, "MyPlugin");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Could not find extracted plugin"));
    }

    #[test]
    fn test_find_extracted_plugin_dir_empty() {
        let temp_dir = TempDir::new().unwrap();

        let result = find_extracted_plugin_dir(temp_dir.path(), "AnyPlugin");
        assert!(result.is_err());
    }
}
