use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use unrealpm::{Config, UPlugin};

pub fn run(
    path: Option<String>,
    engine: Option<String>,
    platform: Option<String>,
    all_platforms: bool,
) -> Result<()> {
    println!("Building plugin binaries...");
    println!();

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
    println!();

    // Load config
    let config = Config::load()?;

    // Determine engine version to build for
    let engine_version = if let Some(v) = engine {
        v
    } else if let Some(v) = &uplugin.engine_version {
        v.clone()
    } else {
        anyhow::bail!(
            "No engine version specified.\n\n\
            Specify with:\n\
              • --engine <version> flag\n\
              • EngineVersion in .uplugin file\n\
              • unrealpm config add-engine <version> <path>"
        );
    };

    // Find engine installation
    let engine_install = config.find_engine(&engine_version).ok_or_else(|| {
        anyhow::anyhow!(
            "Unreal Engine {} not found in configuration.\n\n\
                Configure it with:\n\
                  unrealpm config add-engine {} /path/to/UE_{}\n\n\
                Example:\n\
                  unrealpm config add-engine 5.3 C:\\Program Files\\Epic Games\\UE_5.3",
            engine_version,
            engine_version,
            engine_version
        )
    })?;

    println!(
        "  Using Unreal Engine: {} at {}",
        engine_version,
        engine_install.path.display()
    );
    println!();

    // Determine platforms to build
    let platforms = if all_platforms {
        config.build.platforms.clone()
    } else if let Some(p) = platform {
        vec![p]
    } else {
        vec![unrealpm::detect_platform()]
    };

    println!("  Building for platforms: {}", platforms.join(", "));
    println!();

    // Build for each platform
    for target_platform in &platforms {
        println!("Building for {}...", target_platform);
        build_for_platform(
            &plugin_dir,
            &plugin_name,
            &engine_version,
            target_platform,
            &config,
        )?;
        println!("  ✓ Built for {}", target_platform);
        println!();
    }

    println!(
        "✓ Successfully built {} for {} platform{}",
        plugin_name,
        platforms.len(),
        if platforms.len() == 1 { "" } else { "s" }
    );
    println!();

    // Show where binaries are
    let binaries_dir = plugin_dir.join("Binaries");
    if binaries_dir.exists() {
        println!("Binaries location: {}", binaries_dir.display());
        println!();
        println!("Next steps:");
        println!("  • Test the plugin in Unreal Editor");
        println!("  • Publish with binaries: unrealpm publish --include-binaries");
    }

    Ok(())
}

/// Build plugin for a specific platform (public function for use by publish)
pub fn build_for_platform(
    plugin_dir: &Path,
    plugin_name: &str,
    engine_version: &str,
    platform: &str,
    config: &Config,
) -> Result<()> {
    // Find engine installation
    let engine_install = config
        .find_engine(engine_version)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unreal Engine {} not configured. Add it with: unrealpm config add-engine {} /path/to/UE",
                engine_version,
                engine_version
            )
        })?;

    build_plugin(
        plugin_dir,
        plugin_name,
        &engine_install.path,
        platform,
        &config.build.configuration,
    )
}

fn build_plugin(
    plugin_dir: &Path,
    plugin_name: &str,
    engine_path: &Path,
    platform: &str,
    configuration: &str,
) -> Result<()> {
    println!("  Preparing build...");
    println!("  Platform: {}, Configuration: {}", platform, configuration);

    // Check if we're on WSL and need to convert paths
    let is_wsl = env::var("WSL_DISTRO_NAME").is_ok();

    // Find the .uplugin file (search for it since package name may differ from plugin name)
    let plugin_path = unrealpm::UPlugin::find(plugin_dir)?;

    let plugin_path_arg = if is_wsl && platform == "Win64" {
        unrealpm::platform::wsl_to_windows_path(&plugin_path)
            .unwrap_or_else(|| plugin_path.display().to_string())
    } else {
        plugin_path.display().to_string()
    };

    // Build the plugin using the RunUAT BuildPlugin command
    // This is the proper way to build standalone plugins
    let run_uat = if cfg!(windows) || (is_wsl && platform == "Win64") {
        engine_path.join("Engine/Build/BatchFiles/RunUAT.bat")
    } else {
        engine_path.join("Engine/Build/BatchFiles/RunUAT.sh")
    };

    if !run_uat.exists() {
        anyhow::bail!("RunUAT not found at: {}", run_uat.display());
    }

    // On WSL, we need to call .bat files through cmd.exe
    let mut cmd = if is_wsl && platform == "Win64" {
        let run_uat_windows = unrealpm::platform::wsl_to_windows_path(&run_uat)
            .unwrap_or_else(|| run_uat.display().to_string());

        let mut c = Command::new("cmd.exe");
        c.arg("/C");
        c.arg(&run_uat_windows);
        c
    } else {
        Command::new(&run_uat)
    };

    cmd.arg("BuildPlugin");
    cmd.arg(format!("-Plugin={}", plugin_path_arg));
    cmd.arg(format!(
        "-Package={}",
        plugin_path_arg.replace(".uplugin", "")
    ));
    cmd.arg(format!("-TargetPlatforms={}", platform));
    cmd.arg(format!("-TargetConfigurations={}", configuration));

    println!("  Running RunUAT BuildPlugin...");
    println!();

    // Create progress bar with elapsed time
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg} [{elapsed_precise}]")
            .unwrap()
            .progress_chars("█▓▒░ ")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.set_message("Initializing build...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let start_time = Instant::now();

    // Spawn the process and capture output line by line
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;

    // Capture both stdout and stderr
    let mut all_output = Vec::new();

    // Read stdout in real-time to parse progress
    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        if let Ok(line) = line {
            all_output.push(line.clone());

            // Parse progress like [32/63]
            if let Some(progress) = parse_build_progress(&line) {
                let (current, total) = progress;
                pb.set_length(total);
                pb.set_position(current);
                pb.set_message("Compiling...");
            }
        }
    }

    // Wait for completion
    let status = child.wait()?;

    // Capture stderr after wait
    let mut stderr_output = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        use std::io::Read;
        stderr.read_to_string(&mut stderr_output)?;
    }
    if !stderr_output.is_empty() {
        all_output.extend(stderr_output.lines().map(|s| s.to_string()));
    }

    if !status.success() {
        pb.finish_and_clear();
        println!();

        // Show the last part of the output (most relevant errors)
        if !all_output.is_empty() {
            println!("  Build output (last 30 lines):");
            println!("  ─────────────────────────────────────────────────────────────");
            let start_idx = all_output.len().saturating_sub(30);
            for line in &all_output[start_idx..] {
                println!("  {}", line);
            }
            println!("  ─────────────────────────────────────────────────────────────");
            println!();
        }

        anyhow::bail!("Build failed for {} on {}", plugin_name, platform);
    }

    let elapsed = start_time.elapsed();
    pb.finish_with_message(format!("Build completed in {:.1}s", elapsed.as_secs_f32()));
    println!();

    Ok(())
}

