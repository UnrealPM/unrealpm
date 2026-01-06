//! User and project configuration management
//!
//! This module handles reading and writing UnrealPM configuration files.
//! Configuration is stored in TOML format at `~/.unrealpm/config.toml`.
//!
//! # Examples
//!
//! ```no_run
//! use unrealpm::Config;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load config
//! let config = Config::load()?;
//!
//! println!("Auto-build on install: {}", config.build.auto_build_on_install);
//! println!("Registry URL: {}", config.registry.url);
//!
//! // Modify and save
//! let mut config = config;
//! config.build.auto_build_on_publish = true;
//! config.save()?;
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// User configuration file (`~/.unrealpm/config.toml`)
///
/// Contains user-level settings including engine installations, build preferences,
/// and registry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Unreal Engine installations
    #[serde(default)]
    pub engines: Vec<EngineInstallation>,

    /// Build settings
    #[serde(default)]
    pub build: BuildConfig,

    /// Registry settings
    #[serde(default)]
    pub registry: RegistryConfig,

    /// Package signing settings
    #[serde(default)]
    pub signing: SigningConfig,

    /// Package verification settings
    #[serde(default)]
    pub verification: VerificationConfig,

    /// Authentication settings
    #[serde(default)]
    pub auth: AuthConfig,

    /// Dependency resolver settings
    #[serde(default)]
    pub resolver: ResolverConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineInstallation {
    pub version: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfig {
    /// Automatically build binaries when publishing
    #[serde(default)]
    pub auto_build_on_publish: bool,

    /// Automatically build binaries when installing (if no pre-built binary available)
    #[serde(default)]
    pub auto_build_on_install: bool,

    /// Target platforms to build for
    #[serde(default = "default_build_platforms")]
    pub platforms: Vec<String>,

    /// Build configuration (Development, Shipping, etc.)
    #[serde(default = "default_build_configuration")]
    pub configuration: String,
}

fn default_build_platforms() -> Vec<String> {
    vec!["Win64".to_string()]
}

fn default_build_configuration() -> String {
    "Development".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Registry type: "file" or "http"
    #[serde(default = "default_registry_type")]
    pub registry_type: String,

    /// Registry URL (for HTTP registry)
    #[serde(default = "default_registry_url")]
    pub url: String,
}

fn default_registry_type() -> String {
    "file".to_string() // Default to file-based for backward compatibility
}

fn default_registry_url() -> String {
    "http://localhost:3000".to_string() // Default to local development server
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningConfig {
    /// Enable package signing when publishing
    #[serde(default = "default_signing_enabled")]
    pub enabled: bool,

    /// Path to private signing key (PEM format)
    #[serde(default = "default_private_key_path")]
    pub private_key_path: String,

    /// Path to public verification key (PEM format)
    #[serde(default = "default_public_key_path")]
    pub public_key_path: String,
}

fn default_signing_enabled() -> bool {
    true // Signing enabled by default
}

fn default_private_key_path() -> String {
    "~/.unrealpm/keys/signing_key.pem".to_string()
}

fn default_public_key_path() -> String {
    "~/.unrealpm/keys/public_key.pem".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    /// Require signature verification when installing packages
    #[serde(default)]
    pub require_signatures: bool, // false for now (v0.3.0), true in v1.0.0

    /// If true, fail on signature verification errors
    /// If false, show warning and continue (useful for testing/development)
    #[serde(default = "default_strict_verification")]
    pub strict_verification: bool,
}

fn default_strict_verification() -> bool {
    true
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            require_signatures: false,
            strict_verification: default_strict_verification(),
        }
    }
}

/// Dependency resolver settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolverConfig {
    /// Maximum dependency depth to prevent infinite recursion (default: 100)
    #[serde(default = "default_max_depth")]
    pub max_depth: usize,

    /// Show full derivation tree in conflict errors for debugging
    #[serde(default)]
    pub verbose_conflicts: bool,

    /// Timeout for resolution in seconds (0 = no timeout)
    #[serde(default)]
    pub resolution_timeout_seconds: u64,
}

fn default_max_depth() -> usize {
    100
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            max_depth: default_max_depth(),
            verbose_conflicts: false,
            resolution_timeout_seconds: 0,
        }
    }
}

