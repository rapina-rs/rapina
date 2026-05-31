//! List all registered routes.

use crate::common::urls;
use colored::Colorize;
use serde::Deserialize;

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
    let mut response = crate::common::AGENT.get(url).call().map_err(|e| {
        format!(
            "Failed to fetch routes. Is the server running on {}? ({})",
            url, e
        )
    })?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    serde_json::from_str(&body).map_err(|e| format!("Invalid JSON response: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_routes_success() {
        let json = r#"[{"method":"GET","path":"/users","handler_name":"list_users"}]"#;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 4096];
            use std::io::Read;
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                json.len(),
                json
            );
            use std::io::Write;
            stream.write_all(response.as_bytes()).unwrap();
        });
        let result = fetch_routes(&format!("http://127.0.0.1:{}", port));
        handle.join().unwrap();
        let routes = result.unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/users");
        assert_eq!(routes[0].handler_name, "list_users");
    }

    #[test]
    fn test_fetch_routes_connection_refused() {
        let result = fetch_routes("http://127.0.0.1:1");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to fetch"));
    }
}
