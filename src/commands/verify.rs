use anyhow::Result;
use unrealpm::{verify_signature, Config, RegistryClient};

pub fn run(package_spec: String) -> Result<()> {
    // Parse package spec (e.g., "awesome-plugin" or "awesome-plugin@1.2.0")
    let (package_name, version_spec) = if let Some(pos) = package_spec.find('@') {
        let (name, version) = package_spec.split_at(pos);
        (name.to_string(), Some(version[1..].to_string()))
    } else {
        (package_spec.to_string(), None)
    };

    println!("Verifying package: {}", package_name);
    if let Some(ref ver) = version_spec {
        println!("  Version: {}", ver);
    }
    println!();

    // Get registry client from config
    let config = Config::load()?;
    let registry = RegistryClient::from_config(&config)?;

    // Get package metadata
    let metadata = registry.get_package(&package_name)?;

    // Determine which version to verify
    let version_to_verify = if let Some(ver) = version_spec {
        ver
    } else {
        // Use installed version from lockfile
        let lockfile = unrealpm::Lockfile::load()?;
        if let Some(lf) = lockfile {
            if let Some(pkg) = lf.get_package(&package_name) {
                pkg.version.clone()
            } else {
                // Use latest version from registry
                metadata
                    .versions
                    .last()
                    .ok_or_else(|| {
                        anyhow::anyhow!("No versions available for package '{}'", package_name)
                    })?
                    .version
                    .clone()
            }
        } else {
            // No lockfile, use latest from registry
            metadata
                .versions
                .last()
                .ok_or_else(|| {
                    anyhow::anyhow!("No versions available for package '{}'", package_name)
                })?
                .version
                .clone()
        }
    };

    // Find the version in metadata
    let package_version = metadata
        .versions
        .iter()
        .find(|v| v.version == version_to_verify)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Version {} not found for package '{}'",
                version_to_verify,
                package_name
            )
        })?;

    println!("Verifying {}@{}...", package_name, version_to_verify);
    println!();

    // Check if package is signed
    if package_version.public_key.is_none() {
        println!("✗ Package is NOT signed");
        println!();
        println!("This package was published without a signature.");
        println!("Consider requesting the author to republish with signing enabled.");
        println!();
        return Ok(());
    }

    let public_key = package_version.public_key.as_ref().unwrap();

    println!("Package information:");
    println!("  Public key: {}", public_key);
    if let Some(ref signed_at) = package_version.signed_at {
        println!("  Signed at: {}", signed_at);
    }
    println!();

    // Check if signature file exists
    let sig_path = registry.get_signature_path(&package_name, &version_to_verify);
    if !sig_path.exists() {
        println!("✗ Signature file not found");
        println!("  Expected: {}", sig_path.display());
        println!();
        println!("The package metadata indicates it's signed, but the signature file is missing.");
        println!("This could indicate a problem with the registry.");
        println!();
        return Ok(());
    }

    println!("  ✓ Signature file exists: {}", sig_path.display());

    // Check if tarball exists
    let tarball_path = registry.get_tarball_path(&package_name, &version_to_verify);
    if !tarball_path.exists() {
        println!("✗ Tarball not found");
        println!("  Expected: {}", tarball_path.display());
        println!();
        return Ok(());
    }

    println!("  ✓ Tarball exists: {}", tarball_path.display());
    println!();

    // Read files
    println!("Reading files...");
    let tarball_bytes = std::fs::read(&tarball_path)?;
    let signature_bytes = std::fs::read(&sig_path)?;

    println!("  Tarball size: {} bytes", tarball_bytes.len());
    println!("  Signature size: {} bytes", signature_bytes.len());
    println!();

    // Verify signature
    println!("Verifying signature...");
    let is_valid = verify_signature(&tarball_bytes, &signature_bytes, public_key)?;

    if is_valid {
        println!("  ✓ SIGNATURE VALID");
        println!();
        println!(
            "✓ Package {}@{} is authentic and has not been tampered with",
            package_name, version_to_verify
        );
        println!();
        println!("Publisher public key: {}", public_key);
        println!();
    } else {
        println!("  ✗ SIGNATURE INVALID");
        println!();
        println!("⚠  WARNING: Package signature verification FAILED!");
        println!();
        println!("This package may have been tampered with or the signature is corrupted.");
        println!("DO NOT install this package!");
        println!();
        println!("Actions:");
        println!("  • Contact the package author immediately");
        println!("  • Report this to the UnrealPM team");
        println!("  • Do not use this package in production");
        println!();
        std::process::exit(1);
    }

    Ok(())
}
