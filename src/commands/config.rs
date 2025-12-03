use anyhow::Result;
use std::path::PathBuf;
use unrealpm::Config;

pub fn run(action: &crate::ConfigAction) -> Result<()> {
    use crate::ConfigAction;

    match action {
        ConfigAction::Show => show_config(),
        ConfigAction::Set { key, value } => set_config(key, value),
        ConfigAction::AddEngine { version, path } => add_engine(version, path),
        ConfigAction::RemoveEngine { version } => remove_engine(version),
        ConfigAction::ListEngines => list_engines(),
    }
}

fn show_config() -> Result<()> {
    let config = Config::load()?;
    let config_path = Config::default_path()?;

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                         UnrealPM Configuration                               ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  📁 Config file: {}", config_path.display());
    println!();

    // Build settings
    println!("┌─ Build Settings ─────────────────────────────────────────────────────────────┐");
    println!("│                                                                              │");
    println!(
        "│  Auto-build on publish:  {}                                             │",
        format_bool(config.build.auto_build_on_publish)
    );
    println!(
        "│  Auto-build on install:  {}                                             │",
        format_bool(config.build.auto_build_on_install)
    );
    println!(
        "│  Target platforms:       {}                                    │",
        config.build.platforms.join(", ")
    );
    println!(
        "│  Build configuration:    {}                                       │",
        config.build.configuration
    );
    println!("│                                                                              │");
    println!("└──────────────────────────────────────────────────────────────────────────────┘");
    println!();

    // Registry settings
    println!("┌─ Registry Settings ──────────────────────────────────────────────────────────┐");
    println!("│                                                                              │");
    println!(
        "│  Registry URL:  {}                              │",
        config.registry.url
    );
    println!("│                                                                              │");
    println!("└──────────────────────────────────────────────────────────────────────────────┘");
    println!();

    // Engine installations
    let all_engines = config.get_all_engines();

    println!("┌─ Unreal Engine Installations ────────────────────────────────────────────────┐");
    println!("│                                                                              │");

    if all_engines.is_empty() {
        println!(
            "│  No engines found                                                            │"
        );
        println!(
            "│                                                                              │"
        );
        println!(
            "│  💡 Engines are auto-detected from standard locations                        │"
        );
        println!("│  Or add manually: unrealpm config add-engine <version> <path>               │");
    } else {
        // Separate configured vs auto-detected
        let configured: Vec<_> = all_engines
            .iter()
            .filter(|e| config.engines.iter().any(|c| c.version == e.version))
            .collect();

        let auto_detected: Vec<_> = all_engines
            .iter()
            .filter(|e| !config.engines.iter().any(|c| c.version == e.version))
            .collect();

        if !configured.is_empty() {
            println!(
                "│  📌 Configured:                                                              │"
            );
            for engine in configured {
                let path_str = truncate_path(&engine.path, 58);
                println!(
                    "│     {:6} → {}{}│",
                    engine.version,
                    path_str,
                    " ".repeat(58_usize.saturating_sub(path_str.len()))
                );
            }
            println!(
                "│                                                                              │"
            );
        }

        if !auto_detected.is_empty() {
            println!(
                "│  🔍 Auto-detected:                                                           │"
            );
            for engine in auto_detected {
                let path_str = truncate_path(&engine.path, 58);
                println!(
                    "│     {:6} → {}{}│",
                    engine.version,
                    path_str,
                    " ".repeat(58_usize.saturating_sub(path_str.len()))
                );
            }
            println!(
                "│                                                                              │"
            );
        }

        println!(
            "│  Total: {} engine{}                                                         │",
            all_engines.len(),
            if all_engines.len() == 1 { " " } else { "s" }
        );
    }

    println!("└──────────────────────────────────────────────────────────────────────────────┘");
    println!();

    println!("💡 Modify settings:");
    println!("   unrealpm config set <key> <value>");
    println!();
    println!("   Available keys:");
    println!("     • build.auto_build_on_publish");
    println!("     • build.auto_build_on_install");
    println!("     • build.configuration");
    println!("     • registry.url");
    println!();

    Ok(())
}

fn format_bool(value: bool) -> String {
    if value {
        "✅ enabled ".to_string()
    } else {
        "❌ disabled".to_string()
    }
}

fn truncate_path(path: &std::path::Path, max_len: usize) -> String {
    let path_str = path.display().to_string();
    if path_str.len() <= max_len {
        path_str
    } else {
        format!("...{}", &path_str[path_str.len() - (max_len - 3)..])
    }
}

