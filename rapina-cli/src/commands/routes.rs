//! List all registered routes.

use colored::Colorize;
use serde::Deserialize;
use std::process::Command;

const DEFAULT_URL: &str = "http://127.0.0.1:3000/__rapina/routes";

#[derive(Deserialize)]
struct RouteInfo {
    method: String,
    path: String,
    handler_name: String,
}

/// List all registered routes from the running application.
pub fn execute() -> Result<(), String> {
    println!();
    println!("  {} Fetching routes...", "→".cyan());

    let routes = fetch_routes()?;

    if routes.is_empty() {
        println!("  {} No routes registered", "⚠".yellow());
        return Ok(());
    }

    println!();
    println!(
        "  {:<6}  {:<20}  {}",
        "METHOD".bold(),
        "PATH".bold(),
        "HANDLER".bold()
    );
    println!("  ──────  ────────────────────  ───────────────");

    for route in &routes {
        let method_colored = match route.method.as_str() {
            "GET" => route.method.green(),
            "POST" => route.method.blue(),
            "PUT" => route.method.yellow(),
            "DELETE" => route.method.red(),
            _ => route.method.normal(),
        };
        println!(
            "  {:<6}  {:<20}  {}",
            method_colored,
            route.path.cyan(),
            route.handler_name
        );
    }

    println!();
    println!("  {} {} route(s) registered", "✓".green(), routes.len());
    println!();

    Ok(())
}

/// Fetch routes from running application.
fn fetch_routes() -> Result<Vec<RouteInfo>, String> {
    let output = Command::new("curl")
        .args(["-s", "-f", DEFAULT_URL])
        .output()
        .map_err(|e| format!("Failed to run curl: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to fetch routes. Is the server running on {}?",
            DEFAULT_URL
        ));
    }

    let body =
        String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 response: {}", e))?;

    serde_json::from_str(&body).map_err(|e| format!("Invalid JSON response: {}", e))
}
