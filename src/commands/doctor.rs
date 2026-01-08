//! Doctor command - diagnose setup issues
//!
//! Checks:
//! - Unreal Engine installations
//! - Registry connectivity
//! - Configuration validity
//! - Cache health
//! - Authentication status

use anyhow::Result;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use unrealpm::{get_store_dir, get_store_stats, Config, Lockfile, Manifest, RegistryClient};

/// Status of a check
#[derive(Debug)]
enum CheckStatus {
    Ok,
    Warning,
    Error,
}

impl CheckStatus {
    fn symbol(&self) -> &'static str {
        match self {
            CheckStatus::Ok => "✓",
            CheckStatus::Warning => "⚠",
            CheckStatus::Error => "✗",
        }
    }

    fn color_code(&self) -> &'static str {
        match self {
            CheckStatus::Ok => "\x1b[32m",      // Green
            CheckStatus::Warning => "\x1b[33m", // Yellow
            CheckStatus::Error => "\x1b[31m",   // Red
        }
    }
}

struct CheckResult {
    name: String,
    status: CheckStatus,
    message: String,
    details: Option<String>,
}

impl CheckResult {
    fn new(name: &str, status: CheckStatus, message: &str) -> Self {
        Self {
            name: name.to_string(),
            status,
            message: message.to_string(),
            details: None,
        }
    }

    fn with_details(mut self, details: &str) -> Self {
        self.details = Some(details.to_string());
        self
    }

    fn print(&self, verbose: bool) {
        let reset = "\x1b[0m";
        println!(
            "  {}{}{} {} - {}",
            self.status.color_code(),
            self.status.symbol(),
            reset,
            self.name,
            self.message
        );
        if verbose {
            if let Some(ref details) = self.details {
                for line in details.lines() {
                    println!("      {}", line);
                }
            }
        }
    }
}

pub fn run(verbose: bool, fix: bool) -> Result<()> {
    println!("UnrealPM Doctor");
    println!("===============");
    println!();
    println!("Checking your setup...");
    println!();

    let mut results = Vec::new();
    let mut fixable_issues = Vec::new();

    // Check 1: Configuration
    results.push(check_config());

    // Check 2: Registry connectivity
    results.push(check_registry());

    // Check 3: Unreal Engine installations
    results.push(check_engines());

    // Check 4: Cache health
    let (cache_result, cache_fix) = check_cache();
    results.push(cache_result);
    if let Some(fix_fn) = cache_fix {
        fixable_issues.push(("Clean stale cache entries", fix_fn));
    }

    // Check 5: Project (if in a project directory)
    if let Some(result) = check_project() {
        results.push(result);
    }

    // Check 6: Authentication
    results.push(check_auth());

    // Print results
    println!("Results:");
    println!();
    for result in &results {
        result.print(verbose);
    }

    // Summary
    let ok_count = results
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Ok))
        .count();
    let warn_count = results
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Warning))
        .count();
    let error_count = results
        .iter()
        .filter(|r| matches!(r.status, CheckStatus::Error))
        .count();

    println!();
    println!(
        "Summary: {} passed, {} warnings, {} errors",
        ok_count, warn_count, error_count
    );

    // Offer fixes
    if !fixable_issues.is_empty() {
        println!();
        if fix {
            println!("Applying fixes...");
            println!();
            for (name, fix_fn) in fixable_issues {
                print!("  Fixing: {}... ", name);
                match fix_fn() {
                    Ok(msg) => println!("{}", msg),
                    Err(e) => println!("Failed: {}", e),
                }
            }
        } else {
            println!("Some issues can be fixed automatically. Run with --fix to apply:");
            for (name, _) in &fixable_issues {
                println!("  - {}", name);
            }
        }
    }

    println!();

    if error_count > 0 {
        println!("Some checks failed. See above for details.");
        if !verbose {
            println!("Run with --verbose for more information.");
        }
    } else if warn_count > 0 {
        println!("All critical checks passed, but there are some warnings.");
    } else {
        println!("All checks passed! Your setup looks good.");
    }

    Ok(())
}

