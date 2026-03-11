//! Health checks for your Rapina API.

use crate::common::urls;
use colored::Colorize;
use serde_json::Value;
use std::process::Command;

struct DiagnosticResult {
    warnings: Vec<String>,
    errors: Vec<String>,
    passed: Vec<String>,
}

pub struct DoctorConfig {
    pub host: String,
    pub port: u16,
}

/// Run health checks on the API.
pub fn execute(config: DoctorConfig) -> Result<(), String> {
    println!();
    println!(
        "  {} Running API health checks on http://{}:{}...",
        "→".cyan(),
        config.host,
        config.port
    );
    println!();

    let routes = fetch_json(&urls::build_routes_url(&config.host, config.port))?;
    let openapi = fetch_json(&urls::build_openapi_url(&config.host, config.port));

    let mut result = DiagnosticResult {
        warnings: Vec::new(),
        errors: Vec::new(),
        passed: Vec::new(),
    };

    check_response_schemas(&routes, &mut result);
    check_error_documentation(&routes, &mut result);
    check_openapi_metadata(&openapi, &mut result);
    check_duplicate_routes(&routes, &mut result);

    print_results(&result);

    if !result.errors.is_empty() {
        Err(format!("Found {} error(s)", result.errors.len()))
    } else {
        Ok(())
    }
}

/// Check that routes have response schemas.
fn check_response_schemas(routes: &Value, result: &mut DiagnosticResult) {
    let routes_array = match routes.as_array() {
        Some(arr) => arr,
        None => return,
    };

    let mut missing = Vec::new();

    for route in routes_array {
        let path = route.get("path").and_then(|p| p.as_str()).unwrap_or("?");
        let method = route.get("method").and_then(|m| m.as_str()).unwrap_or("?");
        let has_schema = route.get("response_schema").is_some();

        if !has_schema && !path.starts_with("/__rapina") {
            missing.push(format!("{} {}", method, path));
        }
    }

    if missing.is_empty() {
        result
            .passed
            .push("All routes have response schemas".to_string());
    } else {
        for route in missing {
            result
                .warnings
                .push(format!("Missing response schema: {}", route));
        }
    }
}

/// Check that routes have error documentation.
fn check_error_documentation(routes: &Value, result: &mut DiagnosticResult) {
    let routes_array = match routes.as_array() {
        Some(arr) => arr,
        None => return,
    };

    let mut missing = Vec::new();

    for route in routes_array {
        let path = route.get("path").and_then(|p| p.as_str()).unwrap_or("?");
        let method = route.get("method").and_then(|m| m.as_str()).unwrap_or("?");
        let error_responses = route
            .get("error_responses")
            .and_then(|e| e.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        if error_responses == 0 && !path.starts_with("/__rapina") {
            missing.push(format!("{} {}", method, path));
        }
    }

    if missing.is_empty() {
        result
            .passed
            .push("All routes have documented errors".to_string());
    } else {
        for route in missing {
            result
                .warnings
                .push(format!("No documented errors: {}", route));
        }
    }
}

/// Check for duplicate (method, path) pairs — only one handler is used, others are shadowed.
fn check_duplicate_routes(routes: &Value, result: &mut DiagnosticResult) {
    let routes_array = match routes.as_array() {
        Some(arr) => arr,
        None => return,
    };

    let mut by_key: std::collections::HashMap<(String, String), Vec<String>> =
        std::collections::HashMap::new();

    for route in routes_array {
        let path = route
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("?")
            .to_string();
        if path.starts_with("/__rapina") {
            continue;
        }
        let method = route
            .get("method")
            .and_then(|m| m.as_str())
            .unwrap_or("?")
            .to_string();
        let handler_name = route
            .get("handler_name")
            .and_then(|h| h.as_str())
            .unwrap_or("?")
            .to_string();

        by_key.entry((method, path)).or_default().push(handler_name);
    }

    for ((method, path), handlers) in &by_key {
        if handlers.len() <= 1 {
            continue;
        }
        let list = handlers.join(", ");
        result.warnings.push(format!(
            "Duplicate route {} {}: handlers [{}] — only the first match is used, others are shadowed",
            method, path, list
        ));
    }

    if !by_key.is_empty() && by_key.values().all(|v| v.len() <= 1) {
        result.passed.push("No duplicate handler paths".to_string());
    }
}

/// Check OpenAPI metadata.
fn check_openapi_metadata(openapi: &Result<Value, String>, result: &mut DiagnosticResult) {
    let openapi = match openapi {
        Ok(openapi) => openapi,
        Err(_) => {
            result
                .warnings
                .push("OpenAPI endpoint: not enabled (add .openapi() to enable)".to_string());
            return;
        }
    };

    let paths = match openapi.get("paths").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return,
    };

    let mut missing_summary = Vec::new();

    for (path, item) in paths {
        if path.starts_with("/__rapina") {
            continue;
        }

        for method in ["get", "post", "put", "delete"] {
            if let Some(operation) = item.get(method) {
                let has_summary = operation.get("summary").is_some();
                let has_description = operation.get("description").is_some();

                if !has_summary && !has_description {
                    missing_summary.push(format!("{} {}", method.to_uppercase(), path));
                }
            }
        }
    }

    if missing_summary.is_empty() {
        result
            .passed
            .push("All operations have descriptions".to_string());
    } else {
        for op in missing_summary {
            result
                .warnings
                .push(format!("Missing documentation: {}", op));
        }
    }
}

