use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};

mod commands;

/// UnrealPM - A modern package manager for Unreal Engine plugins
#[derive(Parser)]
#[command(name = "unrealpm")]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new UnrealPM project
    Init,

    /// Install a package
    Install {
        /// Package name (e.g., awesome-plugin@1.2.0)
        package: Option<String>,

        /// Force install even if engine version is incompatible
        #[arg(short, long)]
        force: bool,

        /// Override engine version (e.g., --engine-version 5.3)
        #[arg(short, long)]
        engine_version: Option<String>,

        /// Prefer pre-built binaries, fall back to source if not available
        #[arg(long)]
        prefer_binary: bool,

        /// Only install from source (skip pre-built binaries)
        #[arg(long)]
        source_only: bool,

        /// Only install pre-built binaries (fail if not available)
        #[arg(long, conflicts_with = "source_only")]
        binary_only: bool,

        /// Show what would be installed without actually installing
        #[arg(long)]
        dry_run: bool,

        /// Show verbose conflict information during dependency resolution
        #[arg(long)]
        verbose_resolve: bool,

        /// Maximum dependency depth (default: 100)
        #[arg(long)]
        max_depth: Option<usize>,

        /// Resolution timeout in seconds (0 = no timeout)
        #[arg(long)]
        resolve_timeout: Option<u64>,
    },

    /// Uninstall a package
    Uninstall {
        /// Package name
        package: String,
    },

    /// Update packages
    Update {
        /// Specific package to update (optional)
        package: Option<String>,

        /// Show what would be updated without actually updating
        #[arg(long)]
        dry_run: bool,

        /// Show verbose conflict information during dependency resolution
        #[arg(long)]
        verbose_resolve: bool,

        /// Maximum dependency depth (default: 100)
        #[arg(long)]
        max_depth: Option<usize>,

        /// Resolution timeout in seconds (0 = no timeout)
        #[arg(long)]
        resolve_timeout: Option<u64>,
    },

    /// List installed packages
    List,

    /// Check for outdated packages
    Outdated,

    /// Show dependency tree
    Tree,

    /// Explain why a package is installed
    Why {
        /// Package name
        package: String,
    },

    /// Search for packages in the registry
    Search {
        /// Search query
        query: String,
    },

    /// Publish a package to the registry
    Publish {
        /// Path to plugin directory (defaults to current directory)
        path: Option<String>,

        /// Show what would be published without actually publishing
        #[arg(long)]
        dry_run: bool,

        /// Include Binaries/ folder in package
        #[arg(long)]
        include_binaries: bool,

        /// Target engine version (e.g., 4.27, 5.3) - for engine-specific builds
        #[arg(long)]
        engine: Option<String>,

        /// Git repository URL (for automatic updates)
        #[arg(long)]
        git_repo: Option<String>,

        /// Git tag/branch for this version
        #[arg(long)]
        git_ref: Option<String>,
    },

    /// Build plugin binaries for specified engine/platform
    Build {
        /// Path to plugin directory (defaults to current directory)
        path: Option<String>,

        /// Engine version to build for (e.g., 5.3)
        #[arg(short, long)]
        engine: Option<String>,

        /// Platform to build for (Win64, Linux, Mac)
        #[arg(short, long)]
        platform: Option<String>,

        /// Build all configured platforms
        #[arg(long)]
        all_platforms: bool,
    },

    /// Manage the package cache
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Manage signing keys
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },

    /// Verify package signature
    Verify {
        /// Package name with optional version (e.g., awesome-plugin@1.0.0)
        package: String,
    },

    /// Register for UnrealPM registry
    Register,

    /// Login to UnrealPM registry
    Login {
        /// Use GitHub OAuth for authentication
        #[arg(long, conflicts_with = "email")]
        github: bool,

        /// Use email/password for authentication
        #[arg(long, conflicts_with = "github")]
        email: bool,
    },

    /// Logout from UnrealPM registry
    Logout,

    /// Show current logged-in user
    Whoami,

    /// Unpublish a package version (or entire package)
    Unpublish {
        /// Package name with optional version (e.g., my-plugin@1.0.0 or my-plugin)
        package: String,

        /// Specific version to unpublish (alternative to package@version syntax)
        #[arg(short, long)]
        version: Option<String>,
    },

    /// Yank a package version (prevent new installs)
    Yank {
        /// Package name with version (e.g., my-plugin@1.0.0)
        package: String,
    },

    /// Un-yank a package version (allow installs again)
    Unyank {
        /// Package name with version (e.g., my-plugin@1.0.0)
        package: String,
    },

    /// Manage API tokens
    Tokens {
        #[command(subcommand)]
        action: TokensAction,
    },

    /// Generate shell completion scripts
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum TokensAction {
    /// Create a new API token
    Create {
        /// Token name (e.g., "My Laptop", "CI/CD")
        name: String,

        /// Token scopes (default: read,publish)
        #[arg(short, long, value_delimiter = ',')]
        scopes: Vec<String>,

        /// Expire after N days (omit for permanent token)
        #[arg(short, long)]
        expires: Option<i64>,
    },

    /// List your API tokens
    List,

    /// Revoke (delete) an API token
    Revoke {
        /// Token ID to revoke
        token_id: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show,

    /// Set a configuration value
    Set {
        /// Configuration key (e.g., build.auto_build_on_publish)
        key: String,
        /// Configuration value
        value: String,
    },

    /// Add an Unreal Engine installation
    AddEngine {
        /// Engine version (e.g., 5.3)
        version: String,
        /// Path to engine installation
        path: String,
    },

    /// Remove an Unreal Engine installation
    RemoveEngine {
        /// Engine version to remove
        version: String,
    },

    /// List configured engine installations
    ListEngines,
}

#[derive(Subcommand)]
enum KeysAction {
    /// Generate new signing keys
    Generate,

    /// Show public key
    Show,
}

#[derive(Subcommand)]
enum CacheAction {
    /// List cached packages
    List {
        /// Show verbose output with full hashes and paths
        #[arg(short, long)]
        verbose: bool,
    },

    /// Show cache statistics
    Info,

    /// Show cache directory path
    Path,

    /// Remove unused packages from cache
    Clean {
        /// Remove ALL packages (not just unused)
        #[arg(long)]
        all: bool,

        /// Show what would be removed without actually removing
        #[arg(long)]
        dry_run: bool,
    },

    /// Verify cache integrity
    Verify,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => commands::init::run(),
        Commands::Install {
            package,
            force,
            engine_version,
            prefer_binary,
            source_only,
            binary_only,
            dry_run,
            verbose_resolve,
            max_depth,
            resolve_timeout,
        } => commands::install::run(
            package,
            force,
            engine_version,
            prefer_binary,
            source_only,
            binary_only,
            dry_run,
            verbose_resolve,
            max_depth,
            resolve_timeout,
        ),
        Commands::Uninstall { package } => commands::uninstall::run(package),
        Commands::Update {
            package,
            dry_run,
            verbose_resolve,
            max_depth,
            resolve_timeout,
        } => commands::update::run(
            package,
            dry_run,
            verbose_resolve,
            max_depth,
            resolve_timeout,
        ),
        Commands::List => commands::list::run(),
        Commands::Outdated => commands::outdated::run(),
        Commands::Tree => commands::tree::run(),
        Commands::Why { package } => commands::why::run(package),
        Commands::Search { query } => commands::search::run(query),
        Commands::Publish {
            path,
            dry_run,
            include_binaries,
            engine,
            git_repo,
            git_ref,
        } => commands::publish::run(path, dry_run, include_binaries, engine, git_repo, git_ref),
        Commands::Build {
            path,
            engine,
            platform,
            all_platforms,
        } => commands::build::run(path, engine, platform, all_platforms),
        Commands::Cache { action } => match action {
            CacheAction::List { verbose } => commands::cache::run_list(verbose),
            CacheAction::Info => commands::cache::run_info(),
            CacheAction::Path => commands::cache::run_path(),
            CacheAction::Clean { all, dry_run } => commands::cache::run_clean(all, dry_run),
            CacheAction::Verify => commands::cache::run_verify(),
        },
        Commands::Config { action } => commands::config::run(&action),
        Commands::Keys { action } => commands::keys::run(&action),
        Commands::Verify { package } => commands::verify::run(package),
        Commands::Register => commands::register::run(),
        Commands::Login { github, email } => commands::login::run(github, email),
        Commands::Logout => commands::login::run_logout(),
        Commands::Whoami => commands::whoami::run(),
        Commands::Unpublish { package, version } => commands::unpublish::run(package, version),
        Commands::Yank { package } => commands::yank::run(package, false),
        Commands::Unyank { package } => commands::yank::run(package, true),
        Commands::Tokens { action } => match action {
            TokensAction::Create {
                name,
                scopes,
                expires,
            } => commands::tokens::run_create(name, scopes, expires),
            TokensAction::List => commands::tokens::run_list(),
            TokensAction::Revoke { token_id } => commands::tokens::run_revoke(token_id),
        },
        Commands::Completions { shell } => {
            let mut cmd = Cli::command();
            generate(shell, &mut cmd, "unrealpm", &mut std::io::stdout());
            Ok(())
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
