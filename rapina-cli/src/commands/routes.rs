//! List all registered routes.

use crate::common::urls;
use colored::Colorize;
use serde::Deserialize;
use std::process::Command;

#[derive(Deserialize)]
struct RouteInfo {
    method: String,
    path: String,
    handler_name: String,
}

pub struct RoutesConfig {
    pub host: String,
    pub port: u16,
}

/// List all registered routes from the running application.
pub fn execute(config: RoutesConfig) -> Result<(), String> {
    println!();
    println!(
        "  {} Fetching routes on http://{}:{}...",
        "→".cyan(),
        config.host,
        config.port
    );
    let routes = fetch_routes(&urls::build_routes_url(&config.host, config.port))?;

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
fn fetch_routes(url: &str) -> Result<Vec<RouteInfo>, String> {
    let output = Command::new("curl")
        .args(["-s", "-f", url])
        .output()
        .map_err(|e| format!("Failed to run curl: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to fetch routes. Is the server running on {}?",
            url
        ));
    }

    let body =
        String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 response: {}", e))?;

    serde_json::from_str(&body).map_err(|e| format!("Invalid JSON response: {}", e))
}
