use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::env;
use std::sync::Arc;
use unrealpm::{
    find_matching_version, install_package, resolve_dependencies, verify_checksum, Config,
    Lockfile, Manifest, ProgressCallback, RegistryClient, ResolverConfig,
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
    dry_run: bool,
    verbose_resolve: bool,
    max_depth: Option<usize>,
    resolve_timeout: Option<u64>,
) -> Result<()> {
    let current_dir = env::current_dir()?;

    // Build resolver config from CLI args and loaded config
    let loaded_config = Config::load()?;
    let resolver_config = ResolverConfig {
        max_depth: max_depth.unwrap_or(loaded_config.resolver.max_depth),
        verbose_conflicts: verbose_resolve || loaded_config.resolver.verbose_conflicts,
        resolution_timeout_seconds: resolve_timeout
            .unwrap_or(loaded_config.resolver.resolution_timeout_seconds),
    };

    match package {
        Some(pkg) => update_single_package(&pkg, &current_dir, dry_run),
        None => update_all_packages(&current_dir, dry_run, &resolver_config),
    }
}

fn update_single_package(
    package_name: &str,
    project_dir: &std::path::Path,
    dry_run: bool,
) -> Result<()> {
    if dry_run {
        println!("[DRY RUN] Would update package: {}", package_name);
    } else {
        println!("Updating package: {}", package_name);
    }
    println!();

    // Check if manifest exists
    if !Manifest::exists(project_dir) {
        println!("✗ No unrealpm.json found in current directory");
        println!();
        println!("Run 'unrealpm init' first to initialize the project.");
        return Ok(());
    }

    // Load manifest
    let manifest = Manifest::load(project_dir)?;

    // Check if package is in dependencies
    let version_constraint = manifest
        .dependencies
        .get(package_name)
        .ok_or_else(|| anyhow::anyhow!("Package '{}' not found in dependencies", package_name))?;

    println!("  Current constraint: {}", version_constraint);

    // Get engine version
    let engine_version = manifest.engine_version.as_deref();

    // Get registry client (uses HTTP if configured)
    let config = Config::load()?;
    let registry = RegistryClient::from_config(&config)?;

    // Get package metadata
    println!("  Fetching latest version...");
    let metadata = registry.get_package(package_name)?;

    // Find latest matching version
    let resolved_version =
        find_matching_version(&metadata, version_constraint, engine_version, false)?;
    println!("  ✓ Latest matching version: {}", resolved_version.version);

    // Check if already at latest version
    let current_version = if let Ok(Some(lockfile)) = Lockfile::load() {
        if let Some(locked_pkg) = lockfile.get_package(package_name) {
            if locked_pkg.version == resolved_version.version {
                println!();
                if dry_run {
                    println!(
                        "[DRY RUN] {} is already at the latest version ({})",
                        package_name, resolved_version.version
                    );
                } else {
                    println!(
                        "✓ {} is already at the latest version ({})",
                        package_name, resolved_version.version
                    );
                }
                println!();
                return Ok(());
            }
            println!(
                "  Updating from {} to {}",
                locked_pkg.version, resolved_version.version
            );
            Some(locked_pkg.version.clone())
        } else {
            None
        }
    } else {
        None
    };

    if dry_run {
        // Dry run: show what would happen
        println!(
            "  [DRY RUN] Would verify checksum: {}",
            resolved_version.checksum
        );
        if let Some(cur_ver) = current_version {
            println!(
                "  [DRY RUN] Would update from {} to {}",
                cur_ver, resolved_version.version
            );
        }
        println!(
            "  [DRY RUN] Would install to: {}/Plugins/{}",
            project_dir.display(),
            package_name
        );
        println!("  [DRY RUN] Would update lockfile (unrealpm.lock)");
        println!();
        println!(
            "[DRY RUN] Would successfully update {} to {}",
            package_name, resolved_version.version
        );
        println!();
        return Ok(());
    }

    // Get tarball path
    let tarball_path = registry.get_tarball_path(package_name, &resolved_version.version);

    // Verify checksum with progress spinner
    let progress = Some(create_spinner_callback());
    verify_checksum(&tarball_path, &resolved_version.checksum, progress)?;

    // Install package with progress spinner (this will overwrite the existing installation)
    let progress = Some(create_spinner_callback());
    let installed_path = install_package(
        &tarball_path,
        &project_dir.to_path_buf(),
        package_name,
        progress,
    )?;
    println!("  ✓ Updated at {}", installed_path.display());

    // Update lockfile
    println!("  Updating lockfile...");
    let mut lockfile = Lockfile::load()?.unwrap_or_default();
    lockfile.update_package(
        package_name.to_string(),
        resolved_version.version.clone(),
        resolved_version.checksum.clone(),
        resolved_version.dependencies.as_ref().map(|deps| {
            deps.iter()
                .map(|d| (d.name.clone(), d.version.clone()))
                .collect()
        }),
    );
    lockfile.save()?;
    println!("  ✓ Lockfile updated");

    println!();
    println!(
        "✓ Successfully updated {} to {}",
        package_name, resolved_version.version
    );
    println!();

    Ok(())
}