/// Print diagnostic results.
fn print_results(result: &DiagnosticResult) {
    // Print passed checks
    for msg in &result.passed {
        println!("  {} {}", "✓".green(), msg);
    }

    // Print warnings
    for msg in &result.warnings {
        println!("  {} {}", "⚠".yellow(), msg);
    }

    // Print errors
    for msg in &result.errors {
        println!("  {} {}", "✗".red(), msg);
    }

    println!();

    // Summary
    println!(
        "  {} {} passed, {} warnings, {} errors",
        "Summary:".bold(),
        result.passed.len().to_string().green(),
        result.warnings.len().to_string().yellow(),
        result.errors.len().to_string().red()
    );
    println!();

    if result.warnings.is_empty() && result.errors.is_empty() {
        println!("  Your API is healthy.");
    } else if result.errors.is_empty() {
        println!("  Consider addressing the warnings above.");
    }
    println!();
}

/// Fetch JSON from URL.
fn fetch_json(url: &str) -> Result<Value, String> {
    let output = Command::new("curl")
        .args(["-s", "-f", url])
        .output()
        .map_err(|e| format!("Failed to run curl: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to fetch data. Is the server running? ({})",
            url
        ));
    }

    let body =
        String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 response: {}", e))?;

    serde_json::from_str(&body).map_err(|e| format!("Invalid JSON response: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_duplicate_routes_warns_on_same_method_path() {
        let routes = serde_json::json!([
            {"method": "GET", "path": "/users", "handler_name": "list_users"},
            {"method": "GET", "path": "/users", "handler_name": "other_list"},
            {"method": "POST", "path": "/users", "handler_name": "create_user"},
        ]);
        let mut result = DiagnosticResult {
            warnings: Vec::new(),
            errors: Vec::new(),
            passed: Vec::new(),
        };
        check_duplicate_routes(&routes, &mut result);
        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("Duplicate route GET /users"));
        assert!(result.warnings[0].contains("list_users"));
        assert!(result.warnings[0].contains("other_list"));
        assert!(result.warnings[0].contains("shadowed"));
        assert!(result.passed.is_empty());
    }

    #[test]
    fn check_duplicate_routes_ignores_internal_routes() {
        let routes = serde_json::json!([
            {"method": "GET", "path": "/__rapina/routes", "handler_name": "list_routes"},
            {"method": "GET", "path": "/__rapina/routes", "handler_name": "other"},
        ]);
        let mut result = DiagnosticResult {
            warnings: Vec::new(),
            errors: Vec::new(),
            passed: Vec::new(),
        };
        check_duplicate_routes(&routes, &mut result);
        assert!(result.warnings.is_empty());
        assert!(result.passed.is_empty());
    }

    #[test]
    fn check_duplicate_routes_passed_when_no_duplicates() {
        let routes = serde_json::json!([
            {"method": "GET", "path": "/users", "handler_name": "list_users"},
            {"method": "POST", "path": "/users", "handler_name": "create_user"},
        ]);
        let mut result = DiagnosticResult {
            warnings: Vec::new(),
            errors: Vec::new(),
            passed: Vec::new(),
        };
        check_duplicate_routes(&routes, &mut result);
        assert!(result.warnings.is_empty());
        assert_eq!(result.passed.len(), 1);
        assert_eq!(result.passed[0], "No duplicate handler paths");
    }
}
