#!/bin/bash
# UnrealPM CLI Test Runner
#
# This script provides convenient ways to run the test suite against
# the production registry at https://registry.unreal.dev/

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Registry URL
REGISTRY_URL="https://registry.unreal.dev"

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI_DIR="$(dirname "$SCRIPT_DIR")"

print_header() {
    echo -e "${BLUE}========================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}========================================${NC}"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

# Check if registry is available
check_registry() {
    echo "Checking registry availability..."
    if curl -s --fail "$REGISTRY_URL/api/v1/packages" > /dev/null 2>&1; then
        print_success "Registry is available at $REGISTRY_URL"
        return 0
    else
        print_error "Registry is not available at $REGISTRY_URL"
        return 1
    fi
}

# Build the CLI
build_cli() {
    print_header "Building CLI"
    cd "$CLI_DIR"
    cargo build
    print_success "Build complete"
}

# Run read-only integration tests
run_readonly_tests() {
    print_header "Running Read-Only Integration Tests"
    cd "$CLI_DIR"

    echo "Running registry integration tests..."
    cargo test --test registry_integration_tests -- --test-threads=1 2>&1 | tee /tmp/test_output.txt

    if [ ${PIPESTATUS[0]} -eq 0 ]; then
        print_success "All read-only tests passed"
    else
        print_error "Some tests failed"
        return 1
    fi
}

# Run API tests
run_api_tests() {
    print_header "Running API Tests"
    cd "$CLI_DIR"

    echo "Running API tests..."
    cargo test --test api_tests -- --test-threads=1 2>&1 | tee /tmp/api_test_output.txt

    if [ ${PIPESTATUS[0]} -eq 0 ]; then
        print_success "All API tests passed"
    else
        print_error "Some API tests failed"
        return 1
    fi
}

# Run authenticated tests
run_auth_tests() {
    print_header "Running Authenticated Tests"

    # Check if user is logged in
    if ! "$CLI_DIR/target/debug/unrealpm" tokens list > /dev/null 2>&1; then
        print_warning "Not logged in. Please run 'unrealpm login' first."
        echo "Skipping authenticated tests."
        return 0
    fi

    cd "$CLI_DIR"
    echo "Running authenticated tests..."
    cargo test --test registry_integration_tests -- --ignored --test-threads=1 2>&1 | tee /tmp/auth_test_output.txt

    if [ ${PIPESTATUS[0]} -eq 0 ]; then
        print_success "All authenticated tests passed"
    else
        print_error "Some authenticated tests failed"
        return 1
    fi
}

# Run performance tests
run_perf_tests() {
    print_header "Running Performance Tests"
    cd "$CLI_DIR"

    echo "Running performance tests..."
    cargo test --test api_tests performance -- --nocapture 2>&1 | tee /tmp/perf_test_output.txt

    if [ ${PIPESTATUS[0]} -eq 0 ]; then
        print_success "All performance tests passed"
    else
        print_error "Some performance tests failed"
        return 1
    fi
}

# Run specific test module
run_module_tests() {
    local module=$1
    print_header "Running Tests: $module"
    cd "$CLI_DIR"

    cargo test --test registry_integration_tests "$module" -- --test-threads=1 --nocapture 2>&1

    if [ $? -eq 0 ]; then
        print_success "Module tests passed: $module"
    else
        print_error "Module tests failed: $module"
        return 1
    fi
}

# Run all tests
run_all_tests() {
    print_header "Running All Tests"

    local failed=0

    run_api_tests || failed=1
    run_readonly_tests || failed=1
    run_perf_tests || failed=1

    if [ $failed -eq 0 ]; then
        print_header "All Tests Passed!"
        return 0
    else
        print_header "Some Tests Failed"
        return 1
    fi
}

# Show help
show_help() {
    echo "UnrealPM CLI Test Runner"
    echo ""
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  all         Run all tests (excluding authenticated)"
    echo "  api         Run API endpoint tests"
    echo "  readonly    Run read-only integration tests"
    echo "  auth        Run authenticated tests (requires login)"
    echo "  perf        Run performance tests"
    echo "  module NAME Run specific test module"
    echo "              (read_only, download, verification, errors,"
    echo "               dependencies, engine_version, package_types)"
    echo "  check       Check if registry is available"
    echo "  build       Build the CLI"
    echo "  help        Show this help"
    echo ""
    echo "Examples:"
    echo "  $0 all                   # Run all safe tests"
    echo "  $0 api                   # Run only API tests"
    echo "  $0 module read_only      # Run read_only module"
    echo "  $0 auth                  # Run authenticated tests"
    echo ""
    echo "Test Modules:"
    echo "  read_only      - Basic read operations (search, list, config)"
    echo "  download       - Package download and installation"
    echo "  verification   - Signature verification"
    echo "  errors         - Error handling"
    echo "  dependencies   - Dependency resolution (tree, outdated, why)"
    echo "  engine_version - Engine version compatibility"
    echo "  package_types  - Source/binary/hybrid installation"
    echo "  authenticated  - Tests requiring login"
}

# Main entry point
main() {
    cd "$CLI_DIR"

    case "${1:-help}" in
        all)
            check_registry && build_cli && run_all_tests
            ;;
        api)
            check_registry && build_cli && run_api_tests
            ;;
        readonly)
            check_registry && build_cli && run_readonly_tests
            ;;
        auth)
            check_registry && build_cli && run_auth_tests
            ;;
        perf)
            check_registry && build_cli && run_perf_tests
            ;;
        module)
            if [ -z "$2" ]; then
                print_error "Please specify a module name"
                exit 1
            fi
            check_registry && build_cli && run_module_tests "$2"
            ;;
        check)
            check_registry
            ;;
        build)
            build_cli
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            print_error "Unknown command: $1"
            show_help
            exit 1
            ;;
    esac
}

main "$@"
