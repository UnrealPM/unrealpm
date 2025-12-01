use anyhow::Result;
use std::env;
use std::fs;
use unrealpm::{Lockfile, Manifest};

pub fn run(package: String) -> Result<()> {
    let current_dir = env::current_dir()?;

    println!("Uninstalling package: {}", package);
    println!();

    // Check if manifest exists
    if !Manifest::exists(&current_dir) {
        println!("✗ No unrealpm.json found in current directory");
        println!();
        println!("Run 'unrealpm init' first to initialize the project.");
        return Ok(());
    }

    // Load manifest
    let mut manifest = Manifest::load(&current_dir)?;

    // Check if package is in manifest
    if !manifest.dependencies.contains_key(&package) {
        println!("⚠ Package '{}' is not in dependencies", package);
        println!();
        println!("Currently installed packages:");
        for (name, version) in &manifest.dependencies {
            println!("  - {}@{}", name, version);
        }
        return Ok(());
    }

    // Remove from Plugins/ directory
    let plugin_path = current_dir.join("Plugins").join(&package);
    if plugin_path.exists() {
        println!("  Removing from Plugins/...");
        fs::remove_dir_all(&plugin_path)?;
        println!("  ✓ Removed {}", plugin_path.display());
    } else {
        println!("  ⚠ Plugin directory not found at {}", plugin_path.display());
        println!("  (continuing with manifest/lockfile cleanup)");
    }

    // Remove from manifest
    println!("  Updating manifest...");
    manifest.dependencies.remove(&package);
    manifest.save(&current_dir)?;
    println!("  ✓ Removed from unrealpm.json");

    // Remove from lockfile if it exists
    if let Ok(Some(mut lockfile)) = Lockfile::load() {
        println!("  Updating lockfile...");
        lockfile.remove_package(&package);
        lockfile.save()?;
        println!("  ✓ Removed from unrealpm.lock");
    }

    println!();
    println!("✓ Successfully uninstalled {}", package);
    println!();

    Ok(())
}
