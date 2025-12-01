use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML deserialize error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Version parsing error: {0}")]
    SemVer(#[from] semver::Error),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Dependency conflict: {0}")]
    DependencyConflict(String),

    #[error("Invalid manifest: {0}")]
    InvalidManifest(String),

    #[error("No .uproject file found in current directory\n\n\
             Hint: Make sure you're running this command from your Unreal Engine project root.\n\
             The project root should contain a .uproject file.\n\n\
             Example structure:\n\
             MyProject/\n\
             ├── MyProject.uproject  ← This file is required\n\
             ├── Config/\n\
             ├── Content/\n\
             └── Source/\n\n\
             Try: cd /path/to/your/project")]
    NoUProjectFile,

    #[error("Unreal Engine installation not found{}\n\n\
             Hint: UnrealPM couldn't detect an Unreal Engine installation on your system.\n\n\
             Common locations:\n\
             - Windows: C:\\Program Files\\Epic Games\\UE_5.x\n\
             - Linux:   ~/UnrealEngine\n\
             - macOS:   /Users/Shared/Epic Games/UE_5.x\n\n\
             Solutions:\n\
             1. Install Unreal Engine from Epic Games Launcher\n\
             2. Manually configure the engine path:\n\
                unrealpm config add-engine \"5.3\" \"/path/to/UE_5.3\"\n\
             3. Verify your .uproject has a valid EngineAssociation field",
             .0)]
    EngineNotFound(String),

    #[error("Dependency resolution failed: {0}\n\n\
             Hint: This usually means conflicting version requirements.\n\n\
             Possible solutions:\n\
             1. Check your unrealpm.json for incompatible version constraints\n\
             2. Update package versions to compatible ranges\n\
             3. Use --force to bypass version checks (not recommended)\n\n\
             Need help? Run: unrealpm list --verbose")]
    DependencyResolutionFailed(String),

    #[error("{0}")]
    Other(String),
}
