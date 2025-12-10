use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::env;
use std::sync::Arc;
use unrealpm::{
    find_matching_version, install_package, resolve_dependencies, verify_checksum,
    verify_signature, Config, Lockfile, Manifest, PrebuiltBinary, ProgressCallback, RegistryClient,
};

/// Create an indicatif-based progress callback for CLI display
fn create_spinner_callback() -> ProgressCallback {
    let spinner = Arc::new(std::sync::Mutex::new(ProgressBar::new_spinner()));
    {
        let s = spinner.lock().unwrap();
        s.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        s.enable_steady_tick(std::time::Duration::from_millis(80));
    }

    let spinner_clone = spinner.clone();
    Arc::new(move |msg: &str, current: u64, total: u64| {
        let s = spinner_clone.lock().unwrap();
        if current >= total && total > 0 {
            s.finish_with_message(format!("✓ {}", msg));
        } else {
            s.set_message(msg.to_string());
        }
    })
}

pub fn run(
    package: Option<String>,
    force: bool,
    engine_version_override: Option<String>,
    prefer_binary: bool,
    source_only: bool,
    binary_only: bool,
    dry_run: bool,
) -> Result<()> {
    let current_dir = env::current_dir()?;

    // Determine installation mode
    let install_mode = if binary_only {
        InstallMode::BinaryOnly
    } else if source_only {
        InstallMode::SourceOnly
    } else if prefer_binary {
        InstallMode::PreferBinary
    } else {
        InstallMode::PreferSource
    };

    match package {
        Some(pkg) => install_single_package(
            &pkg,
            &current_dir,
            force,
            engine_version_override,
            install_mode,
            dry_run,
        ),
        None => install_all_dependencies(
            &current_dir,
            force,
            engine_version_override,
            install_mode,
            dry_run,
        ),
    }
}

#[derive(Debug, Clone, Copy)]
enum InstallMode {
    PreferSource, // Default: use source, ignore binaries
    PreferBinary, // Try binary first, fall back to source
    SourceOnly,   // Never use binaries
    BinaryOnly,   // Require binary, fail if not available
}

