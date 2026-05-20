//! OpenAPI specification tools.

use colored::Colorize;
use serde_json::Value;
use std::fs;
use std::process::Command;

/// Export OpenAPI spec to stdout or file.
pub fn export(output: Option<String>, host: &str, port: u16) -> Result<(), String> {
    let agent = ureq::Agent::new_with_defaults();
    let spec = fetch_openapi_spec(&agent, host, port)?;
    let canonical = canonicalize_json(&spec)?;

    match output {
        Some(path) => {
            fs::write(&path, &canonical).map_err(|e| format!("Failed to write file: {}", e))?;
            println!("  {} OpenAPI spec exported to {}", "✓".green(), path.cyan());
        }
        None => {
            println!("{}", canonical);
        }
    }

    Ok(())
}

/// Check if the committed openapi.json matches the current code.
pub fn check(file: &str, host: &str, port: u16) -> Result<(), String> {
    println!();
    println!("  {} Checking OpenAPI spec...", "→".cyan());

    // Read committed file
    let committed =
        fs::read_to_string(file).map_err(|e| format!("Failed to read {}: {}", file, e))?;
    let committed_json: Value =
        serde_json::from_str(&committed).map_err(|e| format!("Failed to parse {}: {}", file, e))?;

    let agent = ureq::Agent::new_with_defaults();

    // Fetch current spec
    let current = fetch_openapi_spec(&agent, host, port)?;

    // Compare canonical versions
    let committed_canonical = canonicalize_json(&committed_json)?;
    let current_canonical = canonicalize_json(&current)?;

    if committed_canonical == current_canonical {
        println!("  {} OpenAPI spec is up to date", "✓".green());
        Ok(())
    } else {
        println!("  {} OpenAPI spec is outdated", "✗".red());
        println!();
        println!(
            "  Run {} to update it.",
            "rapina openapi export -o openapi.json".cyan()
        );
        Err("OpenAPI spec doesn't match the current code".to_string())
    }
}

/// Compare spec with another branch and detect breaking changes.
pub fn diff(base: &str, file: &str, host: &str, port: u16) -> Result<(), String> {
    println!();
    println!(
        "  {} Comparing OpenAPI spec with {} branch...",
        "→".cyan(),
        base.yellow()
    );

    // Get spec from base branch using git
    let base_spec = get_spec_from_branch(base, file)?;

    let agent = ureq::Agent::new_with_defaults();

    // Fetch current spec
    let current_spec = fetch_openapi_spec(&agent, host, port)?;

    // Detect breaking changes
    let changes = detect_breaking_changes(&base_spec, &current_spec);

    if changes.breaking.is_empty() && changes.non_breaking.is_empty() {
        println!("  {} No API changes detected", "✓".green());
        return Ok(());
    }

    println!();

    if !changes.breaking.is_empty() {
        println!("  {} Breaking changes:", "✗".red().bold());
        for change in &changes.breaking {
            println!("    {} {}", "•".red(), change);
        }
        println!();
    }

    if !changes.non_breaking.is_empty() {
        println!("  {} Non-breaking changes:", "⚠".yellow());
        for change in &changes.non_breaking {
            println!("    {} {}", "•".yellow(), change);
        }
        println!();
    }

    if !changes.breaking.is_empty() {
        Err(format!(
            "Found {} breaking change(s)",
            changes.breaking.len()
        ))
    } else {
        Ok(())
    }
}

/// Fetch OpenAPI spec from running application.
fn fetch_openapi_spec(agent: &ureq::Agent, host: &str, port: u16) -> Result<Value, String> {
    let url = crate::common::urls::build_openapi_url(host, port);
    let mut response = agent.get(&url).call().map_err(|e| {
        format!(
            "Failed to fetch OpenAPI spec. Is the server running on {}? ({})",
            url, e
        )
    })?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    serde_json::from_str(&body).map_err(|e| format!("Invalid JSON response: {}", e))
}

/// Get OpenAPI spec from a git branch.
fn get_spec_from_branch(branch: &str, file: &str) -> Result<Value, String> {
    let output = Command::new("git")
        .args(["show", &format!("{}:{}", branch, file)])
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Failed to get {} from branch '{}': {}",
            file,
            branch,
            stderr.trim()
        ));
    }

    let body = String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 in git output: {}", e))?;

    serde_json::from_str(&body).map_err(|e| format!("Invalid JSON in {}: {}", file, e))
}

/// Canonicalize JSON for consistent comparison.
fn canonicalize_json(value: &Value) -> Result<String, String> {
    serde_json::to_string_pretty(value).map_err(|e| format!("Failed to serialize JSON: {}", e))
}

/// Result of breaking change detection.
struct ChangeReport {
    breaking: Vec<String>,
    non_breaking: Vec<String>,
}

/// Detect breaking changes between two OpenAPI specs.
fn detect_breaking_changes(base: &Value, current: &Value) -> ChangeReport {
    let mut report = ChangeReport {
        breaking: Vec::new(),
        non_breaking: Vec::new(),
    };

    let base_paths = base.get("paths").and_then(|p| p.as_object());
    let current_paths = current.get("paths").and_then(|p| p.as_object());

    if let (Some(base_paths), Some(current_paths)) = (base_paths, current_paths) {
        // Check for removed endpoints
        for (path, base_item) in base_paths {
            match current_paths.get(path) {
                None => {
                    report.breaking.push(format!("Removed endpoint: {}", path));
                }
                Some(current_item) => {
                    // Check for removed methods
                    check_removed_methods(path, base_item, current_item, &mut report);
                    // Check for response schema changes
                    check_response_changes(path, base_item, current_item, &mut report);
                }
            }
        }

        // Check for new endpoints (non-breaking)
        for path in current_paths.keys() {
            if !base_paths.contains_key(path) {
                report
                    .non_breaking
                    .push(format!("Added endpoint: {}", path));
            }
        }
    }

    report
}

