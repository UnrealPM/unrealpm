use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;
use unrealpm::Config;

#[derive(Debug, Serialize)]
struct LoginRequest {
    email: String,
    password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    totp_code: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LoginResponse {
    success: bool,
    token: Option<String>,
    expires_in: Option<u64>,
    #[serde(default)]
    requires_2fa: bool,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

pub fn run(use_github: bool, use_email: bool) -> Result<()> {
    // If explicit flag provided, use that method
    if use_github {
        return run_github_oauth();
    }
    if use_email {
        return run_email_login();
    }

    // No flag provided - ask user to choose
    println!("Login to UnrealPM Registry");
    println!();
    println!("Choose login method:");
    println!("  [1] GitHub (recommended)");
    println!("  [2] Email/Password");
    println!();
    print!("Enter choice (1 or 2): ");
    io::stdout().flush()?;

    let mut choice = String::new();
    io::stdin().read_line(&mut choice)?;
    let choice = choice.trim();

    match choice {
        "1" | "github" | "g" => run_github_oauth(),
        "2" | "email" | "e" => run_email_login(),
        _ => {
            println!();
            println!("Invalid choice. Please run 'unrealpm login' again.");
            println!();
            println!("Or use flags directly:");
            println!("  unrealpm login --github");
            println!("  unrealpm login --email");
            Ok(())
        }
    }
}

/// Login using email and password
fn run_email_login() -> Result<()> {
    println!("Login with Email/Password");
    println!();

    // Load config to get registry URL
    let mut config = Config::load().context("Failed to load config")?;

    let registry_url = if config.registry.registry_type == "http" {
        config.registry.url.clone()
    } else {
        println!("ERROR: You are using a file-based registry.");
        println!("Login is only supported for HTTP registries.");
        println!();
        println!("To switch to HTTP registry, run:");
        println!("  unrealpm config set registry.registry_type http");
        println!("  unrealpm config set registry.url https://registry.unreal.dev");
        anyhow::bail!("File-based registry does not support authentication");
    };

    // Security: Require HTTPS for email/password login (except localhost for development)
    let is_localhost = registry_url.contains("localhost") || registry_url.contains("127.0.0.1");
    if !registry_url.starts_with("https://") && !is_localhost {
        println!("ERROR: Email/password login requires HTTPS for security.");
        println!();
        println!("Your current registry URL: {}", registry_url);
        println!();
        println!("Either:");
        println!("  1. Use HTTPS: unrealpm config set registry.url https://registry.unreal.dev");
        println!("  2. Use GitHub login instead: unrealpm login --github");
        anyhow::bail!("Refusing to send credentials over unencrypted connection");
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

    // Prompt for password (securely)
    let password = rpassword::prompt_password("Password: ").context("Failed to read password")?;

    if password.is_empty() {
        anyhow::bail!("Password cannot be empty");
    }

    println!();
    println!("Authenticating...");

    // Send login request
    let client = reqwest::blocking::Client::new();
    let login_url = format!("{}/api/v1/auth/login", registry_url);

    // First attempt without TOTP code
    let request_body = LoginRequest {
        email: email.clone(),
        password: password.clone(),
        totp_code: None,
    };

    let response = client
        .post(&login_url)
        .json(&request_body)
        .send()
        .context("Failed to send login request")?;

    let status = response.status();

    if status.is_success() {
        let login_response: LoginResponse =
            response.json().context("Failed to parse login response")?;

        // Check if 2FA is required
        if login_response.requires_2fa {
            println!();
            println!("Two-factor authentication required.");
            println!();

            // Prompt for TOTP code
            print!("Enter 6-digit code from your authenticator app: ");
            io::stdout().flush()?;
            let mut totp_code = String::new();
            io::stdin().read_line(&mut totp_code)?;
            let totp_code = totp_code.trim().to_string();

            if totp_code.is_empty() {
                anyhow::bail!("2FA code cannot be empty");
            }

            // Validate code format (6 digits)
            if totp_code.len() != 6 || !totp_code.chars().all(|c| c.is_ascii_digit()) {
                anyhow::bail!("Invalid 2FA code format. Please enter a 6-digit code.");
            }

            println!();
            println!("Verifying 2FA code...");

            // Second attempt with TOTP code
            let request_body = LoginRequest {
                email,
                password,
                totp_code: Some(totp_code),
            };

            let response = client
                .post(&login_url)
                .json(&request_body)
                .send()
                .context("Failed to send login request")?;

            let status = response.status();

            if status.is_success() {
                let login_response: LoginResponse =
                    response.json().context("Failed to parse login response")?;

                if let Some(token) = login_response.token {
                    // Save token to config
                    config.auth.token = Some(token);
                    config
                        .save()
                        .context("Failed to save authentication token to config")?;

                    println!("✓ Login successful!");
                    println!();
                    println!("Your authentication token has been saved to ~/.unrealpm/config.toml");
                    if let Some(expires_in) = login_response.expires_in {
                        println!(
                            "Token expires in {} seconds (~{} hours)",
                            expires_in,
                            expires_in / 3600
                        );
                    }
                    println!();
                    println!("You can now publish packages with: unrealpm publish");
                } else {
                    anyhow::bail!("Login succeeded but no token was returned");
                }
            } else {
                // Handle error from 2FA attempt
                let error_msg = if let Ok(error_response) = response.json::<ErrorResponse>() {
                    error_response.error
                } else {
                    format!(
                        "HTTP {}: {}",
                        status.as_u16(),
                        status.canonical_reason().unwrap_or("Unknown error")
                    )
                };

                println!("✗ 2FA verification failed: {}", error_msg);
                anyhow::bail!("Two-factor authentication failed");
            }
        } else if let Some(token) = login_response.token {
            // No 2FA required, save token directly
            config.auth.token = Some(token);
            config
                .save()
                .context("Failed to save authentication token to config")?;

            println!("✓ Login successful!");
            println!();
            println!("Your authentication token has been saved to ~/.unrealpm/config.toml");
            if let Some(expires_in) = login_response.expires_in {
                println!(
                    "Token expires in {} seconds (~{} hours)",
                    expires_in,
                    expires_in / 3600
                );
            }
            println!();
            println!("You can now publish packages with: unrealpm publish");
        } else {
            anyhow::bail!("Login succeeded but no token was returned");
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

        println!("✗ Login failed: {}", error_msg);
        println!();

        if status.as_u16() == 401 {
            println!("Please check your email and password.");
            println!();
            println!(
                "Don't have an account? Register at: {}/register",
                registry_url
            );
        } else if status.as_u16() == 404 {
            println!("Registry endpoint not found. Is the registry server running?");
            println!("Registry URL: {}", registry_url);
        }

        anyhow::bail!("Authentication failed");
    }

    Ok(())
}

/// Logout - clear stored authentication token
pub fn run_logout() -> Result<()> {
    let mut config = Config::load().context("Failed to load config")?;

    if config.auth.token.is_none() {
        println!("You are not currently logged in.");
        return Ok(());
    }

    config.auth.token = None;
    config.save().context("Failed to save config")?;

    println!("✓ Logged out successfully");
    println!();
    println!("Your authentication token has been removed from ~/.unrealpm/config.toml");
    println!("To login again, run: unrealpm login");

    Ok(())
}

/// Login using GitHub OAuth (browser-based flow with automatic token delivery)
fn run_github_oauth() -> Result<()> {
    println!("Login with GitHub");
    println!();

    // Load config to get registry URL
    let mut config = Config::load().context("Failed to load config")?;

    let registry_url = if config.registry.registry_type == "http" {
        config.registry.url.clone()
    } else {
        println!("ERROR: You are using a file-based registry.");
        println!("GitHub login is only supported for HTTP registries.");
        println!();
        println!("To switch to HTTP registry, run:");
        println!("  unrealpm config set registry.registry_type http");
        println!("  unrealpm config set registry.url http://localhost:3000");
        anyhow::bail!("File-based registry does not support authentication");
    };

    // Try to start a local callback server for automatic token delivery
    let (tx, rx) = mpsc::channel::<(String, String)>();
    let callback_port = start_local_callback_server(tx)?;

    // Build authorization URL with cli=true and port for automatic callback
    let registry_url = registry_url.trim_end_matches('/');
    let auth_url = format!(
        "{}/api/v1/auth/github/authorize?cli=true&cli_port={}",
        registry_url, callback_port
    );

    println!("Starting GitHub OAuth flow...");
    println!();
    println!("Opening browser to GitHub authorization page...");
    println!();

    // Open browser
    if let Err(e) = webbrowser::open(&auth_url) {
        println!("⚠ Could not open browser automatically: {}", e);
        println!();
        println!("Please open this URL manually:");
        println!("  {}", auth_url);
        println!();
    }

    println!("Waiting for authorization...");
    println!("(Press Ctrl+C to cancel)");
    println!();

    // Wait for callback with token (timeout after 5 minutes)
    match rx.recv_timeout(Duration::from_secs(300)) {
        Ok((token, username)) => {
            // Save token to config
            config.auth.token = Some(token);
            config
                .save()
                .context("Failed to save authentication token to config")?;

            println!("✓ Login successful!");
            println!();
            println!("Welcome, {}!", username);
            println!();
            println!("Your authentication token has been saved to ~/.unrealpm/config.toml");
            println!();
            println!("You can now publish packages with: unrealpm publish");
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            anyhow::bail!("Login timed out. Please try again.");
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            anyhow::bail!("Login failed. The callback server stopped unexpectedly.");
        }
    }

    Ok(())
}

/// Start a local HTTP server to receive OAuth callback
/// Returns the port number the server is listening on
fn start_local_callback_server(tx: mpsc::Sender<(String, String)>) -> Result<u16> {
    // Try to bind to a random available port
    let listener =
        TcpListener::bind("127.0.0.1:0").context("Failed to start local callback server")?;
    let port = listener.local_addr()?.port();

    // Spawn the server in a background thread
    std::thread::spawn(move || {
        let server = tiny_http::Server::from_listener(listener, None)
            .expect("Failed to create HTTP server from listener");

        // Wait for a single request (the callback)
        if let Ok(Some(request)) = server.recv_timeout(Duration::from_secs(300)) {
            let url = request.url().to_string();

            // Parse query parameters from /callback?token=...&username=...
            if url.starts_with("/callback?") {
                let query = url.trim_start_matches("/callback?");
                let mut token = None;
                let mut username = String::from("User");

                for param in query.split('&') {
                    if let Some((key, value)) = param.split_once('=') {
                        match key {
                            "token" => {
                                token = Some(
                                    urlencoding::decode(value)
                                        .unwrap_or_else(|_| value.into())
                                        .into_owned(),
                                );
                            }
                            "username" => {
                                username = urlencoding::decode(value)
                                    .unwrap_or_else(|_| value.into())
                                    .into_owned();
                            }
                            _ => {}
                        }
                    }
                }

                if let Some(token) = token {
                    // Send token back to main thread
                    let _ = tx.send((token, username.clone()));

                    // Send success response to browser
                    let html = format!(
                        r#"<!DOCTYPE html>
<html>
<head>
    <title>Login Successful - UnrealPM</title>
    <style>
        body {{ font-family: system-ui, -apple-system, sans-serif; max-width: 500px; margin: 100px auto; text-align: center; background: #0a0a0f; color: #fff; }}
        h1 {{ color: #22c55e; }}
        p {{ color: #888; }}
    </style>
</head>
<body>
    <h1>✓ Login Successful!</h1>
    <p>Welcome, <strong>{}</strong>!</p>
    <p>You can close this window and return to your terminal.</p>
</body>
</html>"#,
                        username
                    );

                    let response = tiny_http::Response::from_string(html).with_header(
                        tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"text/html; charset=utf-8"[..],
                        )
                        .unwrap(),
                    );
                    let _ = request.respond(response);
                    return;
                }
            }

            // Invalid request - send error response
            let response =
                tiny_http::Response::from_string("Invalid callback request").with_status_code(400);
            let _ = request.respond(response);
        }
    });

    Ok(port)
}
