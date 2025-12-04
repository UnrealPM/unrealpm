//! UnrealPM - A modern package manager for Unreal Engine plugins
//!
//! UnrealPM brings the developer experience of npm, Cargo, and pip to the Unreal Engine ecosystem.
//! It provides a simple CLI for managing plugin dependencies with features like:
//!
//! - Transitive dependency resolution with circular dependency detection
//! - TOML-based lockfiles with SHA256 checksums for reproducible builds
//! - Engine version filtering to ensure compatibility
//! - Hybrid binary/source package support
//! - Automated plugin building via RunUAT
//! - WSL support for seamless Windows Unreal Engine access from Linux
//!
//! # Examples
//!
//! ```no_run
//! use unrealpm::{Manifest, RegistryClient, resolve_dependencies};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Load project manifest
//! let manifest = Manifest::load(".")?;
//!
//! // Create registry client
//! let registry = RegistryClient::new(std::env::var("HOME").unwrap() + "/.unrealpm-registry");
//!
//! // Resolve dependencies
//! let engine_version = Some("5.3");
//! let resolved = resolve_dependencies(&manifest.dependencies, &registry, engine_version, false)?;
//!
//! println!("Resolved {} packages", resolved.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Modules
//!
//! - [`manifest`] - Parse and manage unrealpm.json and .uproject files
//! - [`registry`] - Interact with the package registry
//! - [`resolver`] - Resolve package dependencies with semantic versioning
//! - [`installer`] - Install packages and verify checksums
//! - [`lockfile`] - Manage unrealpm.lock for reproducible builds
//! - [`platform`] - Platform detection and Unreal Engine path resolution
//! - [`config`] - User and project configuration management
//! - [`error`] - Error types and result handling

pub mod config;
pub mod error;
pub mod installer;
pub mod lockfile;
pub mod manifest;
pub mod platform;
pub mod registry;
pub mod registry_http;
pub mod resolver;
pub mod signing;

pub use config::Config;
pub use error::{Error, Result};
pub use installer::{install_package, verify_checksum, ProgressCallback};
pub use lockfile::{LockedPackage, Lockfile, LOCKFILE_NAME};
pub use manifest::{Manifest, UPlugin, UProject};
pub use platform::{
    detect_platform, detect_unreal_engines, normalize_engine_version, resolve_engine_association,
    wsl_to_windows_path,
};
pub use registry::{
    Dependency, PackageMetadata, PackageType, PackageVersion, PrebuiltBinary, RegistryClient,
};
pub use resolver::{find_matching_version, resolve_dependencies, ResolvedPackage};
pub use signing::{load_or_generate_keys, verify_signature, PackageSigningKey};