fn set_config(key: &str, value: &str) -> Result<()> {
    let mut config = Config::load()?;

    println!();
    println!("⚙️  Updating configuration...");
    println!();

    match key {
        "build.auto_build_on_publish" => {
            config.build.auto_build_on_publish = value
                .parse::<bool>()
                .map_err(|_| anyhow::anyhow!("Invalid boolean value. Use 'true' or 'false'"))?;
            println!(
                "  ✓ build.auto_build_on_publish = {}",
                format_bool(config.build.auto_build_on_publish)
            );
        }
        "build.auto_build_on_install" => {
            config.build.auto_build_on_install = value
                .parse::<bool>()
                .map_err(|_| anyhow::anyhow!("Invalid boolean value. Use 'true' or 'false'"))?;
            println!(
                "  ✓ build.auto_build_on_install = {}",
                format_bool(config.build.auto_build_on_install)
            );
        }
        "build.configuration" => {
            config.build.configuration = value.to_string();
            println!("  ✓ build.configuration = \"{}\"", value);
        }
        "registry.url" => {
            config.registry.url = value.to_string();
            println!("  ✓ registry.url = \"{}\"", value);
        }
        "registry.registry_type" => {
            config.registry.registry_type = value.to_string();
            println!("  ✓ registry.registry_type = \"{}\"", value);
        }
        "auth.token" => {
            if value.is_empty() {
                config.auth.token = None;
                println!("  ✓ auth.token = <cleared>");
            } else {
                config.auth.token = Some(value.to_string());
                println!("  ✓ auth.token = <set>");
            }
        }
        _ => {
            println!("  ❌ Unknown key: {}", key);
            println!();
            println!("  Available keys:");
            println!("    • build.auto_build_on_publish");
            println!("    • build.auto_build_on_install");
            println!("    • build.configuration");
            println!("    • registry.url");
            println!("    • registry.registry_type");
            println!("    • auth.token");
            println!();
            anyhow::bail!("Invalid configuration key");
        }
    }

    config.save()?;
    println!();
    println!("✅ Configuration saved");
    println!();

    Ok(())
}

fn add_engine(version: &str, path: &str) -> Result<()> {
    let mut config = Config::load()?;
    let engine_path = PathBuf::from(path);

    println!();
    println!("🔧 Adding Unreal Engine {}...", version);
    println!();

    // Validate path exists
    if !engine_path.exists() {
        println!("  ❌ Path does not exist: {}", path);
        println!();
        anyhow::bail!("Invalid engine path");
    }

    // Validate it's an Unreal Engine installation
    let ubt_check = if cfg!(windows) {
        engine_path
            .join("Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.exe")
            .exists()
            || engine_path
                .join("Engine/Binaries/DotNET/UnrealBuildTool.exe")
                .exists()
    } else {
        engine_path
            .join("Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool")
            .exists()
            || engine_path
                .join("Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.dll")
                .exists()
    };

    if !ubt_check {
        println!("  ⚠️  Warning: Could not verify UnrealBuildTool at this path");
        println!("     Make sure this is a valid Unreal Engine installation");
        println!();
    } else {
        println!("  ✓ Validated Unreal Engine installation");
        println!();
    }

    config.add_engine(version.to_string(), engine_path.clone());
    config.save()?;

    println!("✅ Added Unreal Engine {}", version);
    println!("   Path: {}", engine_path.display());
    println!();

    Ok(())
}

fn remove_engine(version: &str) -> Result<()> {
    let mut config = Config::load()?;

    println!();
    println!("🗑️  Removing Unreal Engine {}...", version);
    println!();

    if !config.engines.iter().any(|e| e.version == version) {
        println!(
            "  ❌ Engine version '{}' not found in configured engines",
            version
        );
        println!();
        println!("  💡 View configured engines: unrealpm config list-engines");
        println!();
        anyhow::bail!("Engine not found");
    }

    config.remove_engine(version);
    config.save()?;

    println!("✅ Removed Unreal Engine {}", version);
    println!();

    Ok(())
}

fn list_engines() -> Result<()> {
    let config = Config::load()?;

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                   Unreal Engine Installations                                ║");
    println!("╚══════════════════════════════════════════════════════════════════════════════╝");
    println!();

    let all_engines = config.get_all_engines();

    if all_engines.is_empty() {
        println!("  ❌ No Unreal Engine installations found");
        println!();
        println!("  💡 Auto-detection scans standard locations:");
        println!("     • Windows: C:\\Program Files\\Epic Games\\UE_*");
        println!("     • Linux:   ~/UnrealEngine/UE_* and /opt/UnrealEngine/*");
        println!("     • macOS:   /Users/Shared/Epic Games/UE_*");
        println!();
        println!("  Or add manually:");
        println!("     unrealpm config add-engine <version> <path>");
        println!();
        println!("  Example:");
        println!("     unrealpm config add-engine 5.3 /path/to/UE_5.3");
    } else {
        // Separate configured vs auto-detected
        let configured: Vec<_> = all_engines
            .iter()
            .filter(|e| config.engines.iter().any(|c| c.version == e.version))
            .collect();

        let auto_detected: Vec<_> = all_engines
            .iter()
            .filter(|e| !config.engines.iter().any(|c| c.version == e.version))
            .collect();

        if !configured.is_empty() {
            println!("  📌 Configured Engines:");
            println!(
                "  ┌──────────────────────────────────────────────────────────────────────────┐"
            );
            for engine in configured {
                let path_str = truncate_path(&engine.path, 60);
                println!(
                    "  │  {:6} → {}{}│",
                    engine.version,
                    path_str,
                    " ".repeat(60_usize.saturating_sub(path_str.len()))
                );
            }
            println!(
                "  └──────────────────────────────────────────────────────────────────────────┘"
            );
            println!();
        }

        if !auto_detected.is_empty() {
            println!("  🔍 Auto-Detected Engines:");
            println!(
                "  ┌──────────────────────────────────────────────────────────────────────────┐"
            );
            for engine in auto_detected {
                let path_str = truncate_path(&engine.path, 60);
                println!(
                    "  │  {:6} → {}{}│",
                    engine.version,
                    path_str,
                    " ".repeat(60_usize.saturating_sub(path_str.len()))
                );
            }
            println!(
                "  └──────────────────────────────────────────────────────────────────────────┘"
            );
            println!();
        }

        println!(
            "  📊 Total: {} engine{}",
            all_engines.len(),
            if all_engines.len() == 1 { "" } else { "s" }
        );
    }
    println!();

    Ok(())
}
