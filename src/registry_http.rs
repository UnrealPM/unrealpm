use crate::{Error, PackageMetadata, PackageType, PackageVersion, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct HttpRegistryClient {
    base_url: String,
    client: reqwest::blocking::Client,
    cache_dir: PathBuf,
    api_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublishMetadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub checksum: String,
    pub package_type: String,
    pub engine_versions: Option<Vec<String>>,
    pub dependencies: Option<Vec<DependencySpec>>,
    pub public_key: Option<String>,
    pub signed_at: Option<String>,
    pub engine_major: Option<i32>,
    pub engine_minor: Option<i32>,
    pub engine_patch: Option<i32>,
    pub is_multi_engine: Option<bool>,
    pub git_repository: Option<String>,
    pub git_tag: Option<String>,
    pub readme: Option<String>,
    pub readme_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DependencySpec {
    pub name: String,
    pub version: String,
}

impl HttpRegistryClient {
    pub fn new(base_url: String, cache_dir: PathBuf, api_token: Option<String>) -> Result<Self> {
        // Ensure cache directory exists
        std::fs::create_dir_all(&cache_dir)?;
        std::fs::create_dir_all(cache_dir.join("tarballs"))?;
        std::fs::create_dir_all(cache_dir.join("signatures"))?;

        Ok(Self {
            base_url,
            client: reqwest::blocking::Client::new(),
            cache_dir,
            api_token,
        })
    }

    /// Format authorization header based on token type
    /// API tokens (starting with "urpm_") use "Token <token>" format
    /// JWT tokens use "Bearer <token>" format
    fn format_auth_header(token: &str) -> String {
        if token.starts_with("urpm_") {
            format!("Token {}", token)
        } else {
            format!("Bearer {}", token)
        }
    }

    /// Get package metadata from HTTP registry
    pub fn get_package(&self, name: &str) -> Result<PackageMetadata> {
        let url = format!("{}/api/v1/packages/{}", self.base_url, name);

        let response = self.client.get(&url).send().map_err(|e| {
            if e.is_connect() {
                Error::Other(format!(
                    "Cannot connect to registry at {}\n\
                        Please check that the registry is running and the URL is correct.",
                    self.base_url
                ))
            } else if e.is_timeout() {
                Error::Other("Registry request timed out. Please try again.".to_string())
            } else {
                Error::Other(format!("Failed to fetch package: {}", e))
            }
        })?;

        let status = response.status();

        if status == 404 {
            return Err(Error::PackageNotFound(format!(
                "Package '{}' not found in registry",
                name
            )));
        }

        if !status.is_success() {
            let error_msg = match status.as_u16() {
                500 | 502 | 503 | 504 => format!(
                    "Registry server error (HTTP {}).\n\
                    The registry is experiencing issues. Please try again later.",
                    status.as_u16()
                ),
                _ => format!("Registry error: HTTP {}", status.as_u16()),
            };
            return Err(Error::Other(error_msg));
        }

        // Parse response
        let api_response: ApiPackageResponse = response
            .json()
            .map_err(|e| Error::Other(format!("Failed to parse response: {}", e)))?;

        // Use data from list endpoint (already has all fields including engine info)
        let versions: Vec<PackageVersion> = api_response
            .versions
            .into_iter()
            .map(|version_info| {
                let package_type = match version_info.package_type.as_str() {
                    "binary" => PackageType::Binary,
                    "hybrid" => PackageType::Hybrid,
                    _ => PackageType::Source,
                };

                PackageVersion {
                    version: version_info.version.clone(),
                    tarball: version_info.tarball_url.clone(), // Use actual tarball URL from API
                    checksum: version_info.checksum.clone(),
                    engine_versions: version_info.engine_versions.clone(),
                    engine_major: version_info.engine_major,
                    engine_minor: version_info.engine_minor,
                    is_multi_engine: version_info.is_multi_engine,
                    package_type,
                    binaries: None,
                    dependencies: None, // Dependencies fetched separately if needed
                    public_key: version_info.public_key.clone(),
                    signed_at: version_info.signed_at.clone(),
                }
            })
            .collect();

        Ok(PackageMetadata {
            name: api_response.name,
            description: api_response.description,
            versions,
        })
    }

    /// Get dependencies for a specific version from HTTP registry
    /// This fetches the detailed version info which includes dependencies
    pub fn get_version_dependencies(
        &self,
        name: &str,
        version: &str,
    ) -> Result<Option<Vec<crate::Dependency>>> {
        let url = format!("{}/api/v1/packages/{}/{}", self.base_url, name, version);

        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| Error::Other(format!("Failed to fetch version details: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Other(format!(
                "Failed to fetch version details: HTTP {}",
                response.status()
            )));
        }

        let detail: ApiVersionDetail = response
            .json()
            .map_err(|e| Error::Other(format!("Failed to parse version details: {}", e)))?;

        Ok(detail.dependencies.map(|deps| {
            deps.into_iter()
                .map(|d| crate::Dependency {
                    name: d.name,
                    version: d.version_constraint,
                })
                .collect()
        }))
    }

    /// Get tarball path (downloads if not cached)
    pub fn get_tarball_path(&self, name: &str, version: &str) -> PathBuf {
        self.cache_dir
            .join("tarballs")
            .join(format!("{}-{}.tar.gz", name, version))
    }

    /// Download package tarball with cache-first strategy
    pub fn download_if_needed(
        &self,
        name: &str,
        version: &str,
        expected_checksum: &str,
    ) -> Result<PathBuf> {
        let cached_path = self.get_tarball_path(name, version);

        // Check if already cached and verify checksum
        if cached_path.exists() {
            match calculate_checksum(&cached_path) {
                Ok(cached_checksum) if cached_checksum == expected_checksum => {
                    println!("  ✓ Using cached tarball");
                    return Ok(cached_path);
                }
                _ => {
                    println!("  ⚠ Cache checksum mismatch, re-downloading...");
                }
            }
        }

        // Download from HTTP registry
        let url = format!(
            "{}/api/v1/packages/{}/{}/download",
            self.base_url, name, version
        );

        println!("  Downloading from HTTP registry...");

        let response = self
            .client
            .get(&url)
            .send()
            .map_err(|e| Error::Other(format!("Failed to download: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Other(format!(
                "Download failed: HTTP {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .map_err(|e| Error::Other(format!("Failed to read response: {}", e)))?;

        // Save to cache
        std::fs::write(&cached_path, &bytes)?;

        println!("  ✓ Downloaded and cached");

        Ok(cached_path)
    }

    pub fn get_signature_path(&self, name: &str, version: &str) -> PathBuf {
        self.cache_dir
            .join("signatures")
            .join(format!("{}-{}.sig", name, version))
    }

    /// Download signature from HTTP registry to cache
    pub fn download_signature(&self, name: &str, version: &str) -> Result<PathBuf> {
        let url = format!(
            "{}/api/v1/packages/{}/{}/signature",
            self.base_url, name, version
        );
        let sig_path = self.get_signature_path(name, version);

        // Check if already cached
        if sig_path.exists() {
            return Ok(sig_path);
        }

        // Download from registry
        let response = self.client.get(&url).send().map_err(|e| {
            if e.is_connect() {
                Error::Other(format!(
                    "Cannot connect to registry at {}\n\
                        Please check that the registry is running and the URL is correct.",
                    self.base_url
                ))
            } else {
                Error::Other(format!("Failed to download signature: {}", e))
            }
        })?;

        let status = response.status();

        if status == 404 {
            return Err(Error::Other("Signature not found on server".to_string()));
        }

        if !status.is_success() {
            return Err(Error::Other(format!(
                "Failed to download signature: HTTP {}",
                status.as_u16()
            )));
        }

        // Save to cache
        let sig_data = response
            .bytes()
            .map_err(|e| Error::Other(format!("Failed to read signature data: {}", e)))?;

        std::fs::write(&sig_path, sig_data)?;

        Ok(sig_path)
    }

    /// Publish package to HTTP registry
    pub fn publish(
        &self,
        tarball_path: &Path,
        signature_path: Option<&Path>,
        metadata: PublishMetadata,
    ) -> Result<()> {
        let url = format!("{}/api/v1/packages", self.base_url);

        // Build multipart form
        let tarball_bytes = std::fs::read(tarball_path)?;
        let metadata_json = serde_json::to_string(&metadata)?;

        let form = reqwest::blocking::multipart::Form::new()
            .part(
                "tarball",
                reqwest::blocking::multipart::Part::bytes(tarball_bytes).file_name(
                    tarball_path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                ),
            )
            .text("metadata", metadata_json);

        // Add signature if provided
        let form = if let Some(sig_path) = signature_path {
            let sig_bytes = std::fs::read(sig_path)?;
            form.part(
                "signature",
                reqwest::blocking::multipart::Part::bytes(sig_bytes)
                    .file_name(sig_path.file_name().unwrap().to_string_lossy().to_string()),
            )
        } else {
            form
        };

        // Send request with API token if available
        let mut request = self.client.post(&url).multipart(form);

        if let Some(token) = &self.api_token {
            request = request.header("Authorization", Self::format_auth_header(token));
        }

        let response = request.send()
            .map_err(|e| {
                // Check if it's a connection error
                if e.is_connect() {
                    Error::Other(format!("Cannot connect to registry. Is the registry server running?\nError: {}", e))
                } else if e.is_body() {
                    // Body error during multipart - likely auth rejection
                    Error::Other("Authentication required.\n\nYou need to login before publishing.\nRun: unrealpm login".to_string())
                } else if e.is_request() {
                    // Request error - could be various things
                    Error::Other("Authentication required.\n\nYou need to login before publishing.\nRun: unrealpm login".to_string())
                } else {
                    // Unknown error - show the full message
                    Error::Other(format!("Authentication required.\n\nYou need to login before publishing.\nRun: unrealpm login\n\n(Debug: {})", e))
                }
            })?;

        let status = response.status();

        if !status.is_success() {
            let error_text = response
                .text()
                .unwrap_or_else(|_| format!("HTTP {}", status.as_u16()));

            // Provide helpful error messages based on status code
            let error_msg = match status.as_u16() {
                401 => "Authentication required.\n\n\
                    You need to login before publishing.\n\
                    Run: unrealpm login"
                    .to_string(),
                403 => "Permission denied.\n\n\
                    You do not have permission to publish to this package.\n\
                    Only the package owner can publish new versions."
                    .to_string(),
                409 => "Version conflict.\n\n\
                    This version already exists in the registry.\n\
                    Bump the version in your .uplugin file and try again."
                    .to_string(),
                413 => "Package too large.\n\n\
                    The package exceeds the maximum upload size.\n\
                    Consider excluding unnecessary files from the package."
                    .to_string(),
                500 | 502 | 503 | 504 => format!(
                    "Registry server error.\n\n\
                    The registry is experiencing issues. Please try again later.\n\n\
                    Details: {}",
                    error_text
                ),
                _ => format!("Publish failed (HTTP {}):\n{}", status.as_u16(), error_text),
            };

            return Err(Error::Other(error_msg));
        }

        Ok(())
    }

    pub fn get_tarballs_dir(&self) -> PathBuf {
        self.cache_dir.join("tarballs")
    }

    pub fn get_signatures_dir(&self) -> PathBuf {
        self.cache_dir.join("signatures")
    }

    pub fn get_packages_dir(&self) -> PathBuf {
        self.cache_dir.join("packages")
    }

    /// Unpublish a package version or entire package
    pub fn unpublish(&self, name: &str, version: Option<&str>) -> Result<()> {
        let url = if let Some(v) = version {
            format!("{}/api/v1/packages/{}/{}", self.base_url, name, v)
        } else {
            format!("{}/api/v1/packages/{}", self.base_url, name)
        };

        let mut request = self.client.delete(&url);

        if let Some(token) = &self.api_token {
            request = request.header("Authorization", Self::format_auth_header(token));
        }

        let response = request
            .send()
            .map_err(|e| Error::Other(format!("Failed to unpublish: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_msg = match status.as_u16() {
                401 => "Authentication required. Run: unrealpm login".to_string(),
                403 => "Permission denied. You can only unpublish your own packages.".to_string(),
                404 => "Package or version not found.".to_string(),
                _ => format!("Unpublish failed: HTTP {}", status.as_u16()),
            };
            return Err(Error::Other(error_msg));
        }

        Ok(())
    }

    /// Yank or un-yank a package version
    pub fn yank(&self, name: &str, version: &str, unyank: bool) -> Result<()> {
        let url = format!(
            "{}/api/v1/packages/{}/{}/yank",
            self.base_url, name, version
        );

        let mut request = if unyank {
            self.client.delete(&url)
        } else {
            self.client.put(&url)
        };

        if let Some(token) = &self.api_token {
            request = request.header("Authorization", Self::format_auth_header(token));
        }

        let response = request
            .send()
            .map_err(|e| Error::Other(format!("Failed to yank/unyank: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_msg = match status.as_u16() {
                401 => "Authentication required. Run: unrealpm login".to_string(),
                403 => "Permission denied. You can only yank your own packages.".to_string(),
                404 => "Package or version not found.".to_string(),
                _ => format!("Yank failed: HTTP {}", status.as_u16()),
            };
            return Err(Error::Other(error_msg));
        }

        Ok(())
    }

    /// Search for packages by query string
    pub fn search(&self, query: &str) -> Result<Vec<String>> {
        // Don't send ?q= parameter when query is empty - registry treats empty query differently
        let url = if query.is_empty() {
            format!("{}/api/v1/packages", self.base_url)
        } else {
            format!(
                "{}/api/v1/packages?q={}",
                self.base_url,
                urlencoding::encode(query)
            )
        };

        let response = self.client.get(&url).send().map_err(|e| {
            if e.is_connect() {
                Error::Other(format!(
                    "Cannot connect to registry at {}\n\
                        Please check that the registry is running and the URL is correct.",
                    self.base_url
                ))
            } else if e.is_timeout() {
                Error::Other("Registry request timed out. Please try again.".to_string())
            } else {
                Error::Other(format!("Failed to search packages: {}", e))
            }
        })?;

        let status = response.status();

        if !status.is_success() {
            let error_msg = match status.as_u16() {
                500 | 502 | 503 | 504 => format!(
                    "Registry server error (HTTP {}).\n\
                    The registry is experiencing issues. Please try again later.",
                    status.as_u16()
                ),
                _ => format!("Search failed: HTTP {}", status.as_u16()),
            };
            return Err(Error::Other(error_msg));
        }

        // Parse response
        let api_response: ApiPackageListResponse = response
            .json()
            .map_err(|e| Error::Other(format!("Failed to parse search response: {}", e)))?;

        // Extract package names
        Ok(api_response.packages.into_iter().map(|p| p.name).collect())
    }

    /// Search for packages by query string, returning full package info
    pub fn search_packages(&self, query: &str) -> Result<Vec<ApiPackageInfo>> {
        // Don't send ?q= parameter when query is empty - registry treats empty query differently
        let url = if query.is_empty() {
            format!("{}/api/v1/packages", self.base_url)
        } else {
            format!(
                "{}/api/v1/packages?q={}",
                self.base_url,
                urlencoding::encode(query)
            )
        };

        let response = self.client.get(&url).send().map_err(|e| {
            if e.is_connect() {
                Error::Other(format!(
                    "Cannot connect to registry at {}\n\
                        Please check that the registry is running and the URL is correct.",
                    self.base_url
                ))
            } else if e.is_timeout() {
                Error::Other("Registry request timed out. Please try again.".to_string())
            } else {
                Error::Other(format!("Failed to search packages: {}", e))
            }
        })?;

        let status = response.status();

        if !status.is_success() {
            let error_msg = match status.as_u16() {
                500 | 502 | 503 | 504 => format!(
                    "Registry server error (HTTP {}).\n\
                    The registry is experiencing issues. Please try again later.",
                    status.as_u16()
                ),
                _ => format!("Search failed: HTTP {}", status.as_u16()),
            };
            return Err(Error::Other(error_msg));
        }

        // Parse response
        let api_response: ApiPackageListResponse = response
            .json()
            .map_err(|e| Error::Other(format!("Failed to parse search response: {}", e)))?;

        Ok(api_response.packages)
    }
}

// Helper to parse package type string
#[allow(dead_code)]
fn parse_package_type(s: &str) -> crate::PackageType {
    match s {
        "binary" => crate::PackageType::Binary,
        "hybrid" => crate::PackageType::Hybrid,
        _ => crate::PackageType::Source,
    }
}

// Helper to calculate file checksum
fn calculate_checksum(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path)?;
    let hash = Sha256::digest(&data);
    Ok(format!("{:x}", hash))
}

// API response structures
#[derive(Debug, Deserialize)]
struct ApiPackageListResponse {
    packages: Vec<ApiPackageInfo>,
    #[allow(dead_code)]
    total: usize,
    #[allow(dead_code)]
    limit: i64,
    #[allow(dead_code)]
    offset: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiPackageInfo {
    pub name: String,
    pub description: Option<String>,
    pub latest_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiPackageResponse {
    name: String,
    description: Option<String>,
    versions: Vec<ApiVersionInfo>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiVersionInfo {
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
#[allow(dead_code)] // Fields used for deserialization, will be used in future features
struct ApiVersionDetail {
    version: String,
    checksum: String,
    package_type: String,
    engine_versions: Option<Vec<String>>,
    engine_major: Option<i32>,
    engine_minor: Option<i32>,
    is_multi_engine: Option<bool>,
    public_key: Option<String>,
    signed_at: Option<String>,
    dependencies: Option<Vec<ApiDependency>>,
    tarball_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiDependency {
    name: String,
    version_constraint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ============================================================================
    // format_auth_header tests
    // ============================================================================

    #[test]
    fn test_format_auth_header_api_token() {
        // API tokens starting with "urpm_" should use "Token" format
        let token = "urpm_abc123xyz";
        let header = HttpRegistryClient::format_auth_header(token);
        assert_eq!(header, "Token urpm_abc123xyz");
    }

    #[test]
    fn test_format_auth_header_jwt() {
        // JWT tokens (not starting with "urpm_") should use "Bearer" format
        let token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc";
        let header = HttpRegistryClient::format_auth_header(token);
        assert!(header.starts_with("Bearer "));
        assert!(header.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
    }

    #[test]
    fn test_format_auth_header_short_token() {
        // Short tokens without "urpm_" prefix should use Bearer
        let token = "short_token";
        let header = HttpRegistryClient::format_auth_header(token);
        assert_eq!(header, "Bearer short_token");
    }

    #[test]
    fn test_format_auth_header_empty() {
        // Empty token should use Bearer format
        let token = "";
        let header = HttpRegistryClient::format_auth_header(token);
        assert_eq!(header, "Bearer ");
    }

    // ============================================================================
    // HttpRegistryClient::new tests
    // ============================================================================

    #[test]
    fn test_client_new_creates_directories() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let client =
            HttpRegistryClient::new("http://localhost:3000".to_string(), cache_dir.clone(), None);

        assert!(client.is_ok());
        assert!(cache_dir.exists());
        assert!(cache_dir.join("tarballs").exists());
        assert!(cache_dir.join("signatures").exists());
    }

    #[test]
    fn test_client_new_with_token() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let client = HttpRegistryClient::new(
            "http://localhost:3000".to_string(),
            cache_dir,
            Some("urpm_test_token".to_string()),
        );

        assert!(client.is_ok());
    }

    #[test]
    fn test_client_new_existing_cache() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        // Pre-create directories
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::create_dir_all(cache_dir.join("tarballs")).unwrap();

        let client =
            HttpRegistryClient::new("http://localhost:3000".to_string(), cache_dir.clone(), None);

        assert!(client.is_ok());
        // Should also create signatures dir
        assert!(cache_dir.join("signatures").exists());
    }

    // ============================================================================
    // Path generation tests
    // ============================================================================

    #[test]
    fn test_get_tarball_path() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let client =
            HttpRegistryClient::new("http://localhost:3000".to_string(), cache_dir.clone(), None)
                .unwrap();

        let path = client.get_tarball_path("my-plugin", "1.2.3");
        assert_eq!(
            path,
            cache_dir.join("tarballs").join("my-plugin-1.2.3.tar.gz")
        );
    }

    #[test]
    fn test_get_signature_path() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let client =
            HttpRegistryClient::new("http://localhost:3000".to_string(), cache_dir.clone(), None)
                .unwrap();

        let path = client.get_signature_path("my-plugin", "1.2.3");
        assert_eq!(
            path,
            cache_dir.join("signatures").join("my-plugin-1.2.3.sig")
        );
    }

    #[test]
    fn test_get_tarballs_dir() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let client =
            HttpRegistryClient::new("http://localhost:3000".to_string(), cache_dir.clone(), None)
                .unwrap();

        assert_eq!(client.get_tarballs_dir(), cache_dir.join("tarballs"));
    }

    #[test]
    fn test_get_signatures_dir() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let client =
            HttpRegistryClient::new("http://localhost:3000".to_string(), cache_dir.clone(), None)
                .unwrap();

        assert_eq!(client.get_signatures_dir(), cache_dir.join("signatures"));
    }

    #[test]
    fn test_get_packages_dir() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = temp_dir.path().join("cache");

        let client =
            HttpRegistryClient::new("http://localhost:3000".to_string(), cache_dir.clone(), None)
                .unwrap();

        assert_eq!(client.get_packages_dir(), cache_dir.join("packages"));
    }

    // ============================================================================
    // calculate_checksum tests
    // ============================================================================

    #[test]
    fn test_calculate_checksum() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.txt");

        std::fs::write(&test_file, "Hello, World!").unwrap();

        let checksum = calculate_checksum(&test_file);
        assert!(checksum.is_ok());

        // SHA256 of "Hello, World!" is known
        let expected = "dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f";
        assert_eq!(checksum.unwrap(), expected);
    }

    #[test]
    fn test_calculate_checksum_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("empty.txt");

        std::fs::write(&test_file, "").unwrap();

        let checksum = calculate_checksum(&test_file);
        assert!(checksum.is_ok());

        // SHA256 of empty string
        let expected = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(checksum.unwrap(), expected);
    }

    #[test]
    fn test_calculate_checksum_file_not_found() {
        let result = calculate_checksum(Path::new("/nonexistent/file.txt"));
        assert!(result.is_err());
    }

    // ============================================================================
    // parse_package_type tests
    // ============================================================================

    #[test]
    fn test_parse_package_type_source() {
        assert_eq!(parse_package_type("source"), crate::PackageType::Source);
    }

    #[test]
    fn test_parse_package_type_binary() {
        assert_eq!(parse_package_type("binary"), crate::PackageType::Binary);
    }

    #[test]
    fn test_parse_package_type_hybrid() {
        assert_eq!(parse_package_type("hybrid"), crate::PackageType::Hybrid);
    }

    #[test]
    fn test_parse_package_type_unknown() {
        // Unknown types default to Source
        assert_eq!(parse_package_type("unknown"), crate::PackageType::Source);
        assert_eq!(parse_package_type(""), crate::PackageType::Source);
    }

    // ============================================================================
    // PublishMetadata tests
    // ============================================================================

    #[test]
    fn test_publish_metadata_serialization() {
        let metadata = PublishMetadata {
            name: "test-plugin".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test plugin".to_string()),
            checksum: "abc123".to_string(),
            package_type: "source".to_string(),
            engine_versions: Some(vec!["5.3".to_string(), "5.4".to_string()]),
            dependencies: Some(vec![DependencySpec {
                name: "dep".to_string(),
                version: "^1.0.0".to_string(),
            }]),
            public_key: None,
            signed_at: None,
            engine_major: Some(5),
            engine_minor: Some(3),
            engine_patch: None,
            is_multi_engine: Some(true),
            git_repository: None,
            git_tag: None,
            readme: None,
            readme_type: None,
        };

        let json = serde_json::to_string(&metadata);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("test-plugin"));
        assert!(json_str.contains("1.0.0"));
        assert!(json_str.contains("5.3"));
    }

    #[test]
    fn test_dependency_spec_serialization() {
        let dep = DependencySpec {
            name: "my-dep".to_string(),
            version: "^2.0.0".to_string(),
        };

        let json = serde_json::to_string(&dep);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("my-dep"));
        assert!(json_str.contains("^2.0.0"));
    }
}
