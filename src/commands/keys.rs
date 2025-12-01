use anyhow::Result;
use std::path::PathBuf;
use unrealpm::{Config, PackageSigningKey};

pub fn run(action: &crate::KeysAction) -> Result<()> {
    match action {
        crate::KeysAction::Generate => generate(),
        crate::KeysAction::Show => show(),
    }
}

fn generate() -> Result<()> {
    println!("Generating new signing keys...");
    println!();

    // Load config to get key paths
    let config = Config::load()?;

    // Expand tilde in paths
    let private_key_path = PathBuf::from(shellexpand::tilde(&config.signing.private_key_path).to_string());
    let public_key_path = PathBuf::from(shellexpand::tilde(&config.signing.public_key_path).to_string());

    // Check if keys already exist
    if private_key_path.exists() || public_key_path.exists() {
        println!("⚠  Signing keys already exist!");
        println!();
        println!("Existing keys:");
        if private_key_path.exists() {
            println!("  • Private key: {}", private_key_path.display());
        }
        if public_key_path.exists() {
            println!("  • Public key: {}", public_key_path.display());
        }
        println!();
        println!("To regenerate keys:");
        println!("  1. Back up your existing keys");
        println!("  2. Delete the old keys");
        println!("  3. Run 'unrealpm keys generate' again");
        println!();
        println!("⚠  WARNING: Regenerating keys will invalidate all previously signed packages!");
        println!();
        return Ok(());
    }

    // Generate new keys
    println!("Generating Ed25519 keypair...");
    let keys = PackageSigningKey::generate()?;

    // Save to files
    keys.save_to_files(&private_key_path, &public_key_path)?;

    println!("  ✓ Private key saved to {}", private_key_path.display());
    println!("  ✓ Public key saved to {}", public_key_path.display());
    println!();

    // Show security warning
    println!("⚠  IMPORTANT: Keep your private key safe!");
    println!("  • Never commit it to version control");
    println!("  • Back it up securely (encrypted backup recommended)");
    println!("  • Don't share it with anyone");
    println!("  • If compromised, all your signed packages are at risk");
    println!();

    // Show public key
    println!("Your public key (share with users):");
    println!("  {}", keys.public_key_hex());
    println!();

    println!("✓ Keys generated successfully!");
    println!();
    println!("Next steps:");
    println!("  • Publish packages: unrealpm publish");
    println!("  • View public key: unrealpm keys show");
    println!();

    Ok(())
}

fn show() -> Result<()> {
    println!("Signing Keys:");
    println!();

    // Load config to get key paths
    let config = Config::load()?;

    // Expand tilde in paths
    let private_key_path = PathBuf::from(shellexpand::tilde(&config.signing.private_key_path).to_string());
    let public_key_path = PathBuf::from(shellexpand::tilde(&config.signing.public_key_path).to_string());

    // Check if keys exist
    if !private_key_path.exists() && !public_key_path.exists() {
        println!("✗ No signing keys found");
        println!();
        println!("Generate keys with:");
        println!("  unrealpm keys generate");
        println!();
        return Ok(());
    }

    // Show key paths
    println!("Key locations:");
    println!("  Private key: {}", private_key_path.display());
    println!("  Public key:  {}", public_key_path.display());
    println!();

    // Show key status
    println!("Status:");
    if private_key_path.exists() {
        println!("  ✓ Private key exists");

        // Check permissions (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std::fs::metadata(&private_key_path)?;
            let mode = metadata.permissions().mode();
            let perms = mode & 0o777;

            if perms == 0o600 {
                println!("    Permissions: {} (secure)", format!("{:o}", perms));
            } else {
                println!("    Permissions: {} ⚠ WARNING: Should be 600!", format!("{:o}", perms));
            }
        }
    } else {
        println!("  ✗ Private key not found");
    }

    if public_key_path.exists() {
        println!("  ✓ Public key exists");
    } else {
        println!("  ✗ Public key not found");
    }
    println!();

    // Load and display public key if it exists
    if private_key_path.exists() && public_key_path.exists() {
        match PackageSigningKey::load_from_files(&private_key_path, &public_key_path) {
            Ok(keys) => {
                println!("Public Key (share this with users):");
                println!("  {}", keys.public_key_hex());
                println!();
                println!("Signing: {}", if config.signing.enabled { "Enabled ✓" } else { "Disabled ✗" });
                println!();
            }
            Err(e) => {
                println!("✗ Failed to load keys: {}", e);
                println!();
            }
        }
    } else {
        println!("⚠  Keys are incomplete. Regenerate with:");
        println!("  unrealpm keys generate");
        println!();
    }

    Ok(())
}
