//! Pack command - create a package tarball without publishing
//!
//! This is useful for:
//! - Testing packages locally before publishing
//! - CI/CD pipelines that need to create packages
//! - Distributing packages outside the registry

use anyhow::Result;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use unrealpm::UPlugin;

pub fn run(
    path: Option<String>,
    output: Option<String>,
    include_binaries: bool,
    dry_run: bool,
) -> Result<()> {
    println!("Packing plugin...");
    println!();

    // Determine plugin directory
    let plugin_dir = if let Some(p) = path {
        PathBuf::from(p)
    } else {
        env::current_dir()?
    };

    if !plugin_dir.exists() {
        anyhow::bail!("Plugin directory does not exist: {}", plugin_dir.display());
    }

    // Find and load .uplugin file
    println!("  Validating plugin...");
    let uplugin_path = UPlugin::find(&plugin_dir)?;
    let uplugin = UPlugin::load(&uplugin_path)?;
    let plugin_name = UPlugin::name(&uplugin_path)
        .ok_or_else(|| anyhow::anyhow!("Could not determine plugin name from file"))?;

    println!("  Plugin: {}", plugin_name);
    println!("  Version: {}", uplugin.version_name);
    if let Some(desc) = &uplugin.description {
        if !desc.is_empty() {
            println!("  Description: {}", desc);
        }
    }
    println!();

    // Determine output path
    let tarball_name = format!("{}-{}.tar.gz", plugin_name, uplugin.version_name);
    let output_path = if let Some(out) = output {
        let out_path = PathBuf::from(&out);
        if out_path.is_dir() {
            out_path.join(&tarball_name)
        } else if out.ends_with(".tar.gz") || out.ends_with(".tgz") {
            out_path
        } else {
            // Treat as directory, create if needed
            fs::create_dir_all(&out_path)?;
            out_path.join(&tarball_name)
        }
    } else {
        env::current_dir()?.join(&tarball_name)
    };

    // Count files that will be included
    let file_count = count_files(&plugin_dir, include_binaries)?;
    println!("  Files to pack: {}", file_count);

    if dry_run {
        println!();
        println!("[DRY RUN] Would create: {}", output_path.display());
        println!();
        println!("Contents would include:");
        list_files(&plugin_dir, include_binaries, 10)?;
        return Ok(());
    }

    // Create tarball
    println!("  Creating tarball...");
    create_tarball(&plugin_dir, &output_path, include_binaries)?;

    // Calculate checksum
    let checksum = calculate_checksum(&output_path)?;

    // Get file size
    let metadata = fs::metadata(&output_path)?;
    let size_bytes = metadata.len();
    let size_display = format_size(size_bytes);

    println!();
    println!("Package created successfully!");
    println!();
    println!("  Output: {}", output_path.display());
    println!("  Size: {}", size_display);
    println!("  Checksum: {}", checksum);
    println!();
    println!("To publish this package:");
    println!("  unrealpm publish {}", plugin_dir.display());
    println!();
    println!("To install locally (for testing):");
    println!("  unrealpm install --tarball {}", output_path.display());

    Ok(())
}

fn create_tarball(source_dir: &Path, output_path: &Path, include_binaries: bool) -> Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tar_gz = File::create(output_path)?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);

    // Get the plugin name from the source directory
    let plugin_name = source_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Could not determine plugin name"))?;

    // Walk the directory and add files
    for entry in walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_entry(|e| should_include_entry(e, include_binaries))
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let relative_path = path.strip_prefix(source_dir)?;
            let archive_path = PathBuf::from(plugin_name).join(relative_path);

            tar.append_path_with_name(path, &archive_path)?;
        }
    }

    tar.finish()?;
    Ok(())
}

fn should_include_entry(entry: &walkdir::DirEntry, include_binaries: bool) -> bool {
    let path = entry.path();
    let path_str = path.to_string_lossy();

    // Exclude patterns
    let exclude_patterns = vec![
        // Version control
        ".git",
        ".gitignore",
        ".gitattributes",
        ".gitmodules",
        ".svn",
        ".hg",
        // CI/CD
        ".gitlab-ci.yml",
        ".github",
        ".travis.yml",
        ".circleci",
        "azure-pipelines.yml",
        "Jenkinsfile",
        // IDE/Editor
        ".vs",
        ".vscode",
        ".idea",
        ".claude",
        "*.code-workspace",
        // Environment/Secrets
        ".env",
        ".env.local",
        ".env.development",
        ".env.production",
        "*.pem",
        "*.key",
        "credentials.json",
        "secrets.json",
        // Unreal build artifacts
        "Intermediate",
        "Saved",
        "DerivedDataCache",
        "Build",
        // Project files
        "*.sln",
        "*.suo",
        "*.user",
        "*.log",
        // OS files
        ".DS_Store",
        "Thumbs.db",
        "desktop.ini",
        // Documentation
        "CLAUDE.md",
        "CONTRIBUTING.md",
        "CHANGELOG.md",
        // Tooling
        "node_modules",
        "__pycache__",
        ".pytest_cache",
        // Backup files
        "*.bak",
        "*.tmp",
        "*.swp",
        "*~",
    ];

    // Check binaries
    if !include_binaries && path_str.contains("Binaries") {
        return false;
    }

    // Check against exclude patterns
    for pattern in exclude_patterns {
        if path_str.contains(pattern) {
            return false;
        }
    }

    true
}

fn calculate_checksum(file_path: &Path) -> Result<String> {
    let mut file = File::open(file_path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

fn count_files(source_dir: &Path, include_binaries: bool) -> Result<usize> {
    let count = walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_entry(|e| should_include_entry(e, include_binaries))
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .count();
    Ok(count)
}

fn list_files(source_dir: &Path, include_binaries: bool, max_files: usize) -> Result<()> {
    let plugin_name = source_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("plugin");

    let mut count = 0;
    let mut total = 0;

    for entry in walkdir::WalkDir::new(source_dir)
        .into_iter()
        .filter_entry(|e| should_include_entry(e, include_binaries))
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            total += 1;
            if count < max_files {
                let relative_path = path.strip_prefix(source_dir)?;
                let archive_path = PathBuf::from(plugin_name).join(relative_path);
                println!("    {}", archive_path.display());
                count += 1;
            }
        }
    }

    if total > max_files {
        println!("    ... and {} more files", total - max_files);
    }

    Ok(())
}

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