fn install_single_package(
    package_spec: &str,
    project_dir: &std::path::Path,
    force: bool,
    engine_version_override: Option<String>,
    install_mode: InstallMode,
    dry_run: bool,
) -> Result<()> {
    // Parse package spec (e.g., "awesome-plugin" or "awesome-plugin@^1.2.0")
    let (package_name, version_constraint) = if let Some(pos) = package_spec.find('@') {
        let (name, version) = package_spec.split_at(pos);
        (name.to_string(), version[1..].to_string()) // Skip the '@'
    } else {
        (package_spec.to_string(), "*".to_string()) // Default to any version
    };

    if dry_run {
        println!(
            "[DRY RUN] Would install {}@{}...",
            package_name, version_constraint
        );
    } else {
        println!("Installing {}@{}...", package_name, version_constraint);
    }
    println!();

    // Load manifest to get engine version (or use override)
    let manifest = Manifest::load(project_dir).unwrap_or_default();
    let engine_version = if let Some(ref override_version) = engine_version_override {
        println!("  Engine version: {} (overridden)", override_version);
        Some(override_version.as_str())
    } else {
        let detected = manifest.engine_version.as_deref();
        if let Some(engine) = detected {
            println!("  Engine version: {}", engine);
        }
        detected
    };

    // Get registry client (uses HTTP if configured)
    let config_for_registry = Config::load()?;
    let registry = RegistryClient::from_config(&config_for_registry)?;

    // Get package metadata with spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.blue} {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.set_message("Fetching package metadata...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let metadata = registry.get_package(&package_name)?;
    spinner.finish_and_clear();

    // Find matching version
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.blue} {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.set_message("Resolving version...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let resolved_version =
        find_matching_version(&metadata, &version_constraint, engine_version, force)?;

    if force && engine_version.is_some() {
        println!("  ⚠ WARNING: Force installing - engine compatibility not checked");
    }
    spinner.finish_with_message(format!(
        "✓ Resolved to version {}",
        resolved_version.version
    ));

    // Resolve transitive dependencies
    let mut direct_deps = std::collections::HashMap::new();
    direct_deps.insert(package_name.clone(), version_constraint.clone());

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.blue} {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.set_message("Resolving dependencies...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let all_resolved = resolve_dependencies(&direct_deps, &registry, engine_version, force)?;

    let dep_count = all_resolved.len();
    if dep_count > 1 {
        spinner.finish_with_message(format!(
            "✓ Resolved {} packages (including {} dependencies)",
            dep_count,
            dep_count - 1
        ));
    } else {
        spinner.finish_with_message("✓ No additional dependencies");
    }

    // Install dependencies first (before the main package)
    let mut lockfile = Lockfile::load()?.unwrap_or_default();

    for (dep_name, resolved_pkg) in &all_resolved {
        if dep_name == &package_name {
            continue; // Skip the main package, we'll install it with full verification below
        }

        // Check if already installed
        if let Some(locked) = lockfile.get_package(dep_name) {
            if locked.version == resolved_pkg.version {
                println!("  ✓ {} {} (already installed)", dep_name, resolved_pkg.version);
                continue;
            }
        }

        println!("  Installing dependency {}@{}...", dep_name, resolved_pkg.version);

        // Download if HTTP registry
        let dep_tarball = match &registry {
            unrealpm::RegistryClient::Http(http_client) => {
                http_client.download_if_needed(dep_name, &resolved_pkg.version, &resolved_pkg.checksum)?
            }
            unrealpm::RegistryClient::File(_) => registry.get_tarball_path(dep_name, &resolved_pkg.version),
        };

        // Verify checksum
        verify_checksum(&dep_tarball, &resolved_pkg.checksum, None)?;

        // Install
        install_package(&dep_tarball, &project_dir.to_path_buf(), dep_name, None)?;

        // Update lockfile
        lockfile.update_package(
            dep_name.clone(),
            resolved_pkg.version.clone(),
            resolved_pkg.checksum.clone(),
            resolved_pkg.dependencies.clone(),
        );

        println!("  ✓ Installed {}", dep_name);
    }

    // Determine which tarball to use (binary or source)
    let (tarball_path, checksum, install_type) = select_installation_source(
        &resolved_version,
        &registry,
        &package_name,
        engine_version,
        install_mode,
    )?;

    if let Some(ref itype) = install_type {
        println!("  Using: {}", itype);
    }

    if dry_run {
        // Dry run: show what would happen without actually doing it
        if resolved_version.public_key.is_some() {
            println!("  [DRY RUN] Would verify signature");
        }
        println!("  [DRY RUN] Would verify checksum: {}", checksum);
        println!(
            "  [DRY RUN] Would install to: {}/Plugins/{}",
            project_dir.display(),
            package_name
        );

        // Check if auto-build would be triggered
        let config = Config::load()?;
        let was_source_install = install_type.as_ref().is_none_or(|t| t.contains("source"));

        if config.build.auto_build_on_install && was_source_install && engine_version.is_some() {
            println!(
                "  [DRY RUN] Would auto-build binaries for {}",
                unrealpm::detect_platform()
            );
        }

        println!("  [DRY RUN] Would update manifest (unrealpm.json)");
        println!("  [DRY RUN] Would update lockfile (unrealpm.lock)");
        println!();
        println!(
            "[DRY RUN] Would successfully install {}@{}",
            package_name, resolved_version.version
        );
        println!();
        return Ok(());
    }

    // Download if using HTTP registry (cache-first) - BEFORE signature verification
    let tarball_path = match &registry {
        unrealpm::RegistryClient::Http(http_client) => {
            http_client.download_if_needed(&package_name, &resolved_version.version, &checksum)?
        }
        unrealpm::RegistryClient::File(_) => tarball_path,
    };

    // Load config for verification settings
    let config = Config::load()?;

    // Verify signature (if package is signed)
    if let Some(public_key) = &resolved_version.public_key {
        println!("  Verifying signature...");

        // Download signature from registry (or get local path for file registry)
        match registry.download_signature(&package_name, &resolved_version.version) {
            Ok(sig_path) => {
                // Read tarball and signature
                let tarball_bytes = std::fs::read(&tarball_path)?;
                let signature_bytes = std::fs::read(&sig_path)?;

                // Verify
                let is_valid = verify_signature(&tarball_bytes, &signature_bytes, public_key)?;

                if !is_valid {
                    anyhow::bail!(
                        "Signature verification FAILED for {}@{}\n\n\
                        The package signature is invalid. This could mean:\n\
                        • The package has been tampered with\n\
                        • The signature file is corrupted\n\
                        • The public key doesn't match\n\n\
                        For your security, installation has been aborted.\n\
                        If you trust this package, you can:\n\
                        • Contact the package author\n\
                        • Disable signature requirement: unrealpm config set verification.require_signatures false",
                        package_name,
                        resolved_version.version
                    );
                }

                println!(
                    "  ✓ Signature verified (publisher: {}...)",
                    &public_key[..16]
                );
            }
            Err(_) => {
                // Signature download failed or file missing
                if config.verification.require_signatures {
                    anyhow::bail!(
                        "Signature verification required but signature could not be retrieved for {}@{}\n\n\
                        This package is marked as signed but the signature file is not available.\n\n\
                        Solutions:\n\
                        • Disable signature requirement: unrealpm config set verification.require_signatures false\n\
                        • Contact the package author to republish with a valid signature",
                        package_name,
                        resolved_version.version
                    );
                } else {
                    println!("  ⚠ Signature not available (package marked as signed)");
                }
            }
        }
    } else {
        // Package is not signed
        if config.verification.require_signatures {
            anyhow::bail!(
                "Signature verification required but package '{}@{}' is not signed\n\n\
                Solutions:\n\
                • Disable signature requirement: unrealpm config set verification.require_signatures false\n\
                • Request the package author to publish signed packages\n\
                • Use a different package version that is signed",
                package_name,
                resolved_version.version
            );
        }
    }

    // Verify checksum with progress spinner
    let progress = Some(create_spinner_callback());
    verify_checksum(&tarball_path, &checksum, progress)?;

    // Install package with progress spinner
    let progress = Some(create_spinner_callback());
    let installed_path = install_package(
        &tarball_path,
        &project_dir.to_path_buf(),
        &package_name,
        progress,
    )?;
    println!("  ✓ Installed to {}", installed_path.display());

    // Check if we should auto-build binaries (config already loaded above)
    let was_source_install = install_type.as_ref().is_none_or(|t| t.contains("source"));

    if let Some(engine_ver) = engine_version {
        if config.build.auto_build_on_install && was_source_install {
            println!();
            println!("⚙ Auto-build enabled, building binaries...");
            println!();

            let current_platform = unrealpm::detect_platform();
            match crate::commands::build::build_for_platform(
                &installed_path,
                &package_name,
                engine_ver,
                &current_platform,
                &config,
            ) {
                Ok(_) => println!("  ✓ Built for {}", current_platform),
                Err(e) => {
                    eprintln!("  ✗ Build failed: {}", e);
                    eprintln!("  Plugin installed as source-only");
                }
            }
            println!();
        }
    }

    // Update manifest (preserve engine version from earlier load)
    println!("  Updating manifest...");
    let mut manifest = Manifest::load(project_dir).unwrap_or_default();
    manifest
        .dependencies
        .insert(package_name.clone(), version_constraint.clone());
    manifest.save(project_dir)?;

    // Update lockfile with main package (dependencies already added earlier)
    println!("  Updating lockfile...");
    // Get the resolved info for the main package from all_resolved
    if let Some(main_pkg) = all_resolved.get(&package_name) {
        lockfile.update_package(
            package_name.clone(),
            main_pkg.version.clone(),
            main_pkg.checksum.clone(),
            main_pkg.dependencies.clone(),
        );
    }
    lockfile.save()?;
    println!("  ✓ Lockfile updated");

    println!();
    println!(
        "✓ Successfully installed {}@{}",
        package_name, resolved_version.version
    );
    println!();

    Ok(())
}

fn install_all_dependencies(
    project_dir: &std::path::Path,
    force: bool,
    engine_version_override: Option<String>,
    _install_mode: InstallMode,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        println!("[DRY RUN] Would install all dependencies from manifest...");
    } else {
        println!("Installing all dependencies from manifest...");
    }
    println!();

    // Load manifest
    let manifest = Manifest::load(project_dir)?;

    if manifest.dependencies.is_empty() {
        println!("No dependencies to install.");
        println!();
        println!("Add dependencies with: unrealpm install <package>");
        return Ok(());
    }

    println!("Found {} direct dependencies", manifest.dependencies.len());
    println!();

    // Get registry client (uses HTTP if configured)
    let config_for_registry = Config::load()?;
    let registry = RegistryClient::from_config(&config_for_registry)?;

    // Get engine version for filtering (or use override)
    let engine_version = if let Some(ref override_version) = engine_version_override {
        println!("Engine version: {} (overridden)", override_version);
        Some(override_version.as_str())
    } else {
        let detected = manifest.engine_version.as_deref();
        if let Some(engine) = detected {
            println!("Engine version: {}", engine);
        }
        detected
    };

    // Resolve all transitive dependencies with spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.blue} {msg}")
            .unwrap()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.set_message("Resolving dependency tree...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let resolved = resolve_dependencies(&manifest.dependencies, &registry, engine_version, force)?;

    if force && engine_version.is_some() {
        println!("⚠ WARNING: Force installing - engine compatibility not checked");
        println!();
    }
    spinner.finish_with_message(format!(
        "✓ Resolved {} total packages (including transitive dependencies)",
        resolved.len()
    ));
    println!();

    if dry_run {
        // Dry run: show what would be installed
        println!("[DRY RUN] Would install the following packages:");
        println!();
        for (name, resolved_pkg) in &resolved {
            println!("  - {}@{}", name, resolved_pkg.version);
            if let Some(deps) = &resolved_pkg.dependencies {
                if !deps.is_empty() {
                    println!("    Dependencies:");
                    for (dep_name, dep_version) in deps {
                        println!("      - {}@{}", dep_name, dep_version);
                    }
                }
            }
        }
        println!();
        println!("[DRY RUN] Would update lockfile (unrealpm.lock)");
        println!();
        println!(
            "[DRY RUN] Would successfully install {} packages",
            resolved.len()
        );
        println!();
        return Ok(());
    }

    // Load or create lockfile
    let mut lockfile = Lockfile::load()?.unwrap_or_default();

    // Create a progress bar for package installation
    let pb = ProgressBar::new(resolved.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("[{bar:40.cyan/blue}] {pos}/{len} packages")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Install each resolved package
    for (name, resolved_pkg) in &resolved {
        pb.set_message(format!("Installing {}@{}", name, resolved_pkg.version));

        // Get tarball path
        let tarball_path = registry.get_tarball_path(name, &resolved_pkg.version);

        // Verify checksum (no spinner for batch installs - we have a progress bar)
        match verify_checksum(&tarball_path, &resolved_pkg.checksum, None) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("  ✗ Checksum verification failed for {}: {}", name, e);
                eprintln!("  Skipping package...");
                eprintln!();
                continue;
            }
        }

        // Install package (no spinner for batch installs)
        match install_package(&tarball_path, &project_dir.to_path_buf(), name, None) {
            Ok(_installed_path) => {
                // Update lockfile
                lockfile.update_package(
                    name.clone(),
                    resolved_pkg.version.clone(),
                    resolved_pkg.checksum.clone(),
                    resolved_pkg.dependencies.clone(),
                );
                pb.inc(1);
            }
            Err(e) => {
                pb.println(format!("  ✗ Failed to install {}: {}", name, e));
                pb.println("  Continuing with remaining packages...");
                pb.inc(1);
            }
        }
    }

    pb.finish_with_message("✓ All packages processed");

    // Save lockfile
    lockfile.save()?;
    println!("  ✓ Lockfile updated");
    println!();

    println!("✓ Finished installing dependencies");
    println!();

    Ok(())
}

/// Select the best installation source (binary or source) based on availability and preferences
/// Returns: (tarball_path, checksum, install_type_description)
fn select_installation_source(
    resolved_version: &unrealpm::PackageVersion,
    registry: &RegistryClient,
    package_name: &str,
    engine_version: Option<&str>,
    install_mode: InstallMode,
) -> Result<(std::path::PathBuf, String, Option<String>)> {
    // Detect current platform
    let platform = unrealpm::platform::detect_platform();

    // Check for pre-built binary if requested
    if matches!(
        install_mode,
        InstallMode::PreferBinary | InstallMode::BinaryOnly
    ) {
        if let Some(binaries) = &resolved_version.binaries {
            // Try to find matching binary
            if let Some(engine) = engine_version {
                let normalized_engine = unrealpm::platform::normalize_engine_version(engine);

                for binary in binaries {
                    if binary.platform == platform
                        && unrealpm::platform::normalize_engine_version(&binary.engine)
                            == normalized_engine
                    {
                        // Found matching binary!
                        let binary_tarball_path =
                            registry.get_tarball_path(package_name, &binary.tarball);
                        return Ok((
                            binary_tarball_path,
                            binary.checksum.clone(),
                            Some(format!("pre-built binary ({}/{})", platform, engine)),
                        ));
                    }
                }
            }
        }

        // No binary found
        if matches!(install_mode, InstallMode::BinaryOnly) {
            anyhow::bail!(
                "No pre-built binary available for {} on platform {} with engine {}.\n\n\
                Available binaries:\n{}\n\n\
                Suggestions:\n\
                  • Use --prefer-binary to fall back to source\n\
                  • Use --source-only to install from source\n\
                  • Check if binaries exist for your platform/engine combination",
                package_name,
                platform,
                engine_version.unwrap_or("unknown"),
                format_available_binaries(&resolved_version.binaries)
            );
        }
    }

    // Fall back to source (or use source if preferred)
    if matches!(
        install_mode,
        InstallMode::SourceOnly | InstallMode::PreferSource | InstallMode::PreferBinary
    ) {
        let source_tarball_path =
            registry.get_tarball_path(package_name, &resolved_version.version);
        return Ok((
            source_tarball_path,
            resolved_version.checksum.clone(),
            if resolved_version.binaries.is_some() {
                Some("source code".to_string())
            } else {
                None // Don't show "using source" if there's no binary option
            },
        ));
    }

    unreachable!("Invalid install mode state")
}

fn format_available_binaries(binaries: &Option<Vec<PrebuiltBinary>>) -> String {
    if let Some(bins) = binaries {
        if bins.is_empty() {
            return "  None".to_string();
        }
        bins.iter()
            .map(|b| format!("  - {}/{}", b.platform, b.engine))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        "  None".to_string()
    }
}
