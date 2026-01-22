//! Implementation of the `rapina dev` command.

use colored::Colorize;
use notify_debouncer_mini::{DebounceEventResult, new_debouncer, notify::RecursiveMode};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// Configuration for the dev server.
pub struct DevConfig {
    pub host: String,
    pub port: u16,
    pub reload: bool,
}

impl Default for DevConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            reload: true,
        }
    }
}

/// Execute the `dev` command to start the development server.
pub fn execute(config: DevConfig) -> Result<(), String> {
    // Check if we're in a Rapina project
    verify_rapina_project()?;

    // Print banner
    print_banner(&config);

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .map_err(|e| format!("Failed to set Ctrl+C handler: {}", e))?;

    // Initial build and run
    println!(
        "{} Building project...",
        "INFO".custom_color(colors::blue()).bold()
    );

    let mut server_process = build_and_run(&config)?;

    if config.reload {
        // Set up file watcher
        let (tx, rx) = mpsc::channel();

        let mut debouncer = new_debouncer(
            Duration::from_millis(300),
            move |res: DebounceEventResult| {
                if let Ok(events) = res {
                    for event in events {
                        if event.path.extension().is_some_and(|ext| ext == "rs") {
                            let _ = tx.send(());
                            break;
                        }
                    }
                }
            },
        )
        .map_err(|e| format!("Failed to create file watcher: {}", e))?;

        debouncer
            .watcher()
            .watch(Path::new("src"), RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch src directory: {}", e))?;

        println!(
            "{} Watching for changes in: {}",
            "INFO".custom_color(colors::blue()).bold(),
            "./src".custom_color(colors::sky())
        );

        // Main loop
        while running.load(Ordering::SeqCst) {
            // Check for file changes (non-blocking with timeout)
            if rx.recv_timeout(Duration::from_millis(100)).is_ok() {
                println!();
                println!(
                    "{} Change detected, rebuilding...",
                    "INFO".custom_color(colors::yellow()).bold()
                );

                // Kill current server
                let _ = server_process.kill();
                let _ = server_process.wait();

                // Rebuild and restart
                match build_and_run(&config) {
                    Ok(new_process) => {
                        server_process = new_process;
                    }
                    Err(e) => {
                        eprintln!("{} {}", "ERROR".custom_color(colors::red()).bold(), e);
                        // Keep waiting for more changes
                    }
                }
            }

            // Check if server process has exited unexpectedly
            if let Ok(Some(status)) = server_process.try_wait()
                && !status.success()
            {
                eprintln!(
                    "{} Server exited with status: {}",
                    "ERROR".custom_color(colors::red()).bold(),
                    status
                );
                // Wait for file change before trying to restart
            }
        }
    } else {
        // No reload, just wait for the server
        println!(
            "{} Hot reload disabled, press Ctrl+C to stop",
            "INFO".custom_color(colors::blue()).bold()
        );

        while running.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));

            // Check if server process has exited
            if let Ok(Some(status)) = server_process.try_wait() {
                if !status.success() {
                    return Err(format!("Server exited with status: {}", status));
                }
                break;
            }
        }
    }

    // Cleanup
    println!();
    println!(
        "{} Shutting down...",
        "INFO".custom_color(colors::blue()).bold()
    );
    let _ = server_process.kill();
    let _ = server_process.wait();

    Ok(())
}

/// Verify that we're in a valid Rapina project directory.
fn verify_rapina_project() -> Result<(), String> {
    let cargo_toml = Path::new("Cargo.toml");
    if !cargo_toml.exists() {
        return Err("No Cargo.toml found. Are you in a Rust project directory?".to_string());
    }

    let content = std::fs::read_to_string(cargo_toml)
        .map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;

    let parsed: toml::Value = content
        .parse()
        .map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    // Check for rapina in dependencies
    let has_rapina = parsed
        .get("dependencies")
        .and_then(|deps| deps.get("rapina"))
        .is_some();

    if !has_rapina {
        return Err(
            "This doesn't appear to be a Rapina project (no rapina dependency found)".to_string(),
        );
    }

    Ok(())
}

