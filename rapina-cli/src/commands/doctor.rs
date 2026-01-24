//! Health checks for your Rapina API.

use colored::Colorize;
use serde_json::Value;
use std::process::Command;

const ROUTES_URL: &str = "http://127.0.0.1:3000/__rapina/routes";
const OPENAPI_URL: &str = "http://127.0.0.1:3000/__rapina/openapi.json";

struct DiagnosticResult {
    warnings: Vec<String>,
    errors: Vec<String>,
    passed: Vec<String>,
}

/// Run health checks on the API.
pub fn execute() -> Result<(), String> {
    println!();
    println!("  {} Running API health checks...", "→".cyan());
    println!();

    let routes = fetch_json(ROUTES_URL)?;
    let openapi = fetch_json(OPENAPI_URL)?;

    let mut result = DiagnosticResult {
        warnings: Vec::new(),
        errors: Vec::new(),
        passed: Vec::new(),
    };

    check_response_schemas(&routes, &mut result);
    check_error_documentation(&routes, &mut result);
    check_openapi_metadata(&openapi, &mut result);

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

/// Check OpenAPI metadata.
fn check_openapi_metadata(openapi: &Value, result: &mut DiagnosticResult) {
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
