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

    // Before extracting, remove any existing installation by searching for the .uplugin file
    // The .uplugin filename is the canonical identifier for a plugin
    let uplugin_name = format!("{}.uplugin", package_name);
    if let Ok(entries) = fs::read_dir(&plugins_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Check if this directory contains the matching .uplugin file (case-insensitive)
                if let Ok(dir_entries) = fs::read_dir(&path) {
                    for dir_entry in dir_entries.flatten() {
                        let file_path = dir_entry.path();
                        if file_path.is_file() {
                            if let Some(file_name) = file_path.file_name() {
                                if file_name.to_string_lossy().eq_ignore_ascii_case(&uplugin_name) {
                                    if let Some(ref cb) = progress {
                                        cb(&format!("Removing existing installation of {}...", package_name), 0, 100);
                                    }
                                    fs::remove_dir_all(&path).map_err(|e| {
                                        Error::Other(format!(
                                            "Failed to remove existing plugin directory '{}': {}",
                                            path.display(),
                                            e
                                        ))
                                    })?;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Report extraction start
    if let Some(ref cb) = progress {
        cb(&format!("Extracting {}...", package_name), 0, 100);
    }

    // Open and extract the tarball
    let tar_gz = File::open(tarball_path)?;
    let tar = GzDecoder::new(tar_gz);
    let mut archive = Archive::new(tar);

    // Extract to Plugins directory
    archive.unpack(&plugins_dir)?;

    // Report extraction complete
    if let Some(ref cb) = progress {
        cb(&format!("Extracted {}", package_name), 100, 100);
    }

    let installed_path = plugins_dir.join(package_name);

    // Check if the expected path exists
    if installed_path.exists() {
        return Ok(installed_path);
    }

    // The tarball's root folder might have a different name than the package.
    // Find the actual extracted directory by looking for the .uplugin file.
    let extracted_dir = find_extracted_plugin_dir(&plugins_dir, package_name)?;

    // If the extracted directory has a different name, rename it to match package_name
    if extracted_dir != installed_path {
        // Remove any existing directory with the target name (shouldn't happen, but be safe)
        if installed_path.exists() {
            fs::remove_dir_all(&installed_path)?;
        }

        // Rename the extracted directory to the expected name
        fs::rename(&extracted_dir, &installed_path).map_err(|e| {
            Error::Other(format!(
                "Failed to rename plugin directory from '{}' to '{}': {}",
                extracted_dir.display(),
                installed_path.display(),
                e
            ))
        })?;
    }

    if installed_path.exists() {
        Ok(installed_path)
    } else {
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
