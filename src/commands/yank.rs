use anyhow::Result;
use unrealpm::{Config, RegistryClient};

pub fn run(package: String, unyank: bool) -> Result<()> {
    let action = if unyank { "Unyanking" } else { "Yanking" };
    println!("{} package...", action);
    println!();

    // Load config
    let config = Config::load()?;
    let registry = RegistryClient::from_config(&config)?;

    // Parse package@version
    let (package_name, version) = if package.contains('@') {
        let parts: Vec<&str> = package.splitn(2, '@').collect();
        (parts[0].to_string(), parts[1].to_string())
    } else {
        anyhow::bail!("Please specify version: <package>@<version>");
    };

    // Explain what yanking means
    if !unyank {
        println!("Yanking {}@{}", package_name, version);
        println!();
        println!("What yanking does:");
        println!("  • Prevents NEW projects from installing this version");
        println!("  • Existing projects with this version in lockfile can still use it");
        println!("  • Package remains in registry (not deleted)");
        println!("  • You can un-yank later if needed");
        println!();
        println!("Use this when:");
        println!("  • Version has a critical bug");
        println!("  • Version has security vulnerability");
        println!("  • You want to deprecate a version");
        println!();
    } else {
        println!("Un-yanking {}@{}", package_name, version);
        println!("  This will allow users to install this version again.");
        println!();
    }

    print!("Continue? (yes/no): ");
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut confirmation = String::new();
    std::io::stdin().read_line(&mut confirmation)?;
    let confirmation = confirmation.trim().to_lowercase();

    if confirmation != "yes" {
        println!("{} cancelled.", action);
        return Ok(());
    }

    println!();
    println!("{}...", action);

    // Make HTTP request to registry
    match &registry {
        RegistryClient::Http(http_client) => {
            http_client.yank(&package_name, &version, unyank)?;
        }
        RegistryClient::File(_) => {
            anyhow::bail!("Yank is only supported for HTTP registries");
        }
    }

    if unyank {
        println!("✓ Successfully un-yanked {}@{}", package_name, version);
        println!("  Users can now install this version again.");
    } else {
        println!("✓ Successfully yanked {}@{}", package_name, version);
        println!("  This version can no longer be installed by new projects.");
        println!();
        println!(
            "To reverse this: unrealpm unyank {}@{}",
            package_name, version
        );
    }

    Ok(())
}
