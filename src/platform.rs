//! Platform detection and Unreal Engine path resolution
//!
//! This module provides utilities for detecting the current platform,
//! finding Unreal Engine installations, and handling WSL-specific functionality.
//!
//! # Examples
//!
//! ```
//! use unrealpm::{detect_platform, normalize_engine_version};
//!
//! let platform = detect_platform();
//! println!("Platform: {}", platform); // "Win64", "Linux", or "Mac"
//!
//! let version = normalize_engine_version("5.3.0");
//! assert_eq!(version, "5.3");
//! ```

use std::env;
use std::fs;
use std::path::PathBuf;

/// Detect the current platform
///
/// Returns platform string compatible with Unreal Engine:
/// - "Win64" for Windows (and WSL accessing Windows UE)
/// - "Linux" for native Linux
/// - "Mac" for macOS (both Intel and Apple Silicon)
///
/// # WSL Handling
///
/// When running on WSL, this function returns "Win64" because UnrealPM
/// typically uses the Windows Unreal Engine installation from WSL.
pub fn detect_platform() -> String {
    // Check if running on WSL - default to Win64 since we're using Windows UE
    let is_wsl = env::var("WSL_DISTRO_NAME").is_ok() ||
                 fs::read_to_string("/proc/version")
                     .map(|v| v.contains("microsoft") || v.contains("WSL"))
                     .unwrap_or(false);

    if is_wsl {
        return "Win64".to_string();
    }

    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    match (os, arch) {
        ("windows", "x86_64") => "Win64".to_string(),
        ("linux", "x86_64") => "Linux".to_string(),
        ("macos", "x86_64") => "Mac".to_string(),
        ("macos", "aarch64") => "Mac".to_string(), // Apple Silicon
        _ => format!("{}-{}", os, arch), // Fallback
    }
}

/// Normalize engine version for comparison
/// Converts "5.2.0" -> "5.2", "5.3" -> "5.3"
pub fn normalize_engine_version(version: &str) -> String {
    // Take only major.minor
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[0], parts[1])
    } else {
        version.to_string()
    }
}

/// Auto-detect Unreal Engine installations on the system
pub fn detect_unreal_engines() -> Vec<(String, PathBuf)> {
    let mut engines = Vec::new();

    // Check if running on WSL
    let is_wsl = env::var("WSL_DISTRO_NAME").is_ok() ||
                 fs::read_to_string("/proc/version")
                     .map(|v| v.contains("microsoft") || v.contains("WSL"))
                     .unwrap_or(false);

    if cfg!(windows) || is_wsl {
        // Determine the base path for Windows drives
        let windows_base = if is_wsl {
            // On WSL, Windows drives are mounted at /mnt/c, /mnt/d, etc.
            vec![
                PathBuf::from("/mnt/c/Program Files/Epic Games"),
                PathBuf::from("/mnt/d/Program Files/Epic Games"),
            ]
        } else if let Ok(program_files) = env::var("ProgramFiles") {
            // Native Windows
            vec![PathBuf::from(program_files).join("Epic Games")]
        } else {
            vec![]
        };

        // Check Epic Games Launcher installations
        for epic_path in windows_base {
            if let Ok(entries) = fs::read_dir(&epic_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("UE_") {
                            // Extract version from directory name (e.g., UE_5.3 -> 5.3)
                            if let Some(version) = name.strip_prefix("UE_") {
                                if is_valid_engine_install(&path) {
                                    engines.push((version.to_string(), path));
                                }
                            }
                        }
                    }
                }
            }
        }
    } else if cfg!(target_os = "linux") {
        // Check common Linux locations
        if let Ok(home) = env::var("HOME") {
            // ~/UnrealEngine/UE_X.Y
            let ue_path = PathBuf::from(&home).join("UnrealEngine");
            if let Ok(entries) = fs::read_dir(&ue_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("UE_") {
                            if let Some(version) = name.strip_prefix("UE_") {
                                if is_valid_engine_install(&path) {
                                    engines.push((version.to_string(), path));
                                }
                            }
                        }
                    }
                }
            }

            // Check /opt/UnrealEngine
            let opt_path = PathBuf::from("/opt/UnrealEngine");
            if let Ok(entries) = fs::read_dir(&opt_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if is_valid_engine_install(&path) {
                        if let Some(version) = extract_engine_version(&path) {
                            engines.push((version, path));
                        }
                    }
                }
            }
        }
    } else if cfg!(target_os = "macos") {
        // Check macOS locations
        // /Users/Shared/Epic Games/UE_X.Y
        let epic_path = PathBuf::from("/Users/Shared/Epic Games");
        if let Ok(entries) = fs::read_dir(&epic_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("UE_") {
                        if let Some(version) = name.strip_prefix("UE_") {
                            if is_valid_engine_install(&path) {
                                engines.push((version.to_string(), path));
                            }
                        }
                    }
                }
            }
        }
    }

    engines
}

