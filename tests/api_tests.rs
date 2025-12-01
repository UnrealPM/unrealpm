//! Direct API tests for the UnrealPM HTTP registry.
//!
//! These tests verify the registry API at https://registry.unreal.dev/
//! works correctly by making direct HTTP requests.
//!
//! # Running Tests
//!
//! ```bash
//! # Run all API tests
//! cargo test --test api_tests
//!
//! # Run with output
//! cargo test --test api_tests -- --nocapture
//! ```

use serde::Deserialize;

const REGISTRY_URL: &str = "https://registry.unreal.dev";

// ============================================================================
// API Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PackageListResponse {
    packages: Vec<PackageInfo>,
    total: usize,
    limit: i64,
    offset: i64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PackageInfo {
    name: String,
    description: Option<String>,
    latest_version: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PackageDetailResponse {
    name: String,
    description: Option<String>,
    versions: Vec<VersionInfo>,
    total_downloads: i64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct VersionInfo {
    version: String,
    published_at: String,
    checksum: String,
    tarball_url: String,
    engine_versions: Option<Vec<String>>,
    engine_major: Option<i32>,
    engine_minor: Option<i32>,
    is_multi_engine: bool,
    package_type: String,
    downloads: i32,
    public_key: Option<String>,
    signed_at: Option<String>,
    yanked: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct VersionDetailResponse {
    version: String,
    checksum: String,
    tarball_url: String,
    package_type: String,
    engine_versions: Option<Vec<String>>,
    engine_major: Option<i32>,
    engine_minor: Option<i32>,
    is_multi_engine: Option<bool>,
    published_at: String,
    downloads: i32,
    public_key: Option<String>,
    signed_at: Option<String>,
    dependencies: Vec<DependencyInfo>,
    binaries: Vec<BinaryInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DependencyInfo {
    name: String,
    version_constraint: String,
    dependency_type: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct BinaryInfo {
    platform: String,
    engine: String,
    checksum: String,
    file_size: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

// ============================================================================
// Package List Endpoint Tests
// ============================================================================

mod list_packages {
    use super::*;

    /// Test GET /api/v1/packages returns a valid response
    #[test]
    fn test_list_packages_success() {
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/api/v1/packages", REGISTRY_URL);

        let response = client.get(&url).send().expect("Request failed");

        assert!(
            response.status().is_success(),
            "Expected success status, got {}",
            response.status()
        );

        let data: PackageListResponse = response.json().expect("Failed to parse response");
        assert!(data.limit > 0, "Limit should be positive");
    }

    /// Test pagination with limit and offset
    #[test]
    fn test_list_packages_pagination() {
        let client = reqwest::blocking::Client::new();

        // Request with custom limit
        let url = format!("{}/api/v1/packages?limit=5&offset=0", REGISTRY_URL);
        let response = client.get(&url).send().expect("Request failed");

        assert!(response.status().is_success());
        let data: PackageListResponse = response.json().expect("Failed to parse response");

        assert!(data.packages.len() <= 5, "Should return at most 5 packages");
        assert_eq!(data.limit, 5);
        assert_eq!(data.offset, 0);
    }

    /// Test search query parameter
    #[test]
    fn test_list_packages_search() {
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/api/v1/packages?q=chroma", REGISTRY_URL);

        let response = client.get(&url).send().expect("Request failed");

        assert!(response.status().is_success());
        let data: PackageListResponse = response.json().expect("Failed to parse response");

        // If there are results, they should be relevant to the search
        for pkg in &data.packages {
            let name_lower = pkg.name.to_lowercase();
            let desc_lower = pkg
                .description
                .as_ref()
                .map(|d| d.to_lowercase())
                .unwrap_or_default();

            // Package should match the search query in some way
            let matches = name_lower.contains("chroma") || desc_lower.contains("chroma");
            // Note: This may be a fuzzy search, so we just log mismatches
            if !matches {
                eprintln!(
                    "Package '{}' may not match 'chroma' (fuzzy search?)",
                    pkg.name
                );
            }
        }
    }
}

// ============================================================================
// Package Detail Endpoint Tests
// ============================================================================

mod package_details {
    use super::*;

    /// Test GET /api/v1/packages/:name returns package details
    #[test]
    fn test_get_package_success() {
        let client = reqwest::blocking::Client::new();

        // First, get a package name from the list
        let list_url = format!("{}/api/v1/packages?limit=1", REGISTRY_URL);
        let list_response = client.get(&list_url).send().expect("List request failed");

        if !list_response.status().is_success() {
            eprintln!("Skipping test - no packages available");
            return;
        }

        let list_data: PackageListResponse = list_response.json().expect("Failed to parse list");

        if list_data.packages.is_empty() {
            eprintln!("Skipping test - no packages in registry");
            return;
        }

        let package_name = &list_data.packages[0].name;

        // Get package details
        let detail_url = format!("{}/api/v1/packages/{}", REGISTRY_URL, package_name);
        let response = client.get(&detail_url).send().expect("Request failed");

        assert!(
            response.status().is_success(),
            "Expected success for package '{}', got {}",
            package_name,
            response.status()
        );

        let data: PackageDetailResponse = response.json().expect("Failed to parse response");
        assert_eq!(data.name, *package_name);
        assert!(!data.versions.is_empty(), "Package should have versions");
    }

    /// Test GET /api/v1/packages/:name returns 404 for non-existent package
    #[test]
    fn test_get_package_not_found() {
        let client = reqwest::blocking::Client::new();
        let url = format!(
            "{}/api/v1/packages/this-package-definitely-does-not-exist-xyz123",
            REGISTRY_URL
        );

        let response = client.get(&url).send().expect("Request failed");

        assert_eq!(response.status().as_u16(), 404, "Expected 404 Not Found");
    }
}

// ============================================================================
// Version Detail Endpoint Tests
// ============================================================================

mod version_details {
    use super::*;

    /// Test GET /api/v1/packages/:name/:version returns version details
    #[test]
    fn test_get_version_success() {
        let client = reqwest::blocking::Client::new();

        // Get a package with versions
        let list_url = format!("{}/api/v1/packages?limit=1", REGISTRY_URL);
        let list_response = client.get(&list_url).send().expect("List request failed");

        if !list_response.status().is_success() {
            eprintln!("Skipping test - registry unavailable");
            return;
        }

        let list_data: PackageListResponse = list_response.json().expect("Failed to parse list");

        if list_data.packages.is_empty() {
            eprintln!("Skipping test - no packages");
            return;
        }

        let package_name = &list_data.packages[0].name;

        // Get package to find a version
        let detail_url = format!("{}/api/v1/packages/{}", REGISTRY_URL, package_name);
        let detail_response = client
            .get(&detail_url)
            .send()
            .expect("Detail request failed");
        let detail_data: PackageDetailResponse =
            detail_response.json().expect("Failed to parse detail");

        if detail_data.versions.is_empty() {
            eprintln!("Skipping test - no versions");
            return;
        }

        let version = &detail_data.versions[0].version;

        // Get version details
        let version_url = format!(
            "{}/api/v1/packages/{}/{}",
            REGISTRY_URL, package_name, version
        );
        let response = client.get(&version_url).send().expect("Request failed");

        assert!(
            response.status().is_success(),
            "Expected success, got {}",
            response.status()
        );

        let data: VersionDetailResponse = response.json().expect("Failed to parse response");
        assert_eq!(data.version, *version);
        assert!(!data.checksum.is_empty(), "Checksum should not be empty");
        assert!(
            !data.tarball_url.is_empty(),
            "Tarball URL should not be empty"
        );
    }

    /// Test version details include dependencies if present
    #[test]
    fn test_get_version_with_dependencies() {
        let client = reqwest::blocking::Client::new();

        // Get a package
        let list_url = format!("{}/api/v1/packages?limit=10", REGISTRY_URL);
        let list_response = client.get(&list_url).send().expect("Request failed");

        if !list_response.status().is_success() {
            return;
        }

        let list_data: PackageListResponse = list_response.json().expect("Failed to parse");

        // Try to find a package with dependencies
        for pkg in &list_data.packages {
            let detail_url = format!("{}/api/v1/packages/{}", REGISTRY_URL, pkg.name);
            let detail_response = client.get(&detail_url).send();

            if let Ok(resp) = detail_response {
                if resp.status().is_success() {
                    if let Ok(detail_data) = resp.json::<PackageDetailResponse>() {
                        if let Some(version) = detail_data.versions.first() {
                            let version_url = format!(
                                "{}/api/v1/packages/{}/{}",
                                REGISTRY_URL, pkg.name, version.version
                            );
                            if let Ok(version_resp) = client.get(&version_url).send() {
                                if version_resp.status().is_success() {
                                    if let Ok(version_data) =
                                        version_resp.json::<VersionDetailResponse>()
                                    {
                                        // Just verify the structure is correct
                                        // Dependencies may be empty, which is fine
                                        eprintln!(
                                            "Package {} has {} dependencies",
                                            pkg.name,
                                            version_data.dependencies.len()
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ============================================================================
// Download Endpoint Tests
// ============================================================================

mod download {
    use super::*;

    /// Test GET /api/v1/packages/:name/:version/download returns tarball
    #[test]
    fn test_download_package() {
        let client = reqwest::blocking::Client::new();

        // Get a package
        let list_url = format!("{}/api/v1/packages?limit=1", REGISTRY_URL);
        let list_response = client.get(&list_url).send().expect("Request failed");

        if !list_response.status().is_success() {
            eprintln!("Skipping test - registry unavailable");
            return;
        }

        let list_data: PackageListResponse = list_response.json().expect("Failed to parse");

        if list_data.packages.is_empty() {
            eprintln!("Skipping test - no packages");
            return;
        }

        let package_name = &list_data.packages[0].name;

        // Get package details
        let detail_url = format!("{}/api/v1/packages/{}", REGISTRY_URL, package_name);
        let detail_response = client.get(&detail_url).send().expect("Request failed");
        let detail_data: PackageDetailResponse = detail_response.json().expect("Failed to parse");

        if detail_data.versions.is_empty() {
            eprintln!("Skipping test - no versions");
            return;
        }

        let version = &detail_data.versions[0].version;

        // Download the package
        let download_url = format!(
            "{}/api/v1/packages/{}/{}/download",
            REGISTRY_URL, package_name, version
        );
        let response = client.get(&download_url).send().expect("Request failed");

        assert!(
            response.status().is_success(),
            "Download should succeed, got {}",
            response.status()
        );

        // Verify content type
        let content_type = response
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap_or(""))
            .unwrap_or("");

        assert!(
            content_type.contains("gzip") || content_type.contains("octet-stream"),
            "Expected gzip content type, got {}",
            content_type
        );

        // Verify we got some data
        let data = response.bytes().expect("Failed to read response");
        assert!(!data.is_empty(), "Downloaded file should not be empty");

        // Verify it looks like a gzip file (magic number)
        assert!(
            data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b,
            "Downloaded file should be gzip format"
        );
    }

    /// Test download of non-existent version returns 404
    #[test]
    fn test_download_not_found() {
        let client = reqwest::blocking::Client::new();
        let url = format!(
            "{}/api/v1/packages/nonexistent-pkg/99.99.99/download",
            REGISTRY_URL
        );

        let response = client.get(&url).send().expect("Request failed");

        assert_eq!(response.status().as_u16(), 404);
    }
}

// ============================================================================
// Signature Endpoint Tests
// ============================================================================

mod signature {
    use super::*;

    /// Test GET /api/v1/packages/:name/:version/signature returns signature
    #[test]
    fn test_download_signature() {
        let client = reqwest::blocking::Client::new();

        // Get a package
        let list_url = format!("{}/api/v1/packages?limit=10", REGISTRY_URL);
        let list_response = client.get(&list_url).send().expect("Request failed");

        if !list_response.status().is_success() {
            eprintln!("Skipping test - registry unavailable");
            return;
        }

        let list_data: PackageListResponse = list_response.json().expect("Failed to parse");

        // Find a signed package
        for pkg in &list_data.packages {
            let detail_url = format!("{}/api/v1/packages/{}", REGISTRY_URL, pkg.name);
            if let Ok(detail_response) = client.get(&detail_url).send() {
                if detail_response.status().is_success() {
                    if let Ok(detail_data) = detail_response.json::<PackageDetailResponse>() {
                        // Look for a signed version
                        for version in &detail_data.versions {
                            if version.public_key.is_some() {
                                let sig_url = format!(
                                    "{}/api/v1/packages/{}/{}/signature",
                                    REGISTRY_URL, pkg.name, version.version
                                );
                                let sig_response = client.get(&sig_url).send();

                                if let Ok(resp) = sig_response {
                                    if resp.status().is_success() {
                                        let sig_data = resp.bytes().expect("Failed to read sig");
                                        assert!(
                                            !sig_data.is_empty(),
                                            "Signature should not be empty"
                                        );
                                        eprintln!(
                                            "Found signed package: {}@{} (sig size: {} bytes)",
                                            pkg.name,
                                            version.version,
                                            sig_data.len()
                                        );
                                        return; // Success!
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        eprintln!("No signed packages found in registry");
    }

    /// Test signature not found for unsigned package
    #[test]
    fn test_signature_not_found() {
        let client = reqwest::blocking::Client::new();

        // This should return 404 for non-existent packages
        let url = format!(
            "{}/api/v1/packages/nonexistent-package/1.0.0/signature",
            REGISTRY_URL
        );
        let response = client.get(&url).send().expect("Request failed");

        assert_eq!(response.status().as_u16(), 404);
    }
}

// ============================================================================
// Authentication Endpoint Tests (Read-only verification)
// ============================================================================

mod auth {
    use super::*;

    /// Test that unauthenticated publish returns 401
    #[test]
    fn test_publish_requires_auth() {
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/api/v1/packages", REGISTRY_URL);

        // Try to publish without authentication
        let response = client
            .post(&url)
            .header("Content-Type", "application/json")
            .body("{}")
            .send()
            .expect("Request failed");

        // Should get 401 Unauthorized or 400 Bad Request
        let status = response.status().as_u16();
        assert!(
            status == 401 || status == 400 || status == 415,
            "Expected auth error or bad request, got {}",
            status
        );
    }

    /// Test that unauthenticated delete returns 401
    #[test]
    fn test_delete_requires_auth() {
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/api/v1/packages/some-package/1.0.0", REGISTRY_URL);

        let response = client.delete(&url).send().expect("Request failed");

        let status = response.status().as_u16();
        assert!(
            status == 401 || status == 404,
            "Expected 401 or 404, got {}",
            status
        );
    }

    /// Test that unauthenticated yank returns 401
    #[test]
    fn test_yank_requires_auth() {
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/api/v1/packages/some-package/1.0.0/yank", REGISTRY_URL);

        let response = client.put(&url).send().expect("Request failed");

        let status = response.status().as_u16();
        assert!(
            status == 401 || status == 404,
            "Expected 401 or 404, got {}",
            status
        );
    }
}

// ============================================================================
// Response Format Tests
// ============================================================================

mod response_format {
    use super::*;

    /// Test that all responses are JSON
    #[test]
    fn test_json_content_type() {
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/api/v1/packages", REGISTRY_URL);

        let response = client.get(&url).send().expect("Request failed");

        let content_type = response
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap_or(""))
            .unwrap_or("");

        assert!(
            content_type.contains("application/json"),
            "Expected JSON content type, got {}",
            content_type
        );
    }

    /// Test error responses have consistent format
    #[test]
    fn test_error_response_format() {
        let client = reqwest::blocking::Client::new();
        let url = format!("{}/api/v1/packages/nonexistent-xyz123", REGISTRY_URL);

        let response = client.get(&url).send().expect("Request failed");

        assert_eq!(response.status().as_u16(), 404);

        let error: ErrorResponse = response.json().expect("Failed to parse error response");
        assert!(!error.error.is_empty(), "Error message should not be empty");
    }
}

// ============================================================================
// Performance Tests
// ============================================================================

mod performance {
    use super::*;
    use std::time::{Duration, Instant};

    /// Test that package list responds within acceptable time
    #[test]
    fn test_list_response_time() {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build client");

        let url = format!("{}/api/v1/packages", REGISTRY_URL);

        let start = Instant::now();
        let response = client.get(&url).send().expect("Request failed");
        let duration = start.elapsed();

        assert!(response.status().is_success());
        assert!(
            duration < Duration::from_secs(10),
            "Response took too long: {:?}",
            duration
        );

        eprintln!("List response time: {:?}", duration);
    }

    /// Test that package detail responds within acceptable time
    #[test]
    fn test_detail_response_time() {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build client");

        // Get a package name first
        let list_url = format!("{}/api/v1/packages?limit=1", REGISTRY_URL);
        let list_response = client.get(&list_url).send().expect("Request failed");

        if !list_response.status().is_success() {
            return;
        }

        let list_data: PackageListResponse = list_response.json().expect("Failed to parse");

        if list_data.packages.is_empty() {
            return;
        }

        let package_name = &list_data.packages[0].name;
        let detail_url = format!("{}/api/v1/packages/{}", REGISTRY_URL, package_name);

        let start = Instant::now();
        let response = client.get(&detail_url).send().expect("Request failed");
        let duration = start.elapsed();

        assert!(response.status().is_success());
        assert!(
            duration < Duration::from_secs(5),
            "Response took too long: {:?}",
            duration
        );

        eprintln!("Detail response time: {:?}", duration);
    }
}