/// Check for removed HTTP methods on an endpoint.
fn check_removed_methods(
    path: &str,
    base_item: &Value,
    current_item: &Value,
    report: &mut ChangeReport,
) {
    let methods = ["get", "post", "put", "delete", "patch"];

    for method in methods {
        let base_has = base_item.get(method).is_some();
        let current_has = current_item.get(method).is_some();

        if base_has && !current_has {
            report.breaking.push(format!(
                "Removed method: {} {}",
                method.to_uppercase(),
                path
            ));
        } else if !base_has && current_has {
            report
                .non_breaking
                .push(format!("Added method: {} {}", method.to_uppercase(), path));
        }
    }
}

/// Check for breaking changes in response schemas.
fn check_response_changes(
    path: &str,
    base_item: &Value,
    current_item: &Value,
    report: &mut ChangeReport,
) {
    let methods = ["get", "post", "put", "delete", "patch"];

    for method in methods {
        if let (Some(base_op), Some(current_op)) = (base_item.get(method), current_item.get(method))
        {
            // Check for removed required fields in response
            if let (Some(base_resp), Some(current_resp)) = (
                base_op
                    .get("responses")
                    .and_then(|r| r.get("200"))
                    .and_then(|r| r.get("content"))
                    .and_then(|c| c.get("application/json"))
                    .and_then(|m| m.get("schema")),
                current_op
                    .get("responses")
                    .and_then(|r| r.get("200"))
                    .and_then(|r| r.get("content"))
                    .and_then(|c| c.get("application/json"))
                    .and_then(|m| m.get("schema")),
            ) {
                check_schema_changes(
                    &format!("{} {}", method.to_uppercase(), path),
                    base_resp,
                    current_resp,
                    report,
                );
            }
        }
    }
}

/// Check for breaking changes in schemas.
fn check_schema_changes(
    context: &str,
    base_schema: &Value,
    current_schema: &Value,
    report: &mut ChangeReport,
) {
    // Check for removed required fields
    if let (Some(base_props), Some(current_props)) = (
        base_schema.get("properties").and_then(|p| p.as_object()),
        current_schema.get("properties").and_then(|p| p.as_object()),
    ) {
        for prop in base_props.keys() {
            if !current_props.contains_key(prop) {
                report
                    .breaking
                    .push(format!("{}: removed field '{}'", context, prop));
            }
        }

        for prop in current_props.keys() {
            if !base_props.contains_key(prop) {
                report
                    .non_breaking
                    .push(format!("{}: added field '{}'", context, prop));
            }
        }
    }

    // Check for type changes
    if let (Some(base_type), Some(current_type)) =
        (base_schema.get("type"), current_schema.get("type"))
        && base_type != current_type
    {
        report.breaking.push(format!(
            "{}: type changed from {} to {}",
            context, base_type, current_type
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_removed_endpoint() {
        let base = json!({
            "paths": {
                "/users": { "get": {} },
                "/posts": { "get": {} }
            }
        });
        let current = json!({
            "paths": {
                "/users": { "get": {} }
            }
        });

        let report = detect_breaking_changes(&base, &current);
        assert!(report.breaking.iter().any(|c| c.contains("/posts")));
    }

    #[test]
    fn test_detect_added_endpoint() {
        let base = json!({
            "paths": {
                "/users": { "get": {} }
            }
        });
        let current = json!({
            "paths": {
                "/users": { "get": {} },
                "/posts": { "get": {} }
            }
        });

        let report = detect_breaking_changes(&base, &current);
        assert!(report.breaking.is_empty());
        assert!(report.non_breaking.iter().any(|c| c.contains("/posts")));
    }

    #[test]
    fn test_detect_removed_method() {
        let base = json!({
            "paths": {
                "/users": { "get": {}, "post": {} }
            }
        });
        let current = json!({
            "paths": {
                "/users": { "get": {} }
            }
        });

        let report = detect_breaking_changes(&base, &current);
        assert!(report.breaking.iter().any(|c| c.contains("POST")));
    }

    #[test]
    fn test_no_changes() {
        let spec = json!({
            "paths": {
                "/users": { "get": {} }
            }
        });

        let report = detect_breaking_changes(&spec, &spec);
        assert!(report.breaking.is_empty());
        assert!(report.non_breaking.is_empty());
    }

    #[test]
    fn test_fetch_openapi_spec_success() {
        let json = r#"{"openapi":"3.0.0","info":{"title":"Test","version":"1.0"}}"#;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0; 4096];
            use std::io::Read;
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
                json.len(),
                json
            );
            use std::io::Write;
            stream.write_all(response.as_bytes()).unwrap();
        });
        let agent = ureq::Agent::new_with_defaults();
        let result = fetch_openapi_spec(&agent, "127.0.0.1", port);
        handle.join().unwrap();
        let value = result.unwrap();
        assert_eq!(value["info"]["title"], "Test");
    }

    #[test]
    fn test_fetch_openapi_spec_connection_refused() {
        let agent = ureq::Agent::new_with_defaults();
        let result = fetch_openapi_spec(&agent, "127.0.0.1", 1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to fetch"));
    }
}
