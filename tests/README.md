# UnrealPM CLI Test Suite

This directory contains integration tests for the UnrealPM CLI against the production registry at https://registry.unreal.dev/

## Test Categories

### 1. Registry Integration Tests (`registry_integration_tests.rs`)

Tests the full CLI workflow against the production registry:

- **Read-only tests** - Safe to run anytime without authentication
  - Search functionality
  - Package installation
  - Lockfile handling
  - Signature verification
  - Error handling

- **Authenticated tests** - Require login (marked with `#[ignore]`)
  - Token management
  - Publishing (dry-run)

### 2. API Tests (`api_tests.rs`)

Direct HTTP API tests that verify registry endpoints:

- Package list endpoint (`GET /api/v1/packages`)
- Package detail endpoint (`GET /api/v1/packages/:name`)
- Version detail endpoint (`GET /api/v1/packages/:name/:version`)
- Download endpoint (`GET /api/v1/packages/:name/:version/download`)
- Signature endpoint (`GET /api/v1/packages/:name/:version/signature`)
- Authentication requirements
- Response format validation
- Performance benchmarks

### 3. Test Utilities (`test_utils.rs`)

Shared utilities for test setup:

- `TestProject` - Creates isolated test project directories
- `MockPlugin` - Creates mock plugin structures
- `TestRegistry` - Creates file-based test registries
- Assertion helpers

## Running Tests

### Quick Start

```bash
# Run all safe tests (no authentication required)
cargo test --test registry_integration_tests
cargo test --test api_tests

# Run a specific test
cargo test --test registry_integration_tests test_install_package

# Run with output visible
cargo test --test api_tests -- --nocapture
```

### Running Authenticated Tests

First, log in to the registry:

```bash
unrealpm login
```

Then run the ignored tests:

```bash
cargo test --test registry_integration_tests -- --ignored
```

### Running All Tests

```bash
# All tests including ignored ones
cargo test --test registry_integration_tests -- --include-ignored
cargo test --test api_tests -- --include-ignored
```

### Test Filtering

```bash
# Run only read-only tests
cargo test --test registry_integration_tests read_only

# Run only download tests
cargo test --test registry_integration_tests download

# Run only error handling tests
cargo test --test registry_integration_tests errors

# Run only API list tests
cargo test --test api_tests list_packages

# Run only performance tests
cargo test --test api_tests performance
```

## Test Environment

Tests are designed to be **isolated** and **non-destructive**:

1. **Isolated directories** - Each test creates its own temp directory
2. **Isolated config** - Tests use their own config files
3. **No writes to registry** - Read-only tests don't modify server state
4. **Safe package** - Tests use `chromasense` which exists in the registry

### Environment Variables

- `UNREALPM_CONFIG_DIR` - Override config directory (used internally by tests)

## Writing New Tests

### Basic Test Structure

```rust
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

#[test]
fn test_my_feature() {
    // 1. Set up test project
    let temp_dir = setup_test_project();
    configure_http_registry(temp_dir.path());
    create_test_uproject(temp_dir.path(), "5.3");

    // 2. Run CLI command
    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());

    cmd.current_dir(&temp_dir)
        .arg("my-command")
        .assert()
        .success()
        .stdout(predicate::str::contains("expected output"));

    // 3. Verify results
    assert!(temp_dir.path().join("expected-file").exists());
}
```

### Using Test Utilities

```rust
use crate::test_utils::{TestProject, MockPlugin, assertions};

#[test]
fn test_with_utils() {
    // Create project with engine version
    let project = TestProject::with_engine("5.4");
    project.configure_http_registry("https://registry.unreal.dev");

    // ... run tests ...

    // Use assertions
    assertions::file_contains(
        &project.path().join("unrealpm.json"),
        "chromasense"
    );
}
```

### Testing Error Cases

```rust
#[test]
fn test_error_case() {
    let temp_dir = setup_test_project();
    configure_http_registry(temp_dir.path());

    let mut cmd = unrealpm_cmd();
    with_test_config(&mut cmd, temp_dir.path());

    cmd.arg("install")
        .arg("nonexistent-package")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}
```

## Test Patterns

### 1. Registry Availability Check

For tests that depend on registry being available:

```rust
#[test]
fn test_with_registry_check() {
    let client = reqwest::blocking::Client::new();
    let response = client.get(REGISTRY_URL).send();

    if response.is_err() || !response.unwrap().status().is_success() {
        eprintln!("Skipping test - registry unavailable");
        return;
    }

    // ... actual test ...
}
```

### 2. Package Existence Check

When testing with real packages:

```rust
#[test]
fn test_with_package() {
    // First verify the package exists
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/api/v1/packages/{}", REGISTRY_URL, TEST_PACKAGE);

    if !client.get(&url).send().map(|r| r.status().is_success()).unwrap_or(false) {
        eprintln!("Test package not available");
        return;
    }

    // ... test with package ...
}
```

### 3. Cleanup Pattern

Tests automatically clean up via `TempDir`:

```rust
#[test]
fn test_with_cleanup() {
    let temp_dir = TempDir::new().unwrap(); // Auto-cleaned on drop

    // ... test code ...

} // temp_dir dropped here, directory deleted
```

## Troubleshooting

### Tests Fail with Connection Errors

The registry may be temporarily unavailable. Wait and retry:

```bash
# Check registry status
curl -s https://registry.unreal.dev/api/v1/packages | head
```

### Tests Fail Due to Missing Package

The test package may have been unpublished. Update `TEST_PACKAGE` constant:

```rust
const TEST_PACKAGE: &str = "chromasense"; // Update if needed
```

### Authenticated Tests Fail

Ensure you're logged in:

```bash
unrealpm login
unrealpm tokens list  # Verify token exists
```

### Performance Tests Fail

Network latency may cause timeouts. Adjust thresholds or skip:

```bash
# Skip performance tests
cargo test --test api_tests -- --skip performance
```

## CI/CD Integration

For CI pipelines:

```yaml
# Example GitHub Actions
test:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - name: Install Rust
      uses: actions-rs/toolchain@v1
      with:
        toolchain: stable

    - name: Run read-only tests
      run: |
        cd cli
        cargo test --test registry_integration_tests
        cargo test --test api_tests
```

Note: Authenticated tests should not run in CI without proper secrets management.
