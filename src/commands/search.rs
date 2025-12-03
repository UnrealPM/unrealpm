use anyhow::Result;
use unrealpm::{Config, RegistryClient};

pub fn run(query: String) -> Result<()> {
    println!("Searching for: {}", query);
    println!();

    // Get registry client (uses HTTP if configured)
    let config = Config::load()?;
    let registry = RegistryClient::from_config(&config)?;
    let results = registry.search(&query)?;

    if results.is_empty() {
        println!("No packages found matching '{}'", query);
        println!();
        println!("Try a different search term or check the registry.");
        return Ok(());
    }

    println!(
        "Found {} package{}:",
        results.len(),
        if results.len() == 1 { "" } else { "s" }
    );
    for package_name in &results {
        // Try to get metadata to show description
        if let Ok(metadata) = registry.get_package(package_name) {
            if let Some(desc) = metadata.description {
                println!("  {} - {}", package_name, desc);
            } else {
                println!("  {}", package_name);
            }
        } else {
            println!("  {}", package_name);
        }
    }
    println!();

    Ok(())
}
