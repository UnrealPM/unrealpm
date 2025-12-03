use anyhow::Result;
use std::env;
use unrealpm::{Manifest, UProject};

pub fn run() -> Result<()> {
    let current_dir = env::current_dir()?;

    // Check if unrealpm.json already exists
    if Manifest::exists(&current_dir) {
        println!("✓ unrealpm.json already exists in this directory");
        println!();
        println!("To reinitialize, delete unrealpm.json and run 'unrealpm init' again.");
        return Ok(());
    }

    println!("Initializing UnrealPM project...");
    println!();

    // Try to find .uproject file
    let uproject_path = match UProject::find(&current_dir) {
        Ok(path) => {
            let project_name = UProject::name(&path).unwrap_or_else(|| "UnrealProject".to_string());
            println!("✓ Found Unreal project: {}", project_name);
            Some(path)
        }
        Err(_) => {
            println!("⚠ No .uproject file found in current directory");
            println!(
                "  You can still use UnrealPM, but it works best in an Unreal Engine project."
            );
            None
        }
    };

    // Create manifest
    let mut manifest = Manifest::new();

    // If we found a .uproject, extract some info from it
    if let Some(path) = uproject_path {
        if let Ok(uproject) = UProject::load(&path) {
            manifest.description = uproject.description;
            manifest.engine_version = Some(uproject.engine_association.clone());

            println!("  Engine version: {}", uproject.engine_association);

            if !uproject.plugins.is_empty() {
                println!("  Found {} existing plugins", uproject.plugins.len());
            }
        }
    }

    // Save the manifest
    manifest.save(&current_dir)?;

    println!();
    println!("✓ Created unrealpm.json");
    println!();
    println!("Next steps:");
    println!("  • Add dependencies: unrealpm install <package>");
    println!("  • View installed packages: unrealpm list");
    println!();

    Ok(())
}