/// Parse build progress from UBT output (e.g., "[32/63]" -> Some((32, 63)))
fn parse_build_progress(line: &str) -> Option<(u64, u64)> {
    // Look for patterns like [32/63]
    use regex::Regex;
    let re = Regex::new(r"\[(\d+)/(\d+)\]").ok()?;

    if let Some(caps) = re.captures(line) {
        let current = caps.get(1)?.as_str().parse().ok()?;
        let total = caps.get(2)?.as_str().parse().ok()?;
        return Some((current, total));
    }

    None
}

#[allow(dead_code)]
fn find_unreal_build_tool(engine_path: &Path, target_platform: &str) -> Result<PathBuf> {
    // Check if running on WSL
    let is_wsl = env::var("WSL_DISTRO_NAME").is_ok();

    // Determine which UBT to use
    // For Windows builds from WSL, use .exe
    // For Linux builds, use native UBT
    let possible_paths = if target_platform == "Win64" || cfg!(windows) {
        // Building for Windows - use .exe (works on Windows and WSL via Windows interop)
        vec![
            engine_path.join("Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.exe"),
            engine_path.join("Engine/Binaries/DotNET/UnrealBuildTool.exe"),
        ]
    } else {
        // Building for Linux/Mac - use native UBT
        if is_wsl {
            // Can't build Linux binaries from WSL using Windows UE installation
            anyhow::bail!(
                "Cannot build {} binaries from WSL using Windows Unreal Engine.\n\n\
                Options:\n\
                  • Build Win64 binaries instead: unrealpm build --platform Win64\n\
                  • Install Linux version of Unreal Engine\n\
                  • Use a native Linux environment",
                target_platform
            );
        }
        vec![
            engine_path.join("Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool"),
            engine_path.join("Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.dll"),
        ]
    };

    for path in possible_paths {
        if path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "Could not find UnrealBuildTool in engine installation: {}\n\n\
        Expected locations:\n\
          • Engine/Binaries/DotNET/UnrealBuildTool/UnrealBuildTool.exe\n\
          • Engine/Binaries/DotNET/UnrealBuildTool.exe",
        engine_path.display()
    )
}

#[allow(dead_code)]
fn create_temp_project(_plugin_dir: &Path, plugin_name: &str, _platform: &str) -> Result<PathBuf> {
    // Create a minimal .uproject file in a temp location
    // On WSL, use Windows temp directory so UBT can access it
    let is_wsl = env::var("WSL_DISTRO_NAME").is_ok();

    let temp_dir = if is_wsl {
        // Use Windows TEMP directory accessible from WSL
        PathBuf::from("/mnt/c/Users")
            .join(env::var("USER").unwrap_or_else(|_| "Public".to_string()))
            .join("AppData/Local/Temp")
            .join(format!("unrealpm-build-{}", plugin_name))
    } else {
        env::temp_dir().join(format!("unrealpm-build-{}", plugin_name))
    };

    fs::create_dir_all(&temp_dir)?;

    let uproject_path = temp_dir.join(format!("{}.uproject", plugin_name));

    let uproject_content = serde_json::json!({
        "FileVersion": 3,
        "EngineAssociation": "5.3",
        "Category": "",
        "Description": "Temporary project for building plugin",
        "Plugins": [
            {
                "Name": plugin_name,
                "Enabled": true
            }
        ]
    });

    fs::write(
        &uproject_path,
        serde_json::to_string_pretty(&uproject_content)?,
    )?;

    Ok(uproject_path)
}
