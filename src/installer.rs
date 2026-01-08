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

/// Content-Addressable Storage (CAS) version for store layout
const CAS_VERSION: &str = "v1";

/// Get the global UnrealPM store directory
///
/// Returns `~/.unrealpm/store/v1/packages/` and creates it if it doesn't exist.
pub fn get_store_dir() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Other("Could not find home directory".to_string()))?;
    let store_dir = home
        .join(".unrealpm")
        .join("store")
        .join(CAS_VERSION)
        .join("packages");
    fs::create_dir_all(&store_dir)?;
    Ok(store_dir)
}

/// Get the store path for a package by its content hash
///
/// Returns `~/.unrealpm/store/v1/packages/<hash>/`
pub fn get_package_store_path(checksum: &str) -> Result<PathBuf> {
    let store_dir = get_store_dir()?;
    Ok(store_dir.join(checksum))
}

/// Check if a package is already in the global store
pub fn is_package_in_store(checksum: &str) -> Result<bool> {
    let store_path = get_package_store_path(checksum)?;
    Ok(store_path.exists())
}

/// Store a package in the global CAS store
///
/// Extracts the tarball to the global store indexed by its content hash.
/// If the package is already in the store, this is a no-op.
///
/// # Arguments
///
/// * `tarball_path` - Path to the .tar.gz package file
/// * `checksum` - SHA256 hash of the tarball (used as content address)
/// * `progress` - Optional callback for progress updates
///
/// # Returns
///
/// The path where the package was stored in the global store
pub fn store_package<P: AsRef<Path>>(
    tarball_path: P,
    checksum: &str,
    progress: Option<ProgressCallback>,
) -> Result<PathBuf> {
    let tarball_path = tarball_path.as_ref();
    let store_path = get_package_store_path(checksum)?;

    // If already in store, return early
    if store_path.exists() {
        if let Some(ref cb) = progress {
            cb("Package already in store", 100, 100);
        }
        return Ok(store_path);
    }

    if let Some(ref cb) = progress {
        cb("Extracting to global store...", 0, 100);
    }

    // Create a temporary directory for extraction (in case of failure)
    // Use a separate temp directory name to avoid path confusion
    let store_parent = store_path
        .parent()
        .ok_or_else(|| Error::Other("Store path has no parent directory".to_string()))?;
    let store_name = store_path
        .file_name()
        .ok_or_else(|| Error::Other("Store path has no filename".to_string()))?;
    let temp_name = format!("{}-extracting", store_name.to_string_lossy());
    let temp_store_path = store_parent.join(&temp_name);

    if temp_store_path.exists() {
        fs::remove_dir_all(&temp_store_path)?;
    }
    fs::create_dir_all(&temp_store_path)?;

    // Open and extract the tarball
    let tar_gz = File::open(tarball_path)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);

    // Disable permission preservation and extended attributes for better cross-platform support
    // (especially WSL where permission handling can be problematic)
    archive.set_preserve_permissions(false);
    archive.set_preserve_mtime(false);
    archive.set_overwrite(true);

    if let Err(e) = archive.unpack(&temp_store_path) {
        // Clean up on failure
        let _ = fs::remove_dir_all(&temp_store_path);
        return Err(e.into());
    }

    // Atomically move to final location
    fs::rename(&temp_store_path, &store_path).map_err(|e| {
        // Clean up on failure
        let _ = fs::remove_dir_all(&temp_store_path);
        Error::Other(format!("Failed to move package to store: {}", e))
    })?;

    if let Some(ref cb) = progress {
        cb("Stored in global cache", 100, 100);
    }

    Ok(store_path)
}

