use anyhow::Result;
use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use unrealpm::{Config, PackageMetadata, PackageType, PackageVersion, RegistryClient, UPlugin};
use unrealpm::signing::load_or_generate_keys;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Sha256, Digest};
use chrono::Utc;

pub fn run(
    path: Option<String>,
    dry_run: bool,
    include_binaries: bool,
    target_engine: Option<String>,
    git_repo: Option<String>,
    git_ref: Option<String>,
) -> Result<()> {
    println!("Publishing package...");
    println!();

    // Parse target engine version if provided
    let (engine_major, engine_minor, engine_patch, is_multi_engine) = if let Some(ref eng) = target_engine {
        // Parse engine version (e.g., "5.3", "4.27", "5.4.2")
        let parts: Vec<&str> = eng.split('.').collect();
        let major = parts.get(0)
            .and_then(|s| s.parse::<i32>().ok())
            .ok_or_else(|| anyhow::anyhow!("Invalid engine version format. Use: 4.27, 5.3, etc."))?;
        let minor = parts.get(1)
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);
        let patch = parts.get(2)
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(0);

        println!("  Target engine: UE {}.{}.{}", major, minor, patch);
        println!("  Publishing engine-specific version");
        println!();

        (Some(major), Some(minor), Some(patch), false)
    } else {
        // Multi-engine version (current behavior)
        (None, None, None, true)
    };

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

    println!("  ✓ Found plugin: {}", plugin_name);
    println!("    Version: {}", uplugin.version_name);
    println!("    Friendly name: {}", uplugin.friendly_name);
    if let Some(desc) = &uplugin.description {
        if !desc.is_empty() {
            println!("    Description: {}", desc);
        }
    }
    if let Some(engine) = &uplugin.engine_version {
        println!("    Engine version: {}", engine);
    }
    println!();

    // Check if auto-build is enabled
    let config = Config::load()?;
    if config.build.auto_build_on_publish && !include_binaries {
        println!("⚙ Auto-build enabled, building binaries...");
        println!();

        // Run build command for configured platforms
        if let Some(engine_version) = &uplugin.engine_version {
            for platform in &config.build.platforms {
                match crate::commands::build::build_for_platform(
                    &plugin_dir,
                    &plugin_name,
                    engine_version,
                    platform,
                    &config,
                ) {
                    Ok(_) => println!("  ✓ Built for {}", platform),
                    Err(e) => {
                        eprintln!("  ✗ Failed to build for {}: {}", platform, e);
                        eprintln!("  Continuing with source-only publish...");
                    }
                }
            }
            println!();
        } else {
            println!("  ⚠ No engine version in .uplugin, skipping auto-build");
            println!();
        }
    }

    // Create tarball
    println!("  Creating package tarball...");
    let tarball_name = format!("{}-{}.tar.gz", plugin_name, uplugin.version_name);
    let temp_dir = env::temp_dir().join(format!("unrealpm-publish-{}", plugin_name));
    fs::create_dir_all(&temp_dir)?;

    let tarball_path = temp_dir.join(&tarball_name);
    create_tarball(&plugin_dir, &tarball_path, include_binaries)?;

    // Calculate checksum
    println!("  Calculating checksum...");
    let checksum = calculate_checksum(&tarball_path)?;

    // Get file size
    let metadata = fs::metadata(&tarball_path)?;
    let size_mb = metadata.len() as f64 / 1024.0 / 1024.0;

    println!("  ✓ Package created");
    println!("    File: {}", tarball_name);
    println!("    Size: {:.2} MB", size_mb);
    println!("    Checksum: {}", checksum);
    println!();

    if dry_run {
        println!("--dry-run specified, skipping publish");
        println!();
        println!("Summary:");
        println!("  Package: {}@{}", plugin_name, uplugin.version_name);
        println!("  Tarball: {}", tarball_path.display());
        println!("  Ready to publish!");

        // Clean up temp directory
        fs::remove_dir_all(&temp_dir)?;
        return Ok(());
    }

    // Get registry client (uses HTTP if configured)
    let registry = RegistryClient::from_config(&config)?;

    // Check if package already exists
    if let Ok(existing) = registry.get_package(&plugin_name) {
        // Check if this version already exists for this specific engine
        let version_exists = existing.versions.iter().any(|v| {
            v.version == uplugin.version_name && {
                if is_multi_engine {
                    // Multi-engine: Check if another multi-engine version exists
                    v.is_multi_engine
                } else {
                    // Engine-specific: Check if same engine version exists
                    v.engine_major == engine_major && v.engine_minor == engine_minor
                }
            }
        });

        if version_exists {
            if is_multi_engine {
                anyhow::bail!(
                    "Version {} of package '{}' already exists in registry",
                    uplugin.version_name,
                    plugin_name
                );
            } else {
                anyhow::bail!(
                    "Version {} for engine {}.{} of package '{}' already exists in registry",
                    uplugin.version_name,
                    engine_major.unwrap(),
                    engine_minor.unwrap(),
                    plugin_name
                );
            }
        }
    }

    // Check registry type to determine publish method
    match &registry {
        RegistryClient::Http(http_client) => {
            // Publish to HTTP registry
            println!("  Publishing to HTTP registry...");
            publish_to_http(
                http_client,
                &tarball_path,
                &plugin_name,
                &uplugin,
                &checksum,
                &config,
                engine_major,
                engine_minor,
                engine_patch,
                is_multi_engine,
                git_repo.clone(),
                git_ref.clone(),
            )?;

            // Clean up temp directory
            fs::remove_dir_all(&temp_dir)?;

            println!("  ✓ Published to HTTP registry");
            println!();
            println!("✓ Successfully published {}@{}", plugin_name, uplugin.version_name);
            println!();
            println!("Install with:");
            println!("  unrealpm install {}", plugin_name);
            println!();

            return Ok(());
        }
        RegistryClient::File(_) => {
            // Continue with file-based publishing (existing code below)
        }
    }

    // Move tarball to registry (file-based only)
    println!("  Publishing to file registry...");
    let tarballs_dir = registry.get_tarballs_dir();
    fs::create_dir_all(&tarballs_dir)?;

    let final_tarball_path = tarballs_dir.join(&tarball_name);
    fs::rename(&tarball_path, &final_tarball_path)?;

    // Sign the package (if signing is enabled)
    let (public_key_hex, signed_at) = if config.signing.enabled {
        println!("  Signing package...");

        // Expand tilde in paths
        let private_key_path = PathBuf::from(shellexpand::tilde(&config.signing.private_key_path).to_string());
        let public_key_path = PathBuf::from(shellexpand::tilde(&config.signing.public_key_path).to_string());

        // Load or generate keys
        let keys = load_or_generate_keys(&private_key_path, &public_key_path)?;

        // Read tarball bytes
        let tarball_bytes = fs::read(&final_tarball_path)?;

        // Sign
        let signature = keys.sign(&tarball_bytes);

        // Save signature
        let signatures_dir = registry.get_signatures_dir();
        fs::create_dir_all(&signatures_dir)?;

        let signature_path = registry.get_signature_path(&plugin_name, &uplugin.version_name);
        fs::write(&signature_path, signature.to_bytes())?;

        let public_key_hex = keys.public_key_hex();
        let signed_at = Utc::now().to_rfc3339();

        println!("  ✓ Package signed");
        println!("    Public key: {}...", &public_key_hex[..16]);
        println!("    Signature: {}", signature_path.display());

        (Some(public_key_hex), Some(signed_at))
    } else {
        println!("  ⚠ Package signing disabled (config.signing.enabled = false)");
        (None, None)
    };

    // Create/update package metadata
    let packages_dir = registry.get_packages_dir();
    let metadata_path = packages_dir.join(format!("{}.json", plugin_name));

    let mut package_metadata = if metadata_path.exists() {
        // Load existing metadata
        let content = fs::read_to_string(&metadata_path)?;
        serde_json::from_str::<PackageMetadata>(&content)?
    } else {
        // Create new metadata
        PackageMetadata {
            name: plugin_name.clone(),
            description: uplugin.description.clone(),
            versions: vec![],
        }
    };

    // Add new version
    let package_type = if include_binaries {
        PackageType::Binary
    } else {
        PackageType::Source
    };

    let new_version = PackageVersion {
        version: uplugin.version_name.clone(),
        tarball: tarball_name.clone(),
        checksum,
        engine_versions: if is_multi_engine {
            uplugin.engine_version.as_ref().map(|v| vec![v.clone()])
        } else {
            None
        },
        engine_major,
        engine_minor,
        is_multi_engine,
        package_type,
        binaries: None, // Will be added manually or via future `publish-binary` command
        dependencies: if uplugin.plugins.is_empty() {
            None
        } else {
            Some(uplugin.plugins.iter().map(|p| unrealpm::Dependency {
                name: p.name.clone(),
                version: "*".to_string(), // Default to any version
            }).collect())
        },
        public_key: public_key_hex,
        signed_at,
    };

    package_metadata.versions.push(new_version);

    // Save metadata
    let metadata_json = serde_json::to_string_pretty(&package_metadata)?;
    fs::write(&metadata_path, metadata_json)?;

    println!("  ✓ Published to registry");
    println!();

    // Clean up temp directory
    fs::remove_dir_all(&temp_dir)?;

    println!("✓ Successfully published {}@{}", plugin_name, uplugin.version_name);
    println!();
    println!("Install with:");
    println!("  unrealpm install {}", plugin_name);
    println!();

    Ok(())
}

