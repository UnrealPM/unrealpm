use anyhow::Result;
use unrealpm::{Config, RegistryClient};

pub fn run(package: String, version: Option<String>) -> Result<()> {
    println!("Unpublishing package...");
    println!();

    // Load config
    let config = Config::load()?;
    let registry = RegistryClient::from_config(&config)?;

    // Determine what to unpublish
    let (package_name, version_to_unpublish) = if let Some(v) = version {
        (package, Some(v))
    } else if package.contains('@') {
        let parts: Vec<&str> = package.splitn(2, '@').collect();
        (parts[0].to_string(), Some(parts[1].to_string()))
    } else {
        (package, None)
    };

    // Confirm with user
    if let Some(ref v) = version_to_unpublish {
        println!("⚠ You are about to unpublish {}@{}", package_name, v);
        println!("  This will PERMANENTLY DELETE this version from the registry.");
        println!("  Users will no longer be able to install it.");
    } else {
        println!("⚠ You are about to unpublish ALL versions of {}", package_name);
        println!("  This will PERMANENTLY DELETE the entire package from the registry.");
        println!("  This action CANNOT be undone!");
    }
    println!();
    print!("Are you sure? (yes/no): ");
    std::io::Write::flush(&mut std::io::stdout())?;

    let mut confirmation = String::new();
    std::io::stdin().read_line(&mut confirmation)?;
    let confirmation = confirmation.trim().to_lowercase();

    if confirmation != "yes" {
        println!("Unpublish cancelled.");
        return Ok(());
    }

    println!();
    println!("Unpublishing...");

    // Make HTTP request to registry
    match &registry {
        RegistryClient::Http(http_client) => {
            http_client.unpublish(&package_name, version_to_unpublish.as_deref())?;
        }
        RegistryClient::File(_) => {
            anyhow::bail!("Unpublish is only supported for HTTP registries");
        }
    }

    if let Some(v) = version_to_unpublish {
        println!("✓ Successfully unpublished {}@{}", package_name, v);
    } else {
        println!("✓ Successfully unpublished all versions of {}", package_name);
    }

    Ok(())
}
