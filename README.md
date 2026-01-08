# UnrealPM

**A modern package manager for Unreal Engine plugins**

UnrealPM brings the developer experience of npm, Cargo, and pip to the Unreal Engine ecosystem - dependency resolution, lockfiles, and reproducible builds for your UE plugins.

## Features

- **Dependency Resolution** - Automatically resolves and installs transitive dependencies
- **Lockfile Support** - Reproducible builds with `unrealpm.lock`
- **Engine Version Filtering** - Only installs plugins compatible with your UE version
- **Checksum Verification** - SHA256 verification for all downloaded packages
- **Package Signing** - Ed25519 signatures for package authenticity
- **HTTP Registry** - Full-featured package registry with web UI
- **Simple CLI** - Familiar commands: `init`, `install`, `uninstall`, `publish`, `search`

## Installation

### Download Binary

Download the latest release for your platform from [Releases](https://github.com/UnrealPM/unrealpm/releases):

- **Linux x64** - `unrealpm`
- **Windows x64** - `unrealpm.exe`

```bash
# Linux
chmod +x unrealpm
sudo mv unrealpm /usr/local/bin/

# Windows - add to PATH or place in project directory
```

### Build from Source

```bash
git clone https://github.com/UnrealPM/unrealpm.git
cd unrealpm
cargo build --release
# Binary at target/release/unrealpm
```

## Quick Start

```bash
# Initialize a new project (in your UE project directory)
unrealpm init

# Search for plugins
unrealpm search multiplayer

# Install a plugin
unrealpm install awesome-plugin

# Install with version constraint
unrealpm install awesome-plugin@^1.0.0

# List installed packages
unrealpm list

# Show dependency tree
unrealpm tree

# Uninstall a plugin
unrealpm uninstall awesome-plugin
```

## Publishing Plugins

```bash
# Login to registry
unrealpm login

# Publish from plugin directory (dry-run first)
cd MyPlugin
unrealpm publish --dry-run

# Publish for real
unrealpm publish
```

## Commands

| Command | Description |
|---------|-------------|
| `init` | Initialize a new UnrealPM project |
| `install [package]` | Install dependencies or a specific package |
| `install --offline` | Install from lockfile and cache only (no network) |
| `uninstall <package>` | Remove a package |
| `update [package]` | Update dependencies |
| `list` | List installed packages |
| `tree` | Show dependency tree |
| `search <query>` | Search for packages |
| `pack` | Create package tarball without publishing |
| `publish` | Publish a plugin to the registry |
| `unpublish <package>` | Delete a package or version |
| `yank <package@version>` | Deprecate a version (prevent new installs) |
| `unyank <package@version>` | Un-deprecate a version |
| `register` | Create account with email/password |
| `login` | Authenticate with the registry (supports `--github`) |
| `logout` | Clear authentication |
| `whoami` | Show current logged-in user |
| `tokens create` | Create a long-lived API token |
| `tokens list` | List your API tokens |
| `tokens revoke` | Revoke an API token |
| `cache list` | List cached packages |
| `cache info` | Show cache statistics |
| `cache path` | Show cache directory path |
| `cache clean` | Remove unused packages from cache |
| `cache verify` | Verify cache integrity |
| `config` | View or modify configuration |
| `doctor` | Diagnose setup issues (with `--fix` for auto-repair) |
| `verify <package>` | Verify package signature |
| `why <package>` | Explain why a package is installed |
| `outdated` | Show outdated packages |
| `keys` | Manage signing keys |
| `build` | Build plugin binaries |
| `completions` | Generate shell completions |

## Configuration

UnrealPM stores configuration in `~/.unrealpm/config.toml`:

```bash
# Show current configuration
unrealpm config show

# Set registry URL
unrealpm config set registry.url https://registry.unreal.dev
```

## Project Manifest

UnrealPM creates an `unrealpm.json` manifest in your project:

```json
{
  "name": "MyGame",
  "engine_version": "5.4",
  "dependencies": {
    "awesome-plugin": "^1.0.0",
    "networking-utils": "^2.1.0"
  }
}
```

## Security

- **Package Signing** - All packages signed with Ed25519
- **Automatic Verification** - Signatures verified on install
- **Key Management** - `unrealpm keys generate` / `unrealpm keys show`

## Registry

The public registry is at [registry.unreal.dev](https://registry.unreal.dev).

## License

Copyright 2025 UnrealPM. All rights reserved.

## Contributing

Contributions welcome! Please open an issue or pull request.