fn update_all_packages(
    project_dir: &std::path::Path,
    dry_run: bool,
    resolver_config: &ResolverConfig,
) -> Result<()> {
    if dry_run {
        println!("[DRY RUN] Would update all packages...");
    } else {
        println!("Updating all packages...");
    }
    println!();

    // Check if manifest exists
    if !Manifest::exists(project_dir) {
        println!("✗ No unrealpm.json found in current directory");
        println!();
        println!("Run 'unrealpm init' first to initialize the project.");
        return Ok(());
    }

    // Load manifest
    let manifest = Manifest::load(project_dir)?;

    if manifest.dependencies.is_empty() {
        println!("No dependencies to update.");
        println!();
        return Ok(());
    }

    println!("Found {} dependencies", manifest.dependencies.len());
    println!();

    // Get engine version
    let engine_version = manifest.engine_version.as_deref();

    // Get registry client (uses HTTP if configured)
    let config = Config::load()?;
    let registry = RegistryClient::from_config(&config)?;

    // Resolve all dependencies (this will get latest matching versions)
    println!("Resolving latest versions...");
    let resolved = resolve_dependencies(&manifest.dependencies, &registry, engine_version, false, Some(resolver_config))?;
    println!("  ✓ Resolved {} packages", resolved.len());
    println!();

    // Load existing lockfile to compare
    let old_lockfile = Lockfile::load()?.unwrap_or_default();
    let mut lockfile = Lockfile::new();
    let mut updated_count = 0;
    let mut pending_updates = Vec::new();

    // Install each resolved package
    for (name, resolved_pkg) in &resolved {
        // Check if version changed
        let is_update = if let Some(old_pkg) = old_lockfile.get_package(name) {
            if old_pkg.version == resolved_pkg.version {
                if dry_run {
                    println!(
                        "  {} already at latest version ({})",
                        name, resolved_pkg.version
                    );
                } else {
                    println!(
                        "  ✓ {} already at latest version ({})",
                        name, resolved_pkg.version
                    );
                }
                false
            } else {
                if dry_run {
                    println!(
                        "  [DRY RUN] Would update {}@{} -> {}",
                        name, old_pkg.version, resolved_pkg.version
                    );
                    pending_updates.push((
                        name.clone(),
                        old_pkg.version.clone(),
                        resolved_pkg.version.clone(),
                    ));
                } else {
                    println!(
                        "  Updating {}@{} -> {}...",
                        name, old_pkg.version, resolved_pkg.version
                    );
                }
                true
            }
        } else {
            if dry_run {
                println!(
                    "  [DRY RUN] Would install new dependency {}@{}",
                    name, resolved_pkg.version
                );
                pending_updates.push((
                    name.clone(),
                    "none".to_string(),
                    resolved_pkg.version.clone(),
                ));
            } else {
                println!(
                    "  Installing new dependency {}@{}...",
                    name, resolved_pkg.version
                );
            }
            true
        };

        if is_update && !dry_run {
            // Get tarball path
            let tarball_path = registry.get_tarball_path(name, &resolved_pkg.version);

            // Verify checksum (no spinner for batch updates)
            match verify_checksum(&tarball_path, &resolved_pkg.checksum, None) {
                Ok(_) => {}
                Err(e) => {
                    eprintln!("    ✗ Checksum verification failed: {}", e);
                    eprintln!("    Skipping...");
                    continue;
                }
            }

            // Install package (no spinner for batch updates)
            match install_package(&tarball_path, &project_dir.to_path_buf(), name, None) {
                Ok(installed_path) => {
                    println!("    ✓ Installed to {}", installed_path.display());
                    updated_count += 1;
                }
                Err(e) => {
                    eprintln!("    ✗ Failed to install: {}", e);
                    eprintln!("    Continuing...");
                }
            }
        } else if is_update && dry_run {
            updated_count += 1;
        }

        // Update lockfile for all packages (whether updated or not)
        lockfile.update_package(
            name.clone(),
            resolved_pkg.version.clone(),
            resolved_pkg.checksum.clone(),
            resolved_pkg.dependencies.clone(),
        );
    }

    if dry_run {
        println!();
        println!("[DRY RUN] Would update lockfile (unrealpm.lock)");
        println!();
        if updated_count > 0 {
            println!("[DRY RUN] Would update {} packages", updated_count);
        } else {
            println!("[DRY RUN] All packages already at latest versions");
        }
        println!();
        return Ok(());
    }

    // Save lockfile
    lockfile.save()?;
    println!();
    println!("  ✓ Lockfile updated");
    println!();

    if updated_count > 0 {
        println!("✓ Updated {} packages", updated_count);
    } else {
        println!("✓ All packages already at latest versions");
    }
    println!();

    Ok(())
}
