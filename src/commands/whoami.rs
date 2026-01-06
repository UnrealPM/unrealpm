use anyhow::{Context, Result};
use serde::Deserialize;
use unrealpm::{config::AuthConfig, Config};

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    user_id: String,
    username: String,
    email: String,
    email_verified: bool,
    created_at: String,
    github_username: Option<String>,
    has_2fa: bool,
    is_admin: bool,
}

pub fn run() -> Result<()> {
    // Load config
    let config = Config::load()?;

    if config.registry.registry_type != "http" {
        anyhow::bail!("whoami is only supported for HTTP registries");
    }

    // Check we're logged in
    let auth_token = config
        .auth
        .token
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Not logged in. Run: unrealpm login"))?;

    // Send request
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/api/v1/auth/me", config.registry.url);

    let response = client
        .get(&url)
        .header("Authorization", AuthConfig::format_auth_header(auth_token))
        .send()
        .context("Failed to get user info")?;

    if !response.status().is_success() {
        if response.status().as_u16() == 401 {
            anyhow::bail!("Session expired or invalid. Run: unrealpm login");
        }
        anyhow::bail!(
            "Failed to get user info: HTTP {}",
            response.status().as_u16()
        );
    }

    let user: UserInfoResponse = response.json().context("Failed to parse response")?;

    // Display user info
    println!("Logged in as: {}", user.username);
    println!();
    println!("  User ID:    {}", user.id_short(&user.user_id));
    println!("  Email:      {}", user.email);
    println!(
        "  Verified:   {}",
        if user.email_verified { "Yes" } else { "No" }
    );

    if let Some(ref github) = user.github_username {
        println!("  GitHub:     @{}", github);
    }

    println!(
        "  2FA:        {}",
        if user.has_2fa { "Enabled" } else { "Disabled" }
    );

    if user.is_admin {
        println!("  Role:       Admin");
    }

    println!("  Member since: {}", format_date(&user.created_at));
    println!();
    println!("Registry: {}", config.registry.url);

    Ok(())
}

impl UserInfoResponse {
    fn id_short(&self, id: &str) -> String {
        if id.len() > 8 {
            format!("{}...", &id[..8])
        } else {
            id.to_string()
        }
    }
}

fn format_date(rfc3339: &str) -> String {
    // Try to parse and format nicely, otherwise return as-is
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc3339) {
        dt.format("%B %d, %Y").to_string()
    } else {
        rfc3339.to_string()
    }
}
