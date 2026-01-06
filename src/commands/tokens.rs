use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use unrealpm::{config::AuthConfig, Config};

#[derive(Debug, Serialize)]
struct CreateTokenRequest {
    name: String,
    scopes: Vec<String>,
    expires_in_days: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct CreateTokenResponse {
    success: bool,
    token: String,
    token_id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TokenInfo {
    id: String,
    name: Option<String>,
    scopes: Vec<String>,
    created_at: String,
    last_used_at: Option<String>,
    expires_at: Option<String>,
    revoked: bool,
}

#[derive(Debug, Deserialize)]
struct TokenListResponse {
    tokens: Vec<TokenInfo>,
}

pub fn run_create(name: String, scopes: Vec<String>, expires_days: Option<i64>) -> Result<()> {
    println!("Creating API token...");
    println!();

    // Load config
    let config = Config::load()?;

    if config.registry.registry_type != "http" {
        anyhow::bail!("API tokens are only supported for HTTP registries");
    }

    // Check we're logged in
    let auth_token = config
        .auth
        .token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run: unrealpm login"))?;

    // Default scopes if none provided
    let scopes = if scopes.is_empty() {
        vec!["read".to_string(), "publish".to_string()]
    } else {
        scopes
    };

    let request_body = CreateTokenRequest {
        name,
        scopes: scopes.clone(),
        expires_in_days: expires_days,
    };

    // Send request
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/api/v1/auth/tokens", config.registry.url);

    let response = client
        .post(&url)
        .header("Authorization", AuthConfig::format_auth_header(auth_token))
        .json(&request_body)
        .send()
        .context("Failed to create token")?;

    if !response.status().is_success() {
        let error_text = response
            .text()
            .unwrap_or_else(|_| "Unknown error".to_string());
        anyhow::bail!("Failed to create token: {}", error_text);
    }

    let token_response: CreateTokenResponse =
        response.json().context("Failed to parse response")?;

    println!("✓ Token created successfully!");
    println!();
    println!("  Token ID: {}", token_response.token_id);
    println!("  Scopes: {}", scopes.join(", "));
    if let Some(days) = expires_days {
        println!("  Expires in: {} days", days);
    } else {
        println!("  Expires: Never (permanent)");
    }
    println!();
    println!("⚠ IMPORTANT: Save this token securely - you won't be able to see it again!");
    println!();
    println!("  {}", token_response.token);
    println!();
    println!("To use this token:");
    println!(
        "  unrealpm config set auth.token \"{}\"",
        token_response.token
    );
    println!();
    println!("Or set environment variable:");
    println!("  export UNREALPM_TOKEN=\"{}\"", token_response.token);

    Ok(())
}

pub fn run_list() -> Result<()> {
    println!("Your API tokens:");
    println!();

    // Load config
    let config = Config::load()?;

    if config.registry.registry_type != "http" {
        anyhow::bail!("API tokens are only supported for HTTP registries");
    }

    // Check we're logged in
    let auth_token = config
        .auth
        .token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run: unrealpm login"))?;

    // Send request
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/api/v1/auth/tokens", config.registry.url);

    let response = client
        .get(&url)
        .header("Authorization", AuthConfig::format_auth_header(auth_token))
        .send()
        .context("Failed to list tokens")?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to list tokens: HTTP {}", response.status().as_u16());
    }

    let token_list: TokenListResponse = response.json().context("Failed to parse response")?;

    if token_list.tokens.is_empty() {
        println!("No API tokens found.");
        println!();
        println!("Create one with: unrealpm tokens create <name>");
        return Ok(());
    }

    println!("┌─────────────────────────────────────────────────────────────────┐");
    for token in &token_list.tokens {
        let status = if token.revoked {
            "REVOKED"
        } else if token.expires_at.is_some() {
            "Active (expires)"
        } else {
            "Active (permanent)"
        };

        println!(
            "│ {:<30} │ {:<15} │",
            token.name.as_deref().unwrap_or("Unnamed"),
            status
        );
        println!("│   ID: {:<55} │", &token.id);
        println!("│   Scopes: {:<52} │", token.scopes.join(", "));

        if let Some(ref last_used) = token.last_used_at {
            println!("│   Last used: {:<49} │", last_used);
        }

        println!("├─────────────────────────────────────────────────────────────────┤");
    }
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
    println!("Total: {} token(s)", token_list.tokens.len());

    Ok(())
}

pub fn run_revoke(token_id: String) -> Result<()> {
    println!("Revoking token...");
    println!();

    // Load config
    let config = Config::load()?;

    if config.registry.registry_type != "http" {
        anyhow::bail!("API tokens are only supported for HTTP registries");
    }

    // Check we're logged in
    let auth_token = config
        .auth
        .token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run: unrealpm login"))?;

    // Confirm
    print!("Are you sure you want to revoke this token? (yes/no): ");
    io::stdout().flush()?;

    let mut confirmation = String::new();
    io::stdin().read_line(&mut confirmation)?;

    if confirmation.trim().to_lowercase() != "yes" {
        println!("Revoke cancelled.");
        return Ok(());
    }

    // Send request
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/api/v1/auth/tokens/{}", config.registry.url, token_id);

    let response = client
        .delete(&url)
        .header("Authorization", AuthConfig::format_auth_header(auth_token))
        .send()
        .context("Failed to revoke token")?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to revoke token: HTTP {}",
            response.status().as_u16()
        );
    }

    println!("✓ Token revoked successfully");
    println!();
    println!("This token can no longer be used for authentication.");

    Ok(())
}
