use anyhow::Result;
use std::env;
use unrealpm::{find_matching_version, Config, Lockfile, Manifest, RegistryClient};

pub fn run() -> Result<()> {
    let current_dir = env::current_dir()?;

    println!("Checking for outdated packages...");
    println!();

    // Check if manifest exists
    if !Manifest::exists(&current_dir) {
        println!("✗ No unrealpm.json found in current directory");
        println!();
        println!("Run 'unrealpm init' first to initialize the project.");
        return Ok(());
    }

    // Load manifest and lockfile
    let manifest = Manifest::load(&current_dir)?;
    let lockfile = Lockfile::load()?;

    if manifest.dependencies.is_empty() {
        println!("No dependencies to check.");
        println!();
        return Ok(());
    }

    let lockfile = match lockfile {
        Some(lf) => lf,
        None => {
            println!("✗ No lockfile found (unrealpm.lock)");
            println!();
            println!("Run 'unrealpm install' first to install dependencies.");
            return Ok(());
        }
    };

    // Get engine version
    let engine_version = manifest.engine_version.as_deref();

    // Get registry client (uses HTTP if configured)
    let config = Config::load()?;
    let registry = RegistryClient::from_config(&config)?;

    let mut outdated_packages = Vec::new();

    // Check each dependency
    for (name, constraint) in &manifest.dependencies {
        // Get current installed version from lockfile
        let current_version = match lockfile.get_package(name) {
            Some(pkg) => &pkg.version,
            None => {
                eprintln!("  ⚠ Package '{}' not found in lockfile", name);
                continue;
            }
        };

        // Get package metadata
        let metadata = match registry.get_package(name) {
            Ok(meta) => meta,
            Err(e) => {
                eprintln!("  ✗ Failed to fetch metadata for '{}': {}", name, e);
                continue;
            }
        };

        // Find latest matching version
        let latest_version = match find_matching_version(&metadata, constraint, engine_version, false) {
            Ok(ver) => ver,
            Err(e) => {
                eprintln!("  ✗ Failed to resolve version for '{}': {}", name, e);
                continue;
            }
        };

        // Compare versions
        if current_version != &latest_version.version {
            outdated_packages.push((name.clone(), current_version.clone(), latest_version.version.clone(), constraint.clone()));
        }
    }

    // Display results
    if outdated_packages.is_empty() {
        println!("✓ All packages are up to date!");
        println!();
    } else {
        println!("Found {} outdated packages:", outdated_packages.len());
        println!();

        // Print table header
        println!(
            "{:<30} {:<15} {:<15} {:<20}",
            "Package",
            "Current",
            "Latest",
            "Constraint"
        );
        println!("{}", "-".repeat(80));

        // Print outdated packages
        for (name, current, latest, constraint) in outdated_packages {
            println!(
                "{:<30} {:<15} {:<15} {:<20}",
                name,
                current,
                latest,
                constraint
            );
        }

        println!();
        println!("Run 'unrealpm update' to update all packages");
        println!("Run 'unrealpm update <package>' to update a specific package");
        println!();
    }

    Ok(())
}
