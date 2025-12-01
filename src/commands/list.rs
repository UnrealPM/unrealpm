use anyhow::Result;
use std::env;
use unrealpm::Manifest;

pub fn run() -> Result<()> {
    let current_dir = env::current_dir()?;

    // Try to load the manifest
    let manifest = match Manifest::load(&current_dir) {
        Ok(m) => m,
        Err(_) => {
            println!("No unrealpm.json found in current directory.");
            println!();
            println!("Run 'unrealpm init' to initialize a project.");
            return Ok(());
        }
    };

    // Check if there are any dependencies
    let total_deps = manifest.dependencies.len() + manifest.dev_dependencies.len();

    if total_deps == 0 {
        println!("No packages installed.");
        println!();
        println!("Install packages with: unrealpm install <package>");
        return Ok(());
    }

    // Display runtime dependencies
    if !manifest.dependencies.is_empty() {
        println!("Dependencies:");
        for (name, version) in &manifest.dependencies {
            println!("  {} @ {}", name, version);
        }
        println!();
    }

    // Display dev dependencies
    if !manifest.dev_dependencies.is_empty() {
        println!("Dev Dependencies:");
        for (name, version) in &manifest.dev_dependencies {
            println!("  {} @ {}", name, version);
        }
        println!();
    }

    // Summary
    println!(
        "Total: {} package{}",
        total_deps,
        if total_deps == 1 { "" } else { "s" }
    );

    Ok(())
}
