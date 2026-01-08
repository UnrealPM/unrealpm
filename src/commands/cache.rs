//! Cache management commands for the global CAS store
//!
//! Provides commands to manage the content-addressable storage:
//! - `cache list` - List cached packages
//! - `cache clean` - Remove unused packages
//! - `cache info` - Show store statistics
//! - `cache path` - Show store location

use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use unrealpm::{get_store_dir, get_store_stats, Lockfile};

/// Format bytes as human-readable size
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Calculate directory size recursively
fn dir_size(path: &PathBuf) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path);
            } else if let Ok(meta) = fs::metadata(&path) {
                total += meta.len();
            }
        }
    }
    total
}

/// Get package name from store directory by looking for .uplugin file
fn get_package_name(store_path: &PathBuf) -> Option<String> {
    // Look for .uplugin file in the directory or subdirectories
    if let Ok(entries) = fs::read_dir(store_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Check subdirectory for .uplugin
                if let Ok(sub_entries) = fs::read_dir(&path) {
                    for sub_entry in sub_entries.flatten() {
                        let sub_path = sub_entry.path();
                        if let Some(ext) = sub_path.extension() {
                            if ext == "uplugin" {
                                if let Some(stem) = sub_path.file_stem() {
                                    return Some(stem.to_string_lossy().to_string());
                                }
                            }
                        }
                    }
                }
            } else if let Some(ext) = path.extension() {
                if ext == "uplugin" {
                    if let Some(stem) = path.file_stem() {
                        return Some(stem.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    None
}

/// List all cached packages in the store
pub fn run_list(verbose: bool) -> Result<()> {
    let store_dir = get_store_dir()?;

    println!("Cached packages in {}:", store_dir.display());
    println!();

    let mut entries: Vec<_> = fs::read_dir(&store_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    if entries.is_empty() {
        println!("  (no packages cached)");
        println!();
        println!("Packages are cached automatically when you run `unrealpm install`.");
        return Ok(());
    }

    // Sort by modification time (most recent first)
    entries.sort_by(|a, b| {
        let a_time = a.metadata().and_then(|m| m.modified()).ok();
        let b_time = b.metadata().and_then(|m| m.modified()).ok();
        b_time.cmp(&a_time)
    });

    let mut total_size: u64 = 0;

    for entry in &entries {
        let path = entry.path();
        let hash = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let size = dir_size(&path);
        total_size += size;

        let package_name = get_package_name(&path);

        if verbose {
            // Verbose: show full hash and more details
            let name_display = package_name
                .as_ref()
                .map(|n| format!(" ({})", n))
                .unwrap_or_default();
            println!("  {}{}", hash, name_display);
            println!("    Size: {}", format_size(size));
            println!("    Path: {}", path.display());
            println!();
        } else {
            // Compact: show short hash and name
            let short_hash = if hash.len() > 12 { &hash[..12] } else { &hash };
            let name_display = package_name.unwrap_or_else(|| "unknown".to_string());
            println!(
                "  {}...  {:>10}  {}",
                short_hash,
                format_size(size),
                name_display
            );
        }
    }

    println!();
    println!(
        "Total: {} packages, {}",
        entries.len(),
        format_size(total_size)
    );

    Ok(())
}

/// Show cache statistics
pub fn run_info() -> Result<()> {
    let store_dir = get_store_dir()?;
    let stats = get_store_stats()?;

    println!("Cache Information");
    println!("=================");
    println!();
    println!("Store location: {}", store_dir.display());
    println!("Packages cached: {}", stats.package_count);
    println!("Total size: {}", format_size(stats.total_size));
    println!();

    // Show store structure
    println!("Store structure:");
    println!("  ~/.unrealpm/store/v1/packages/<sha256>/");
    println!();

    // Check if store exists and is accessible
    if store_dir.exists() {
        println!("Status: Active");
    } else {
        println!("Status: Not initialized (will be created on first install)");
    }

    Ok(())
}

/// Show store path
pub fn run_path() -> Result<()> {
    let store_dir = get_store_dir()?;
    println!("{}", store_dir.display());
    Ok(())
}

/// Clean unused packages from the cache
pub fn run_clean(all: bool, dry_run: bool) -> Result<()> {
    let store_dir = get_store_dir()?;

    if all {
        // Remove ALL cached packages
        if dry_run {
            println!("[DRY RUN] Would remove all cached packages from:");
            println!("  {}", store_dir.display());

            let stats = get_store_stats()?;
            println!();
            println!(
                "Would free {} ({} packages)",
                format_size(stats.total_size),
                stats.package_count
            );
            return Ok(());
        }

        println!("Removing all cached packages...");

        let mut removed_count = 0;
        let mut freed_size: u64 = 0;

        if let Ok(entries) = fs::read_dir(&store_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let size = dir_size(&path);
                    if fs::remove_dir_all(&path).is_ok() {
                        removed_count += 1;
                        freed_size += size;
                    }
                }
            }
        }

        println!();
        println!(
            "Removed {} packages, freed {}",
            removed_count,
            format_size(freed_size)
        );
        return Ok(());
    }

    // Smart clean: only remove packages not referenced by any lockfile
    println!("Scanning for unused packages...");
    println!();

    // Collect checksums from current project's lockfile
    let mut used_checksums = HashSet::new();

    if let Ok(Some(lockfile)) = Lockfile::load() {
        for pkg in lockfile.packages.values() {
            used_checksums.insert(pkg.checksum.clone());
        }
    }

    // Find unused packages in store
    let mut unused_packages = Vec::new();
    let mut unused_size: u64 = 0;

    if let Ok(entries) = fs::read_dir(&store_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let hash = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Skip temp/extracting directories
                if hash.ends_with("-extracting") {
                    let size = dir_size(&path);
                    unused_packages.push((path, hash, size, true));
                    unused_size += size;
                    continue;
                }

                if !used_checksums.contains(&hash) {
                    let size = dir_size(&path);
                    unused_packages.push((path, hash, size, false));
                    unused_size += size;
                }
            }
        }
    }

    if unused_packages.is_empty() {
        println!("No unused packages found.");
        println!();
        println!("All cached packages are referenced by the current project's lockfile.");
        println!("Use `unrealpm cache clean --all` to remove everything.");
        return Ok(());
    }

    println!("Found {} unused packages:", unused_packages.len());
    println!();

    for (path, hash, size, is_temp) in &unused_packages {
        let short_hash = if hash.len() > 12 { &hash[..12] } else { hash };
        let name = get_package_name(path).unwrap_or_else(|| "unknown".to_string());
        let temp_marker = if *is_temp { " (temp)" } else { "" };
        println!(
            "  {}...  {:>10}  {}{}",
            short_hash,
            format_size(*size),
            name,
            temp_marker
        );
    }

    println!();
    println!("Total: {}", format_size(unused_size));
    println!();

    if dry_run {
        println!("[DRY RUN] Would remove {} packages", unused_packages.len());
        return Ok(());
    }

    // Actually remove
    println!("Removing unused packages...");

    let mut removed_count = 0;
    let mut freed_size: u64 = 0;

    for (path, _, size, _) in unused_packages {
        if fs::remove_dir_all(&path).is_ok() {
            removed_count += 1;
            freed_size += size;
        }
    }

    println!();
    println!(
        "Removed {} packages, freed {}",
        removed_count,
        format_size(freed_size)
    );

    Ok(())
}

/// Verify cache integrity
pub fn run_verify() -> Result<()> {
    let store_dir = get_store_dir()?;

    println!("Verifying cache integrity...");
    println!();

    let mut total = 0;
    let mut valid = 0;
    let mut invalid = Vec::new();

    if let Ok(entries) = fs::read_dir(&store_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                total += 1;
                let hash = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                // Check if it's a temp directory (should be cleaned up)
                if hash.ends_with("-extracting") {
                    invalid.push((path, "incomplete extraction".to_string()));
                    continue;
                }

                // Check if directory has content
                let has_content = fs::read_dir(&path)
                    .map(|mut d| d.next().is_some())
                    .unwrap_or(false);

                if !has_content {
                    invalid.push((path, "empty directory".to_string()));
                    continue;
                }

                valid += 1;
            }
        }
    }

    if invalid.is_empty() {
        println!("All {} cached packages are valid.", total);
    } else {
        println!("Found {} issues:", invalid.len());
        println!();
        for (path, reason) in &invalid {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let short_name = if name.len() > 16 {
                format!("{}...", &name[..16])
            } else {
                name
            };
            println!("  {} - {}", short_name, reason);
        }
        println!();
        println!("{}/{} packages valid", valid, total);
        println!();
        println!("Run `unrealpm cache clean` to remove invalid entries.");
    }

    Ok(())
}