/// Check if a path is a valid Unreal Engine installation
fn is_valid_engine_install(path: &PathBuf) -> bool {
    // Check for Engine directory and UnrealBuildTool
    path.join("Engine").exists() && (
        path.join("Engine/Binaries/DotNET/UnrealBuildTool").exists() ||
        path.join("Engine/Binaries/DotNET/UnrealBuildTool.exe").exists() ||
        path.join("Engine/Binaries/DotNET/UnrealBuildTool.dll").exists()
    )
}

/// Extract engine version from installation path
fn extract_engine_version(path: &PathBuf) -> Option<String> {
    // Try to read version from Engine/Build/Build.version
    let version_file = path.join("Engine/Build/Build.version");
    if let Ok(content) = fs::read_to_string(&version_file) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(major) = json["MajorVersion"].as_u64() {
                if let Some(minor) = json["MinorVersion"].as_u64() {
                    return Some(format!("{}.{}", major, minor));
                }
            }
        }
    }

    // Fallback: try to extract from directory name
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if let Some(version) = name.strip_prefix("UE_") {
            return Some(version.to_string());
        }
    }

    None
}

/// Resolve engine path from EngineAssociation (e.g., "5.6", "{GUID}")
/// Uses Epic Games Launcher associations on Windows, config files on Linux
pub fn resolve_engine_association(engine_association: &str) -> Option<PathBuf> {
    // If it's a version string (e.g., "5.6"), try to find it
    if !engine_association.starts_with('{') {
        // Try auto-detection first
        let detected = detect_unreal_engines();
        if let Some((_, path)) = detected.into_iter().find(|(v, _)| v == engine_association) {
            return Some(path);
        }
    }

    // On Windows, check registry for GUID associations
    if cfg!(windows) {
        if let Some(path) = resolve_windows_engine_association(engine_association) {
            return Some(path);
        }
    }

    // On Linux, check ~/.config/Epic/UnrealEngine/Install.ini
    if cfg!(target_os = "linux") {
        if let Some(path) = resolve_linux_engine_association(engine_association) {
            return Some(path);
        }
    }

    None
}

#[cfg(windows)]
fn resolve_windows_engine_association(association: &str) -> Option<PathBuf> {
    use winreg::enums::*;
    use winreg::RegKey;

    // Open HKEY_CURRENT_USER\Software\Epic Games\Unreal Engine\Builds
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    if let Ok(builds_key) = hkcu.open_subkey("Software\\Epic Games\\Unreal Engine\\Builds") {
        // Try to read the value for this association (GUID or version)
        if let Ok(engine_path) = builds_key.get_value::<String, _>(association) {
            let path = PathBuf::from(engine_path);
            if is_valid_engine_install(&path) {
                return Some(path);
            }
        }
    }

    None
}

#[cfg(not(windows))]
fn resolve_windows_engine_association(_association: &str) -> Option<PathBuf> {
    None
}

fn resolve_linux_engine_association(association: &str) -> Option<PathBuf> {
    // Check ~/.config/Epic/UnrealEngine/Install.ini or similar
    if let Ok(home) = env::var("HOME") {
        let config_file = PathBuf::from(home).join(".config/Epic/UnrealEngine/Install.ini");
        if let Ok(content) = fs::read_to_string(&config_file) {
            // Parse INI format looking for engine associations
            for line in content.lines() {
                if line.starts_with(association) || line.contains(&format!("={}", association)) {
                    // Extract path from line
                    if let Some(path_str) = line.split('=').nth(1) {
                        let path = PathBuf::from(path_str.trim());
                        if is_valid_engine_install(&path) {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Convert WSL path to Windows path (e.g., /mnt/c/foo -> C:\foo)
pub fn wsl_to_windows_path(wsl_path: &std::path::Path) -> Option<String> {
    let path_str = wsl_path.to_string_lossy();

    // Check if it's a /mnt/ path
    if path_str.starts_with("/mnt/") {
        let parts: Vec<&str> = path_str.splitn(4, '/').collect();
        if parts.len() >= 3 {
            let drive = parts[2].to_uppercase();
            let rest = if parts.len() > 3 {
                parts[3].replace('/', "\\")
            } else {
                String::new()
            };
            return Some(format!("{}:\\{}", drive, rest));
        }
    }

    // If it's already a Windows path or not /mnt/, return as-is
    Some(path_str.to_string())
}

/// Convert Windows path to WSL path (e.g., C:\foo -> /mnt/c/foo)
pub fn windows_to_wsl_path(windows_path: &str) -> Option<String> {
    // Check if it's a Windows path (e.g., C:\foo)
    if windows_path.len() >= 3 && windows_path.chars().nth(1) == Some(':') {
        let drive = windows_path.chars().next()?.to_lowercase();
        let rest = windows_path[2..].replace('\\', "/");
        return Some(format!("/mnt/{}{}", drive, rest));
    }

    Some(windows_path.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_engine_version() {
        assert_eq!(normalize_engine_version("5.2.0"), "5.2");
        assert_eq!(normalize_engine_version("5.3"), "5.3");
        assert_eq!(normalize_engine_version("5.4.1"), "5.4");
    }

    #[test]
    fn test_detect_platform() {
        let platform = detect_platform();
        // Just make sure it returns something
        assert!(!platform.is_empty());
    }
}
