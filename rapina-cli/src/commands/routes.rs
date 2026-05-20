//! List all registered routes.

use crate::common::urls;
use colored::Colorize;
use serde::Deserialize;
use std::fmt::Write as _;
use std::process::Command;

#[derive(Deserialize)]
struct RouteInfo {
    method: String,
    path: String,
    handler_name: String,
    #[serde(default)]
    response_schema: Option<serde_json::Value>,
    #[serde(default)]
    request_schema: Option<serde_json::Value>,
    #[serde(default)]
    request_content_type: Option<String>,
    #[serde(default)]
    request_body_required: Option<bool>,
    #[serde(default)]
    error_responses: Vec<ErrorResponse>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    status: u16,
    code: String,
    description: String,
}

impl RouteInfo {
    fn is_internal(&self) -> bool {
        self.path.starts_with("/__rapina")
    }
}

pub struct RoutesConfig {
    pub host: String,
    pub port: u16,
    pub llms: bool,
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

    if config.llms {
        println!();
        let title = format!("http://{}:{}", config.host, config.port);
        print!("{}", to_llms_txt(&title, &routes));
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

/// Render the route list as an llms.txt Markdown document.
fn to_llms_txt(title: &str, routes: &[RouteInfo]) -> String {
    let mut out = String::new();

    let user_routes: Vec<&RouteInfo> = routes.iter().filter(|r| !r.is_internal()).collect();

    writeln!(out, "# {title}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "## Routes").unwrap();

    for route in &user_routes {
        writeln!(out).unwrap();
        writeln!(out, "### {} {}", route.method, route.path).unwrap();

        if let Some(ct) = &route.request_content_type {
            if let Some(schema) = &route.request_schema {
                let required = route.request_body_required.unwrap_or(true);
                writeln!(
                    out,
                    "\nRequest ({}){}:",
                    ct,
                    if required { "" } else { " (optional)" }
                )
                .unwrap();
                let pretty = serde_json::to_string_pretty(schema).unwrap_or_default();
                writeln!(out, "```json").unwrap();
                writeln!(out, "{pretty}").unwrap();
                writeln!(out, "```").unwrap();
            }
        }

        if let Some(schema) = &route.response_schema {
            writeln!(out, "\nResponse:").unwrap();
            let pretty = serde_json::to_string_pretty(schema).unwrap_or_default();
            writeln!(out, "```json").unwrap();
            writeln!(out, "{pretty}").unwrap();
            writeln!(out, "```").unwrap();
        }

        if !route.error_responses.is_empty() {
            writeln!(out, "\nErrors:").unwrap();
            for err in &route.error_responses {
                writeln!(out, "  - {} {}: {}", err.status, err.code, err.description).unwrap();
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_route(method: &str, path: &str) -> RouteInfo {
        RouteInfo {
            method: method.to_string(),
            path: path.to_string(),
            handler_name: "handler".to_string(),
            response_schema: None,
            request_schema: None,
            request_content_type: None,
            request_body_required: None,
            error_responses: vec![],
        }
    }

    #[test]
    fn test_to_llms_txt_heading() {
        let routes = vec![make_route("GET", "/users")];
        let out = to_llms_txt("My API", &routes);
        assert!(out.starts_with("# My API\n"));
    }

    #[test]
    fn test_to_llms_txt_contains_routes_section() {
        let routes = vec![make_route("GET", "/users")];
        let out = to_llms_txt("API", &routes);
        assert!(out.contains("## Routes"));
        assert!(out.contains("### GET /users"));
    }

    #[test]
    fn test_to_llms_txt_filters_internal_routes() {
        let routes = vec![
            make_route("GET", "/users"),
            make_route("GET", "/__rapina/routes"),
        ];
        let out = to_llms_txt("API", &routes);
        assert!(out.contains("/users"));
        assert!(!out.contains("/__rapina"));
    }

    #[test]
    fn test_to_llms_txt_empty_routes() {
        let out = to_llms_txt("API", &[]);
        assert!(out.starts_with("# API\n"));
        assert!(out.contains("## Routes"));
        assert!(!out.contains("###"));
    }

    #[test]
    fn test_to_llms_txt_request_schema() {
        let route = RouteInfo {
            method: "POST".to_string(),
            path: "/items".to_string(),
            handler_name: "create_item".to_string(),
            response_schema: None,
            request_schema: Some(json!({"type": "object"})),
            request_content_type: Some("application/json".to_string()),
            request_body_required: Some(true),
            error_responses: vec![],
        };
        let out = to_llms_txt("API", &[route]);
        assert!(out.contains("Request (application/json):"));
        assert!(out.contains("```json"));
    }

    #[test]
    fn test_to_llms_txt_optional_request_body() {
        let route = RouteInfo {
            method: "POST".to_string(),
            path: "/items".to_string(),
            handler_name: "create_item".to_string(),
            response_schema: None,
            request_schema: Some(json!({"type": "object"})),
            request_content_type: Some("application/json".to_string()),
            request_body_required: Some(false),
            error_responses: vec![],
        };
        let out = to_llms_txt("API", &[route]);
        assert!(out.contains("(optional)"));
    }

    #[test]
    fn test_to_llms_txt_response_schema() {
        let route = RouteInfo {
            method: "GET".to_string(),
            path: "/items".to_string(),
            handler_name: "list_items".to_string(),
            response_schema: Some(json!({"type": "array"})),
            request_schema: None,
            request_content_type: None,
            request_body_required: None,
            error_responses: vec![],
        };
        let out = to_llms_txt("API", &[route]);
        assert!(out.contains("Response:"));
        assert!(out.contains("```json"));
    }

    #[test]
    fn test_to_llms_txt_error_responses() {
        let route = RouteInfo {
            method: "GET".to_string(),
            path: "/items/:id".to_string(),
            handler_name: "get_item".to_string(),
            response_schema: None,
            request_schema: None,
            request_content_type: None,
            request_body_required: None,
            error_responses: vec![ErrorResponse {
                status: 404,
                code: "NOT_FOUND".to_string(),
                description: "item not found".to_string(),
            }],
        };
        let out = to_llms_txt("API", &[route]);
        assert!(out.contains("Errors:"));
        assert!(out.contains("404 NOT_FOUND: item not found"));
    }

    #[test]
    fn test_is_internal() {
        let internal = make_route("GET", "/__rapina/routes");
        assert!(internal.is_internal());
        let user = make_route("GET", "/users");
        assert!(!user.is_internal());
    }

    #[test]
    fn test_route_info_deserialize_minimal() {
        let json = r#"[{"method":"GET","path":"/users","handler_name":"list_users"}]"#;
        let routes: Vec<RouteInfo> = serde_json::from_str(json).unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].method, "GET");
        assert_eq!(routes[0].path, "/users");
        assert!(routes[0].response_schema.is_none());
        assert!(routes[0].error_responses.is_empty());
    }

    #[test]
    fn test_route_info_deserialize_full() {
        let json = json!([{
            "method": "POST",
            "path": "/users",
            "handler_name": "create_user",
            "request_schema": {"type": "object"},
            "request_content_type": "application/json",
            "request_body_required": true,
            "response_schema": {"type": "object"},
            "error_responses": [{"status": 409, "code": "CONFLICT", "description": "duplicate"}]
        }])
        .to_string();
        let routes: Vec<RouteInfo> = serde_json::from_str(&json).unwrap();
        assert_eq!(routes[0].error_responses.len(), 1);
        assert_eq!(routes[0].error_responses[0].status, 409);
        assert!(routes[0].request_schema.is_some());
    }
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