fn check_config() -> CheckResult {
    match Config::load() {
        Ok(config) => {
            let mut details = String::new();
            details.push_str(&format!(
                "Registry type: {:?}\n",
                config.registry.registry_type
            ));
            details.push_str(&format!("Registry URL: {}\n", config.registry.url));
            details.push_str(&format!("Signing enabled: {}\n", config.signing.enabled));
            details.push_str(&format!(
                "Auto-build on install: {}\n",
                config.build.auto_build_on_install
            ));

            CheckResult::new(
                "Configuration",
                CheckStatus::Ok,
                "Valid configuration loaded",
            )
            .with_details(&details)
        }
        Err(e) => CheckResult::new(
            "Configuration",
            CheckStatus::Error,
            &format!("Failed to load: {}", e),
        ),
    }
}

fn check_registry() -> CheckResult {
    let config = match Config::load() {
        Ok(c) => c,
        Err(_) => {
            return CheckResult::new(
                "Registry",
                CheckStatus::Error,
                "Cannot check registry - config failed to load",
            )
        }
    };

    let start = Instant::now();
    match RegistryClient::from_config(&config) {
        Ok(registry) => {
            // Try to list packages to verify connectivity
            match registry.search("") {
                Ok(packages) => {
                    let elapsed = start.elapsed();
                    let details = format!(
                        "URL: {}\nPackages available: {}\nResponse time: {:?}",
                        config.registry.url,
                        packages.len(),
                        elapsed
                    );

                    if elapsed > Duration::from_secs(5) {
                        CheckResult::new(
                            "Registry",
                            CheckStatus::Warning,
                            &format!("Connected but slow ({:.1}s)", elapsed.as_secs_f64()),
                        )
                        .with_details(&details)
                    } else {
                        CheckResult::new(
                            "Registry",
                            CheckStatus::Ok,
                            &format!("Connected ({} packages)", packages.len()),
                        )
                        .with_details(&details)
                    }
                }
                Err(e) => CheckResult::new(
                    "Registry",
                    CheckStatus::Error,
                    &format!("Failed to connect: {}", e),
                )
                .with_details(&format!("URL: {}", config.registry.url)),
            }
        }
        Err(e) => CheckResult::new(
            "Registry",
            CheckStatus::Error,
            &format!("Failed to create client: {}", e),
        ),
    }
}

fn check_engines() -> CheckResult {
    let config = match Config::load() {
        Ok(c) => c,
        Err(_) => {
            return CheckResult::new(
                "Unreal Engines",
                CheckStatus::Warning,
                "Cannot check engines - config failed to load",
            )
        }
    };

    if config.engines.is_empty() {
        // Try to detect engines
        let detected = unrealpm::detect_unreal_engines();
        if !detected.is_empty() {
            let details = detected
                .iter()
                .map(|(v, p)| format!("{}: {}", v, p.display()))
                .collect::<Vec<_>>()
                .join("\n");

            CheckResult::new(
                "Unreal Engines",
                CheckStatus::Warning,
                &format!(
                    "No configured engines, but {} detected. Run `unrealpm config add-engine` to add them.",
                    detected.len()
                ),
            )
            .with_details(&details)
        } else {
            CheckResult::new(
                "Unreal Engines",
                CheckStatus::Warning,
                "No engines configured or detected. Building will require manual engine paths.",
            )
        }
    } else {
        let mut valid = 0;
        let mut invalid = Vec::new();

        for engine in &config.engines {
            let path =
                PathBuf::from(shellexpand::tilde(&engine.path.to_string_lossy()).to_string());
            if path.exists() {
                valid += 1;
            } else {
                invalid.push(format!(
                    "{}: {} (not found)",
                    engine.version,
                    path.display()
                ));
            }
        }

        if invalid.is_empty() {
            let details = config
                .engines
                .iter()
                .map(|e| format!("{}: {}", e.version, e.path.display()))
                .collect::<Vec<_>>()
                .join("\n");

            CheckResult::new(
                "Unreal Engines",
                CheckStatus::Ok,
                &format!("{} engine(s) configured", valid),
            )
            .with_details(&details)
        } else {
            CheckResult::new(
                "Unreal Engines",
                CheckStatus::Warning,
                &format!("{} valid, {} invalid paths", valid, invalid.len()),
            )
            .with_details(&invalid.join("\n"))
        }
    }
}