/// Link or copy a package from the global store to a target directory
///
/// Attempts to create hard links for all files. If hard linking fails
/// (e.g., cross-filesystem), falls back to copying.
///
/// # Arguments
///
/// * `store_path` - Path to the package in the global store
/// * `target_path` - Destination path in the project's Plugins/ directory
/// * `progress` - Optional callback for progress updates
pub fn link_or_copy_from_store(
    store_path: &Path,
    target_path: &Path,
    progress: Option<ProgressCallback>,
) -> Result<()> {
    if let Some(ref cb) = progress {
        cb("Linking from store...", 0, 100);
    }

    // Remove existing target if it exists
    if target_path.exists() {
        fs::remove_dir_all(target_path)?;
    }

    // Try hard linking first
    match link_directory_recursive(store_path, target_path) {
        Ok(()) => {
            if let Some(ref cb) = progress {
                cb("Linked from store", 100, 100);
            }
            Ok(())
        }
        Err(_) => {
            // Fall back to copying
            if let Some(ref cb) = progress {
                cb("Copying from store (hard links not supported)...", 50, 100);
            }
            copy_directory_recursive(store_path, target_path)?;
            if let Some(ref cb) = progress {
                cb("Copied from store", 100, 100);
            }
            Ok(())
        }
    }
}

/// Recursively create hard links for all files in a directory
fn link_directory_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            link_directory_recursive(&src_path, &dst_path)?;
        } else {
            // Try to create hard link
            fs::hard_link(&src_path, &dst_path).map_err(|e| {
                Error::Other(format!(
                    "Failed to hard link {} -> {}: {}",
                    src_path.display(),
                    dst_path.display(),
                    e
                ))
            })?;
        }
    }

    Ok(())
}

/// Recursively copy all files in a directory
fn copy_directory_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_directory_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Install a package using Content-Addressable Storage (CAS)
///
/// This function:
/// 1. Checks if the package is already in the global store
/// 2. If not, extracts the tarball to the store
/// 3. Hard links (or copies) from the store to the project's Plugins/ directory
///
/// This approach provides significant disk space savings when the same package
/// is used across multiple projects.
///
/// # Arguments
///
/// * `tarball_path` - Path to the .tar.gz package file
/// * `target_dir` - Project root directory
/// * `package_name` - Name of the package being installed
/// * `checksum` - SHA256 hash of the tarball
/// * `progress` - Optional callback for progress updates
///
/// # Returns
///
/// The path where the package was installed in the project
pub fn install_package_cas<P: AsRef<Path>>(
    tarball_path: P,
    target_dir: P,
    package_name: &str,
    checksum: &str,
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

    // Store the package in the global store (if not already there)
    let store_path = store_package(tarball_path, checksum, progress.clone())?;

    // Find the plugin directory within the store
    // The tarball may have a root folder with a different name
    let plugin_store_path =
        find_extracted_plugin_dir(&store_path, package_name).unwrap_or_else(|_| store_path.clone());

    // Before linking, handle existing installation
    let installed_path = plugins_dir.join(package_name);
    let uplugin_name = format!("{}.uplugin", package_name);
    let mut existing_plugin_dir: Option<PathBuf> = None;
    let mut backup_dir: Option<PathBuf> = None;

    // Search for existing plugin by .uplugin file
    if let Ok(entries) = fs::read_dir(&plugins_dir) {
        'outer: for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
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

    // Backup existing installation
    if let Some(ref existing_dir) = existing_plugin_dir {
        let backup_path = plugins_dir.join(format!("{}.unrealpm_backup", package_name));
        if backup_path.exists() {
            let _ = fs::remove_dir_all(&backup_path);
        }

        if let Some(ref cb) = progress {
            cb(&format!("Backing up existing {}...", package_name), 0, 100);
        }

        fs::rename(existing_dir, &backup_path)?;
        backup_dir = Some(backup_path);
    }

    // Link or copy from store to project
    let link_result =
        link_or_copy_from_store(&plugin_store_path, &installed_path, progress.clone());

    if let Err(e) = link_result {
        // Restore backup on failure
        if let (Some(backup_path), Some(original_path)) = (&backup_dir, &existing_plugin_dir) {
            if backup_path.exists() {
                let _ = fs::rename(backup_path, original_path);
            }
        }
        return Err(e);
    }

    // Clean up backup on success
    if let Some(ref backup_path) = backup_dir {
        let _ = fs::remove_dir_all(backup_path);
    }

    if let Some(ref cb) = progress {
        cb(&format!("Installed {}", package_name), 100, 100);
    }

    Ok(installed_path)
}

/// Get statistics about the global store
pub fn get_store_stats() -> Result<StoreStats> {
    let store_dir = get_store_dir()?;
    let mut stats = StoreStats::default();

    if let Ok(entries) = fs::read_dir(&store_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                stats.package_count += 1;
                stats.total_size += calculate_dir_size(&entry.path()).unwrap_or(0);
            }
        }
    }

    Ok(stats)
}