impl Default for SigningConfig {
    fn default() -> Self {
        Self {
            enabled: default_signing_enabled(),
            private_key_path: default_private_key_path(),
            public_key_path: default_public_key_path(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// API token for publishing to HTTP registry
    pub token: Option<String>,
}

impl AuthConfig {
    /// Format authorization header based on token type
    /// API tokens (starting with "urpm_") use "Token <token>" format
    /// JWT session tokens use "Bearer <token>" format
    pub fn format_auth_header(token: &str) -> String {
        if token.starts_with("urpm_") {
            format!("Token {}", token)
        } else {
            format!("Bearer {}", token)
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            engines: Vec::new(),
            build: BuildConfig {
                auto_build_on_publish: false,
                auto_build_on_install: false,
                platforms: default_build_platforms(),
                configuration: default_build_configuration(),
            },
            registry: RegistryConfig {
                registry_type: default_registry_type(),
                url: default_registry_url(),
            },
            signing: SigningConfig::default(),
            verification: VerificationConfig::default(),
            auth: AuthConfig::default(),
            resolver: ResolverConfig::default(),
        }
    }
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            auto_build_on_publish: false,
            auto_build_on_install: false,
            platforms: default_build_platforms(),
            configuration: default_build_configuration(),
        }
    }
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            registry_type: default_registry_type(),
            url: default_registry_url(),
        }
    }
}

impl Config {
    /// Get the default config file path
    ///
    /// Uses UNREALPM_CONFIG_DIR if set, otherwise ~/.unrealpm/config.toml
    pub fn default_path() -> Result<PathBuf> {
        // Check for custom config directory (useful for testing)
        if let Ok(config_dir) = std::env::var("UNREALPM_CONFIG_DIR") {
            return Ok(PathBuf::from(config_dir).join("config.toml"));
        }

        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| Error::Other("Could not find home directory".to_string()))?;

        Ok(PathBuf::from(home).join(".unrealpm").join("config.toml"))
    }

    /// Load config from file, or create default if it doesn't exist
    ///
    /// Environment variable overrides:
    /// - `UNREALPM_TOKEN`: Overrides `auth.token` for API authentication
    /// - `UNREALPM_CONFIG_DIR`: Overrides the config directory location
    pub fn load() -> Result<Self> {
        let path = Self::default_path()?;

        let mut config = if !path.exists() {
            // Return default config
            Self::default()
        } else {
            let content = fs::read_to_string(&path)?;
            toml::from_str(&content)?
        };

        // Override auth token from environment if set
        if let Ok(token) = std::env::var("UNREALPM_TOKEN") {
            if !token.is_empty() {
                config.auth.token = Some(token);
            }
        }

        Ok(config)
    }

    /// Save config to file
    pub fn save(&self) -> Result<()> {
        let path = Self::default_path()?;

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = toml::to_string_pretty(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Find an engine installation by version
    /// Checks configured engines first, then auto-detection, then EngineAssociation resolution
    pub fn find_engine(&self, version: &str) -> Option<EngineInstallation> {
        // Check configured engines first
        if let Some(engine) = self.engines.iter().find(|e| e.version == version) {
            return Some(engine.clone());
        }

        // Try auto-detection
        let detected = crate::platform::detect_unreal_engines();
        if let Some((version, path)) = detected.into_iter().find(|(v, _)| v == version) {
            return Some(EngineInstallation { version, path });
        }

        // Try resolving from EngineAssociation (handles GUIDs and version strings)
        if let Some(path) = crate::platform::resolve_engine_association(version) {
            return Some(EngineInstallation {
                version: version.to_string(),
                path,
            });
        }

        None
    }

    /// Get all available engines (configured + auto-detected)
    pub fn get_all_engines(&self) -> Vec<EngineInstallation> {
        let mut all_engines = self.engines.clone();

        // Add auto-detected engines that aren't already configured
        let detected = crate::platform::detect_unreal_engines();
        for (version, path) in detected {
            if !all_engines.iter().any(|e| e.version == version) {
                all_engines.push(EngineInstallation { version, path });
            }
        }

        all_engines.sort_by(|a, b| a.version.cmp(&b.version));
        all_engines
    }

    /// Add an engine installation
    pub fn add_engine(&mut self, version: String, path: PathBuf) {
        // Remove existing entry for this version if it exists
        self.engines.retain(|e| e.version != version);

        self.engines.push(EngineInstallation { version, path });
    }

    /// Remove an engine installation
    pub fn remove_engine(&mut self, version: &str) {
        self.engines.retain(|e| e.version != version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.build.platforms, vec!["Win64"]);
        assert_eq!(config.build.configuration, "Development");
        assert!(!config.build.auto_build_on_publish);
    }

    #[test]
    fn test_engine_management() {
        let mut config = Config::default();

        config.add_engine("5.3".to_string(), PathBuf::from("/path/to/ue5.3"));
        assert_eq!(config.engines.len(), 1);

        let engine = config.find_engine("5.3");
        assert!(engine.is_some());

        config.remove_engine("5.3");
        assert_eq!(config.engines.len(), 0);
    }
}
