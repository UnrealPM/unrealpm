use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use unrealpm::Config;

#[derive(Debug, Serialize)]
struct RegisterRequest {
    username: String,
    email: String,
    password: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RegisterResponse {
    success: bool,
    user_id: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LoginResponse {
    success: bool,
    token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

pub fn run() -> Result<()> {
    println!("Register for UnrealPM Registry");
    println!();

    // Load config to get registry URL
    let mut config = Config::load().context("Failed to load config")?;

    let registry_url = if config.registry.registry_type == "http" {
        config.registry.url.clone()
    } else {
        println!("ERROR: You are using a file-based registry.");
        println!("Registration is only supported for HTTP registries.");
        println!();
        println!("To switch to HTTP registry, run:");
        println!("  unrealpm config set registry.registry_type http");
        println!("  unrealpm config set registry.url http://localhost:3000");
        anyhow::bail!("File-based registry does not support authentication");
    };

    // Prompt for username
    print!("Username: ");
    io::stdout().flush()?;
    let mut username = String::new();
    io::stdin().read_line(&mut username)?;
    let username = username.trim().to_string();

    if username.is_empty() {
        anyhow::bail!("Username cannot be empty");
    }

    // Validate username (alphanumeric, dash, underscore only)
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("Username can only contain letters, numbers, dashes, and underscores");
    }

    // Prompt for email
    print!("Email: ");
    io::stdout().flush()?;
    let mut email = String::new();
    io::stdin().read_line(&mut email)?;
    let email = email.trim().to_string();

    if email.is_empty() {
        anyhow::bail!("Email cannot be empty");
    }

    // Basic email validation
    if !email.contains('@') || !email.contains('.') {
        anyhow::bail!("Invalid email address");
    }

    // Prompt for password (securely)
    let password = rpassword::prompt_password("Password: ").context("Failed to read password")?;

    if password.is_empty() {
        anyhow::bail!("Password cannot be empty");
    }

    if password.len() < 8 {
        anyhow::bail!("Password must be at least 8 characters");
    }

    // Confirm password
    let password_confirm = rpassword::prompt_password("Confirm password: ")
        .context("Failed to read password confirmation")?;

    if password != password_confirm {
        anyhow::bail!("Passwords do not match");
    }

    println!();
    println!("Creating account...");

    // Send registration request
    let client = reqwest::blocking::Client::new();
    let register_url = format!("{}/api/v1/auth/register", registry_url);

    let request_body = RegisterRequest {
        username,
        email: email.clone(),
        password: password.clone(),
    };

    let response = client
        .post(&register_url)
        .json(&request_body)
        .send()
        .context("Failed to send registration request")?;

    let status = response.status();

    if status.is_success() {
        let register_response: RegisterResponse = response
            .json()
            .context("Failed to parse registration response")?;

        println!("✓ Registration successful!");
        println!();
        println!("  User ID: {}", register_response.user_id);
        println!();

        // Auto-login after successful registration
        println!("Logging you in...");

        let login_url = format!("{}/api/v1/auth/login", registry_url);
        let login_body = serde_json::json!({
            "email": email,
            "password": password,
        });

        let login_response = client
            .post(&login_url)
            .json(&login_body)
            .send()
            .context("Failed to login after registration")?;

        if login_response.status().is_success() {
            let login_data: LoginResponse = login_response
                .json()
                .context("Failed to parse login response")?;

            // Save token to config
            config.auth.token = Some(login_data.token);
            config
                .save()
                .context("Failed to save authentication token to config")?;

            println!("✓ Logged in successfully!");
            println!();
            println!("Your authentication token has been saved to ~/.unrealpm/config.toml");
            println!(
                "Token expires in {} seconds (~{} hours)",
                login_data.expires_in,
                login_data.expires_in / 3600
            );
        } else {
            println!("⚠ Registration successful, but auto-login failed.");
            println!("  Please run: unrealpm login");
        }

        println!();
        println!("You can now publish packages with: unrealpm publish");
        println!();

        if register_response.message.contains("verify") {
            println!("Note: {}", register_response.message);
            println!("You may need to verify your email before publishing.");
        }
    } else {
        // Try to parse error response
        let error_msg = if let Ok(error_response) = response.json::<ErrorResponse>() {
            error_response.error
        } else {
            format!(
                "HTTP {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown error")
            )
        };

        println!("✗ Registration failed: {}", error_msg);
        println!();

        if status.as_u16() == 409 {
            println!("This username or email is already taken.");
            println!("Please try a different username or email.");
        } else if status.as_u16() == 400 {
            println!("Invalid input. Please check your details and try again.");
        } else if status.as_u16() == 404 {
            println!("Registry endpoint not found. Is the registry server running?");
            println!("Registry URL: {}", registry_url);
        }

        anyhow::bail!("Registration failed");
    }

    Ok(())
}