fn create_tarball(source_dir: &Path, output_path: &Path, include_binaries: bool) -> Result<()> {
    let tar_gz = File::create(output_path)?;
    let enc = GzEncoder::new(tar_gz, Compression::default());
    let mut tar = tar::Builder::new(enc);

    // Get the plugin name from the source directory
    let plugin_name = source_dir.file_name()
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
        ".git",
        ".gitignore",
        ".vs",
        ".vscode",
        ".idea",
        "Intermediate",
        "Saved",
        "*.sln",
        "*.suo",
        "*.user",
        "*.log",
        ".DS_Store",
    ];

    // Check if we should exclude binaries
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

/// Publish to HTTP registry
fn publish_to_http(
    http_client: &unrealpm::registry_http::HttpRegistryClient,
    tarball_path: &Path,
    plugin_name: &str,
    uplugin: &UPlugin,
    checksum: &str,
    config: &Config,
    engine_major: Option<i32>,
    engine_minor: Option<i32>,
    engine_patch: Option<i32>,
    is_multi_engine: bool,
    git_repo: Option<String>,
    git_ref: Option<String>,
) -> Result<()> {
    // Sign the package if enabled
    let (public_key, signed_at, signature_path) = if config.signing.enabled {
        println!("  Signing package...");

        let private_key_path = PathBuf::from(shellexpand::tilde(&config.signing.private_key_path).to_string());
        let public_key_path = PathBuf::from(shellexpand::tilde(&config.signing.public_key_path).to_string());

        let keys = unrealpm::load_or_generate_keys(&private_key_path, &public_key_path)?;
        let tarball_bytes = fs::read(tarball_path)?;
        let signature = keys.sign(&tarball_bytes);

        // Save signature to temp file
        let sig_path = tarball_path.with_extension("sig");
        fs::write(&sig_path, signature.to_bytes())?;

        let public_key_hex = keys.public_key_hex();
        let signed_at_str = Utc::now().to_rfc3339();

        println!("  ✓ Package signed");
        println!("    Public key: {}...", &public_key_hex[..16]);

        (Some(public_key_hex), Some(signed_at_str), Some(sig_path))
    } else {
        (None, None, None)
    };

    // Build metadata for HTTP API
    let metadata = unrealpm::registry_http::PublishMetadata {
        name: plugin_name.to_string(),
        version: uplugin.version_name.clone(),
        description: uplugin.description.clone(),
        checksum: checksum.to_string(),
        package_type: "source".to_string(), // TODO: Handle binary packages
        engine_versions: if is_multi_engine {
            uplugin.engine_version.as_ref().map(|v| vec![v.clone()])
        } else {
            None // Engine-specific versions don't use array
        },
        dependencies: if uplugin.plugins.is_empty() {
            None
        } else {
            Some(uplugin.plugins.iter().map(|p| {
                unrealpm::registry_http::DependencySpec {
                    name: p.name.clone(),
                    version: "*".to_string(),
                }
            }).collect())
        },
        public_key,
        signed_at,
        engine_major,
        engine_minor,
        engine_patch,
        is_multi_engine: Some(is_multi_engine),
        git_repository: git_repo,
        git_tag: git_ref,
    };

    // Publish via HTTP
    http_client.publish(tarball_path, signature_path.as_deref(), metadata)?;

    Ok(())
}