#[allow(clippy::type_complexity)]
fn check_cache() -> (CheckResult, Option<Box<dyn FnOnce() -> Result<String>>>) {
    match get_store_dir() {
        Ok(store_dir) => {
            match get_store_stats() {
                Ok(stats) => {
                    // Check for stale temp directories
                    let mut stale_count = 0;
                    if let Ok(entries) = fs::read_dir(&store_dir) {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if name.ends_with("-extracting") {
                                stale_count += 1;
                            }
                        }
                    }

                    let details = format!(
                        "Location: {}\nPackages: {}\nTotal size: {:.2} MB",
                        store_dir.display(),
                        stats.package_count,
                        stats.total_size as f64 / 1024.0 / 1024.0
                    );

                    if stale_count > 0 {
                        let fix: Box<dyn FnOnce() -> Result<String>> = Box::new(move || {
                            let mut removed = 0;
                            if let Ok(entries) = fs::read_dir(&store_dir) {
                                for entry in entries.flatten() {
                                    let name = entry.file_name().to_string_lossy().to_string();
                                    if name.ends_with("-extracting")
                                        && fs::remove_dir_all(entry.path()).is_ok()
                                    {
                                        removed += 1;
                                    }
                                }
                            }
                            Ok(format!("Removed {} stale entries", removed))
                        });

                        (
                            CheckResult::new(
                                "Cache",
                                CheckStatus::Warning,
                                &format!(
                                    "{} packages cached, {} stale entries",
                                    stats.package_count, stale_count
                                ),
                            )
                            .with_details(&details),
                            Some(fix),
                        )
                    } else {
                        (
                            CheckResult::new(
                                "Cache",
                                CheckStatus::Ok,
                                &format!("{} packages cached", stats.package_count),
                            )
                            .with_details(&details),
                            None,
                        )
                    }
                }
                Err(e) => (
                    CheckResult::new(
                        "Cache",
                        CheckStatus::Error,
                        &format!("Failed to read: {}", e),
                    ),
                    None,
                ),
            }
        }
        Err(e) => (
            CheckResult::new(
                "Cache",
                CheckStatus::Error,
                &format!("Not accessible: {}", e),
            ),
            None,
        ),
    }
}

fn check_project() -> Option<CheckResult> {
    let current_dir = env::current_dir().ok()?;

    // Check for unrealpm.json
    let manifest_path = current_dir.join("unrealpm.json");
    if !manifest_path.exists() {
        return None; // Not in a project, skip this check
    }

    match Manifest::load(&current_dir) {
        Ok(manifest) => {
            let dep_count = manifest.dependencies.len();
            let lockfile_exists = current_dir.join("unrealpm.lock").exists();

            let mut details = format!("Dependencies: {}\n", dep_count);
            if let Some(engine) = &manifest.engine_version {
                details.push_str(&format!("Engine version: {}\n", engine));
            }
            details.push_str(&format!(
                "Lockfile: {}",
                if lockfile_exists {
                    "present"
                } else {
                    "missing"
                }
            ));

            // Check lockfile sync
            if lockfile_exists {
                if let Ok(Some(lockfile)) = Lockfile::load() {
                    let locked_count = lockfile.packages.len();
                    if locked_count < dep_count {
                        return Some(
                            CheckResult::new(
                                "Project",
                                CheckStatus::Warning,
                                &format!(
                                    "{} deps, lockfile has {} - run `unrealpm install`",
                                    dep_count, locked_count
                                ),
                            )
                            .with_details(&details),
                        );
                    }
                }
            } else if dep_count > 0 {
                return Some(
                    CheckResult::new(
                        "Project",
                        CheckStatus::Warning,
                        &format!(
                            "{} deps but no lockfile - run `unrealpm install`",
                            dep_count
                        ),
                    )
                    .with_details(&details),
                );
            }

            Some(
                CheckResult::new(
                    "Project",
                    CheckStatus::Ok,
                    &format!("{} dependencies", dep_count),
                )
                .with_details(&details),
            )
        }
        Err(e) => Some(CheckResult::new(
            "Project",
            CheckStatus::Error,
            &format!("Invalid manifest: {}", e),
        )),
    }
}

fn check_auth() -> CheckResult {
    let config = match Config::load() {
        Ok(c) => c,
        Err(_) => {
            return CheckResult::new(
                "Authentication",
                CheckStatus::Warning,
                "Cannot check auth - config failed to load",
            )
        }
    };

    if let Some(token) = &config.auth.token {
        if token.starts_with("urpm_") {
            CheckResult::new("Authentication", CheckStatus::Ok, "API token configured")
                .with_details(&format!("Token: {}...", &token[..15.min(token.len())]))
        } else {
            CheckResult::new("Authentication", CheckStatus::Ok, "JWT token configured")
        }
    } else {
        CheckResult::new(
            "Authentication",
            CheckStatus::Warning,
            "Not logged in - run `unrealpm login` to publish packages",
        )
    }
}
