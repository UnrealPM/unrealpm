use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::env;
use unrealpm::{Lockfile, Manifest};

pub fn run() -> Result<()> {
    let current_dir = env::current_dir()?;

    println!("Dependency tree:");
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
        println!("No dependencies to display.");
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

    // Build dependency map from lockfile
    let mut dep_map: HashMap<String, Vec<(String, String)>> = HashMap::new();

    for (pkg_name, pkg) in lockfile.packages.iter() {
        if let Some(deps) = &pkg.dependencies {
            // Convert HashMap to Vec of tuples
            let deps_vec: Vec<(String, String)> = deps.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            dep_map.insert(pkg_name.clone(), deps_vec);
        } else {
            dep_map.insert(pkg_name.clone(), Vec::new());
        }
    }

    // Print tree for each direct dependency
    let mut visited = HashSet::new();

    for (name, constraint) in &manifest.dependencies {
        if let Some(pkg) = lockfile.get_package(name) {
            print_tree_node(
                name,
                &pkg.version,
                constraint,
                &dep_map,
                0,
                true,
                &mut visited,
                &HashSet::new(),
            );
        } else {
            println!("├── {} (not installed)", name);
        }
    }

    println!();
    Ok(())
}

fn print_tree_node(
    name: &str,
    version: &str,
    constraint: &str,
    dep_map: &HashMap<String, Vec<(String, String)>>,
    depth: usize,
    is_last: bool,
    visited: &mut HashSet<String>,
    ancestors: &HashSet<String>,
) {
    // Indentation
    let prefix = if depth == 0 {
        String::new()
    } else {
        let mut p = String::new();
        for _ in 0..(depth - 1) {
            p.push_str("│   ");
        }
        if is_last {
            p.push_str("└── ");
        } else {
            p.push_str("├── ");
        }
        p
    };

    // Check for circular dependencies
    let is_circular = ancestors.contains(name);

    // Package display
    let pkg_display = if depth == 0 {
        format!("{}@{} ({})", name, version, constraint)
    } else {
        format!("{}@{}", name, version)
    };

    if is_circular {
        println!("{}{} (circular)", prefix, pkg_display);
        return;
    }

    // Check if we've already visited this package
    let was_visited = visited.contains(name);
    visited.insert(name.to_string());

    if was_visited && depth > 0 {
        println!("{}{} (already shown)", prefix, pkg_display);
        return;
    }

    println!("{}{}", prefix, pkg_display);

    // Print dependencies
    if let Some(deps) = dep_map.get(name) {
        if !deps.is_empty() {
            let mut new_ancestors = ancestors.clone();
            new_ancestors.insert(name.to_string());

            for (i, (dep_name, dep_version)) in deps.iter().enumerate() {
                let is_last_dep = i == deps.len() - 1;
                print_tree_node(
                    dep_name,
                    dep_version,
                    "*", // We don't have constraint info for transitive deps
                    dep_map,
                    depth + 1,
                    is_last_dep,
                    visited,
                    &new_ancestors,
                );
            }
        }
    }
}
