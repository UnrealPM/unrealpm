use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use unrealpm::{Lockfile, Manifest};

pub fn run(package: String) -> Result<()> {
    let current_dir = env::current_dir()?;

    println!("Searching for why {} is installed...", package);
    println!();

    // Check if manifest exists
    if !Manifest::exists(&current_dir) {
        println!("✗ No unrealpm.json found in current directory");
        println!();
        println!("Run 'unrealpm init' first to initialize the project.");
        return Ok(());
    }

    // Load manifest and lockfile
    let manifest = Manifest::load(&current_dir)?;
    let lockfile = Lockfile::load()?;

    if manifest.dependencies.is_empty() {
        println!("No dependencies installed.");
        println!();
        return Ok(());
    }

    let lockfile = match lockfile {
        Some(lf) => lf,
        None => {
            println!("✗ No lockfile found (unrealpm.lock)");
            println!();
            println!("Run 'unrealpm install' first to install dependencies.");
            return Ok(());
        }
    };

    // Check if package is installed
    if lockfile.get_package(&package).is_none() {
        println!("✗ Package '{}' is not installed", package);
        println!();
        return Ok(());
    }

    // Build reverse dependency map (who depends on whom)
    let mut reverse_deps: HashMap<String, Vec<String>> = HashMap::new();

    for (pkg_name, pkg) in lockfile.packages.iter() {
        if let Some(deps) = &pkg.dependencies {
            for dep_name in deps.keys() {
                reverse_deps
                    .entry(dep_name.clone())
                    .or_insert_with(Vec::new)
                    .push(pkg_name.clone());
            }
        }
    }

    // Find all paths from direct dependencies to the target package
    let mut paths = Vec::new();

    // Check if it's a direct dependency
    if manifest.dependencies.contains_key(&package) {
        paths.push(vec![package.clone()]);
    }

    // BFS to find all paths from direct dependencies
    for direct_dep in manifest.dependencies.keys() {
        if direct_dep == &package {
            continue; // Already handled above
        }

        let found_paths = find_paths(direct_dep, &package, &reverse_deps, &lockfile);
        paths.extend(found_paths);
    }

    // Display results
    if paths.is_empty() {
        println!("✗ Could not determine why '{}' is installed", package);
        println!("  It may be orphaned or the lockfile may be corrupted.");
        println!();
    } else {
        if paths.len() == 1 && paths[0].len() == 1 {
            // Direct dependency
            println!("{} is a direct dependency in unrealpm.json", package);
            if let Some(constraint) = manifest.dependencies.get(&package) {
                if let Some(pkg) = lockfile.get_package(&package) {
                    println!("  Constraint: {}", constraint);
                    println!("  Installed: {}", pkg.version);
                }
            }
        } else {
            // Transitive dependency
            println!(
                "{} is installed because of the following dependency chains:",
                package
            );
            println!();

            for (idx, path) in paths.iter().enumerate() {
                if path.len() == 1 {
                    // Direct dependency (shown separately above)
                    continue;
                }

                println!("  Chain #{}:", idx + 1);
                for (i, pkg_name) in path.iter().enumerate() {
                    let indent = "    ".repeat(i);
                    let arrow = if i > 0 { "└─> " } else { "" };

                    if let Some(pkg) = lockfile.get_package(pkg_name) {
                        if i == 0 {
                            // Root of chain (direct dependency)
                            if let Some(constraint) = manifest.dependencies.get(pkg_name) {
                                println!(
                                    "{}{}{}@{} ({})",
                                    indent, arrow, pkg_name, pkg.version, constraint
                                );
                            }
                        } else if i == path.len() - 1 {
                            // Target package
                            println!("{}{}{}@{} (target)", indent, arrow, pkg_name, pkg.version);
                        } else {
                            // Intermediate dependency
                            println!("{}{}{}@{}", indent, arrow, pkg_name, pkg.version);
                        }
                    }
                }
                println!();
            }
        }
        println!();
    }

    Ok(())
}

fn find_paths(
    start: &str,
    target: &str,
    _reverse_deps: &HashMap<String, Vec<String>>,
    lockfile: &Lockfile,
) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    let mut queue = VecDeque::new();
    queue.push_back((start.to_string(), vec![start.to_string()]));

    let mut visited = HashSet::new();

    while let Some((current, path)) = queue.pop_front() {
        if visited.contains(&current) {
            continue;
        }
        visited.insert(current.clone());

        // Get dependencies of current package
        if let Some(pkg) = lockfile.get_package(&current) {
            if let Some(deps) = &pkg.dependencies {
                for dep_name in deps.keys() {
                    if dep_name == target {
                        // Found target
                        let mut new_path = path.clone();
                        new_path.push(dep_name.clone());
                        paths.push(new_path);
                    } else {
                        // Continue searching
                        let mut new_path = path.clone();
                        new_path.push(dep_name.clone());
                        queue.push_back((dep_name.clone(), new_path));
                    }
                }
            }
        }
    }

    paths
}