/// Build the project and run the server.
fn build_and_run(config: &DevConfig) -> Result<Child, String> {
    // Run cargo build
    let build_output = Command::new("cargo")
        .args(["build"])
        .output()
        .map_err(|e| format!("Failed to run cargo build: {}", e))?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        eprintln!("{}", stderr);
        return Err("Build failed".to_string());
    }

    println!(
        "{} Build successful",
        "INFO".custom_color(colors::green()).bold()
    );

    // Get the binary name from Cargo.toml
    let binary_name = get_binary_name()?;

    // Run the server
    let child = Command::new(format!("./target/debug/{}", binary_name))
        .env("RAPINA_HOST", &config.host)
        .env("RAPINA_PORT", config.port.to_string())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("Failed to start server: {}", e))?;

    println!(
        "{} Server started on {} (Press CTRL+C to quit)",
        "INFO".custom_color(colors::green()).bold(),
        format!("http://{}:{}", config.host, config.port).custom_color(colors::sky())
    );

    Ok(child)
}

/// Get the binary name from Cargo.toml.
fn get_binary_name() -> Result<String, String> {
    let content = std::fs::read_to_string("Cargo.toml")
        .map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;

    let parsed: toml::Value = content
        .parse()
        .map_err(|e| format!("Failed to parse Cargo.toml: {}", e))?;

    // Check for [[bin]] section first
    if let Some(bins) = parsed.get("bin").and_then(|b| b.as_array())
        && let Some(first_bin) = bins.first()
        && let Some(name) = first_bin.get("name").and_then(|n| n.as_str())
    {
        return Ok(name.to_string());
    }

    // Fall back to package name
    parsed
        .get("package")
        .and_then(|pkg| pkg.get("name"))
        .and_then(|name| name.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Could not determine binary name from Cargo.toml".to_string())
}

/// Catppuccin Mocha color palette
mod colors {
    use colored::CustomColor;

    pub fn mauve() -> CustomColor {
        CustomColor::new(203, 166, 247)
    }

    pub fn lavender() -> CustomColor {
        CustomColor::new(180, 190, 254)
    }

    pub fn sky() -> CustomColor {
        CustomColor::new(137, 220, 235)
    }

    pub fn subtext() -> CustomColor {
        CustomColor::new(166, 173, 200)
    }

    pub fn green() -> CustomColor {
        CustomColor::new(166, 227, 161)
    }

    pub fn yellow() -> CustomColor {
        CustomColor::new(249, 226, 175)
    }

    pub fn red() -> CustomColor {
        CustomColor::new(243, 139, 168)
    }

    pub fn blue() -> CustomColor {
        CustomColor::new(137, 180, 250)
    }
}

/// Print the development server banner.
fn print_banner(config: &DevConfig) {
    let url = format!("http://{}:{}", config.host, config.port);
    let routes_url = format!("{}/.__rapina/routes", url);

    // Box is 61 chars wide total, 59 chars inner content
    let b = "│".custom_color(colors::mauve());

    println!();
    println!(
        " {}",
        "╭───────────────────────────────────────────────────────────╮"
            .custom_color(colors::mauve())
    );

    // Title - centered in 59 chars
    println!(
        " {}{}{}",
        b,
        format!("{:^59}", "Rapina CLI - Development Mode")
            .custom_color(colors::lavender())
            .bold(),
        b
    );

    println!(
        " {}",
        "├───────────────────────────────────────────────────────────┤"
            .custom_color(colors::mauve())
    );

    // Empty line
    println!(" {}{:59}{}", b, "", b);

    // Serving at: align label and value
    let serving_line = format!(" Serving at: {:<44}", url);
    println!(
        " {}{}{}",
        b,
        format!("{:59}", serving_line).custom_color(colors::sky()),
        b
    );

    // Routes: align with Serving at
    let routes_line = format!(" Routes:     {:<44}", routes_url);
    println!(
        " {}{}{}",
        b,
        format!("{:59}", routes_line).custom_color(colors::sky()),
        b
    );

    // Empty line
    println!(" {}{:59}{}", b, "", b);

    // Production hint
    println!(
        " {}{}{}",
        b,
        format!(" {:<58}", "For production: cargo build --release").custom_color(colors::subtext()),
        b
    );

    // Empty line
    println!(" {}{:59}{}", b, "", b);

    println!(
        " {}",
        "╰───────────────────────────────────────────────────────────╯"
            .custom_color(colors::mauve())
    );
    println!();
}