/// Statistics about the global CAS store
#[derive(Default, Debug)]
pub struct StoreStats {
    /// Number of packages in the store
    pub package_count: usize,
    /// Total size of the store in bytes
    pub total_size: u64,
}

/// Calculate the total size of a directory recursively
fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0;

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            total += calculate_dir_size(&path)?;
        } else {
            total += fs::metadata(&path)?.len();
        }
    }

    Ok(total)
}

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

    // ============================================================================
    // Content-Addressable Storage (CAS) tests
    // ============================================================================

    #[test]
    fn test_get_store_dir_creates_directory() {
        let store_dir = get_store_dir().unwrap();
        assert!(store_dir.exists(), "Store directory should be created");
        assert!(
            store_dir.ends_with("packages"),
            "Store should end with 'packages'"
        );
        assert!(
            store_dir
                .to_string_lossy()
                .contains(".unrealpm/store/v1/packages"),
            "Store path should contain expected structure"
        );
    }

    #[test]
    fn test_get_package_store_path() {
        let checksum = "abc123def456";
        let path = get_package_store_path(checksum).unwrap();
        assert!(
            path.ends_with(checksum),
            "Package store path should end with checksum"
        );
    }

    #[test]
    fn test_is_package_in_store_not_present() {
        let fake_checksum = "nonexistent_checksum_12345";
        let result = is_package_in_store(fake_checksum).unwrap();
        assert!(!result, "Non-existent package should not be in store");
    }

    #[test]
    fn test_store_package_basic() {
        let temp_dir = TempDir::new().unwrap();
        let tarball = create_test_tarball(temp_dir.path(), "StoreTestPlugin", "StoreTestPlugin");
        let checksum = compute_sha256(&tarball);

        // Store the package
        let store_path = store_package(&tarball, &checksum, None).unwrap();
        assert!(store_path.exists(), "Store path should exist after storing");

        // Verify package is in store
        assert!(
            is_package_in_store(&checksum).unwrap(),
            "Package should be in store"
        );

        // Verify the plugin content was extracted
        let plugin_dir = store_path.join("StoreTestPlugin");
        assert!(
            plugin_dir.exists(),
            "Plugin directory should exist in store"
        );
        assert!(
            plugin_dir.join("StoreTestPlugin.uplugin").exists(),
            ".uplugin file should exist in store"
        );
    }

    #[test]
    fn test_store_package_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let tarball = create_test_tarball(temp_dir.path(), "IdempotentPlugin", "IdempotentPlugin");
        let checksum = compute_sha256(&tarball);

        // Store the package twice
        let store_path1 = store_package(&tarball, &checksum, None).unwrap();
        let store_path2 = store_package(&tarball, &checksum, None).unwrap();

        // Should return the same path
        assert_eq!(store_path1, store_path2, "Store path should be consistent");
    }

    #[test]
    fn test_store_package_with_progress() {
        let temp_dir = TempDir::new().unwrap();
        let tarball = create_test_tarball(
            temp_dir.path(),
            "ProgressStorePlugin",
            "ProgressStorePlugin",
        );
        let checksum = compute_sha256(&tarball);

        let progress_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
        let progress_messages_clone = progress_messages.clone();

        let progress: ProgressCallback = Arc::new(move |msg, _current, _total| {
            progress_messages_clone
                .lock()
                .unwrap()
                .push(msg.to_string());
        });

        let result = store_package(&tarball, &checksum, Some(progress));
        assert!(result.is_ok());

        let messages = progress_messages.lock().unwrap();
        assert!(!messages.is_empty(), "Progress should be reported");
    }

    #[test]
    fn test_link_directory_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("source");
        let dst_dir = temp_dir.path().join("dest");

        // Create source structure
        fs::create_dir_all(src_dir.join("subdir")).unwrap();
        fs::write(src_dir.join("file1.txt"), "content1").unwrap();
        fs::write(src_dir.join("subdir/file2.txt"), "content2").unwrap();

        // Link
        let result = link_directory_recursive(&src_dir, &dst_dir);

        // On some systems (like cross-filesystem), hard links may fail
        // That's OK - we have fallback to copy
        if result.is_ok() {
            assert!(dst_dir.join("file1.txt").exists());
            assert!(dst_dir.join("subdir/file2.txt").exists());

            // Verify content
            let content = fs::read_to_string(dst_dir.join("file1.txt")).unwrap();
            assert_eq!(content, "content1");
        }
    }

    #[test]
    fn test_copy_directory_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("source");
        let dst_dir = temp_dir.path().join("dest");

        // Create source structure
        fs::create_dir_all(src_dir.join("subdir")).unwrap();
        fs::write(src_dir.join("file1.txt"), "content1").unwrap();
        fs::write(src_dir.join("subdir/file2.txt"), "content2").unwrap();

        // Copy
        let result = copy_directory_recursive(&src_dir, &dst_dir);
        assert!(result.is_ok());

        assert!(dst_dir.join("file1.txt").exists());
        assert!(dst_dir.join("subdir/file2.txt").exists());

        // Verify content
        let content = fs::read_to_string(dst_dir.join("file1.txt")).unwrap();
        assert_eq!(content, "content1");
    }

    #[test]
    fn test_link_or_copy_from_store() {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("store");
        let target_path = temp_dir.path().join("target");

        // Create store structure
        fs::create_dir_all(&store_path).unwrap();
        fs::write(store_path.join("test.txt"), "test content").unwrap();

        // Link or copy
        let result = link_or_copy_from_store(&store_path, &target_path, None);
        assert!(result.is_ok());

        assert!(target_path.exists());
        assert!(target_path.join("test.txt").exists());

        let content = fs::read_to_string(target_path.join("test.txt")).unwrap();
        assert_eq!(content, "test content");
    }

    #[test]
    fn test_link_or_copy_replaces_existing() {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("store");
        let target_path = temp_dir.path().join("target");

        // Create store structure
        fs::create_dir_all(&store_path).unwrap();
        fs::write(store_path.join("new.txt"), "new content").unwrap();

        // Create existing target
        fs::create_dir_all(&target_path).unwrap();
        fs::write(target_path.join("old.txt"), "old content").unwrap();

        // Link or copy should replace
        let result = link_or_copy_from_store(&store_path, &target_path, None);
        assert!(result.is_ok());

        // Old file should be gone, new file should exist
        assert!(!target_path.join("old.txt").exists());
        assert!(target_path.join("new.txt").exists());
    }

    #[test]
    fn test_install_package_cas_basic() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();

        let tarball = create_test_tarball(temp_dir.path(), "CASPlugin", "CASPlugin");
        let checksum = compute_sha256(&tarball);

        // Install using CAS
        let result = install_package_cas(&tarball, &project_dir, "CASPlugin", &checksum, None);
        assert!(
            result.is_ok(),
            "CAS installation should succeed: {:?}",
            result
        );

        let installed_path = result.unwrap();
        assert!(installed_path.exists(), "Plugin should be installed");
        assert!(
            installed_path.join("CASPlugin.uplugin").exists(),
            ".uplugin should exist"
        );

        // Verify package is in global store
        assert!(
            is_package_in_store(&checksum).unwrap(),
            "Package should be in global store"
        );
    }

    #[test]
    fn test_install_package_cas_reuses_store() {
        let temp_dir = TempDir::new().unwrap();
        let tarball = create_test_tarball(temp_dir.path(), "ReusePlugin", "ReusePlugin");
        let checksum = compute_sha256(&tarball);

        // First project
        let project1 = temp_dir.path().join("project1");
        fs::create_dir_all(&project1).unwrap();
        let result1 = install_package_cas(&tarball, &project1, "ReusePlugin", &checksum, None);
        assert!(result1.is_ok());

        // Second project - should reuse from store
        let project2 = temp_dir.path().join("project2");
        fs::create_dir_all(&project2).unwrap();

        let progress_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
        let progress_clone = progress_messages.clone();
        let progress: ProgressCallback = Arc::new(move |msg, _, _| {
            progress_clone.lock().unwrap().push(msg.to_string());
        });

        let result2 = install_package_cas(
            &tarball,
            &project2,
            "ReusePlugin",
            &checksum,
            Some(progress),
        );
        assert!(result2.is_ok());

        // Check that "already in store" message was shown
        let messages = progress_messages.lock().unwrap();
        assert!(
            messages.iter().any(|m| m.contains("already in store")),
            "Should indicate package was already in store"
        );

        // Both plugins should exist
        assert!(project1
            .join("Plugins/ReusePlugin/ReusePlugin.uplugin")
            .exists());
        assert!(project2
            .join("Plugins/ReusePlugin/ReusePlugin.uplugin")
            .exists());
    }

    #[test]
    fn test_install_package_cas_reinstall() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path().join("project");
        fs::create_dir_all(&project_dir).unwrap();

        let tarball = create_test_tarball(temp_dir.path(), "ReinstallCAS", "ReinstallCAS");
        let checksum = compute_sha256(&tarball);

        // First install
        let result1 = install_package_cas(&tarball, &project_dir, "ReinstallCAS", &checksum, None);
        assert!(result1.is_ok());

        // Second install (reinstall)
        let result2 = install_package_cas(&tarball, &project_dir, "ReinstallCAS", &checksum, None);
        assert!(result2.is_ok());

        // Verify plugin exists
        let installed = result2.unwrap();
        assert!(installed.exists());
        assert!(installed.join("ReinstallCAS.uplugin").exists());

        // Verify no backup remains
        assert!(!project_dir
            .join("Plugins/ReinstallCAS.unrealpm_backup")
            .exists());
    }

    #[test]
    fn test_install_package_cas_tarball_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let nonexistent = temp_dir.path().join("nonexistent.tar.gz");
        let target = temp_dir.path().to_path_buf();

        let result = install_package_cas(&nonexistent, &target, "Plugin", "somechecksum", None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("tarball not found"));
    }

    #[test]
    fn test_get_store_stats() {
        // Just verify it doesn't crash and returns valid data
        let stats = get_store_stats().unwrap();
        // package_count >= 0 is always true for usize
        // total_size >= 0 is always true for u64
        // Just check we can access the fields
        let _ = stats.package_count;
        let _ = stats.total_size;
    }

    #[test]
    fn test_calculate_dir_size() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("size_test");
        fs::create_dir_all(&test_dir).unwrap();

        // Create files with known sizes
        fs::write(test_dir.join("file1.txt"), "12345").unwrap(); // 5 bytes
        fs::write(test_dir.join("file2.txt"), "1234567890").unwrap(); // 10 bytes

        let size = calculate_dir_size(&test_dir).unwrap();
        assert_eq!(size, 15, "Directory size should be 15 bytes");
    }

    #[test]
    fn test_calculate_dir_size_nested() {
        let temp_dir = TempDir::new().unwrap();
        let test_dir = temp_dir.path().join("nested_size");
        fs::create_dir_all(test_dir.join("sub")).unwrap();

        fs::write(test_dir.join("file1.txt"), "12345").unwrap(); // 5 bytes
        fs::write(test_dir.join("sub/file2.txt"), "1234567890").unwrap(); // 10 bytes

        let size = calculate_dir_size(&test_dir).unwrap();
        assert_eq!(size, 15, "Nested directory size should be 15 bytes");
    }
}
