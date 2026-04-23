//! llms.txt generation for Rapina applications.
//!
//! Converts the route registry into a Markdown document following the
//! llms.txt convention (<https://llmstxt.org/>), suitable for serving at
//! `/__rapina/llms.txt` so AI agents can discover all routes without
//! scraping HTML or guessing.

use std::fmt::Write as _;

use crate::introspection::RouteInfo;

/// Render the route registry as an llms.txt Markdown document.
///
/// The output is a single self-contained file (no index/detail split —
/// that is a docs-site concern). JSON Schemas are rendered verbatim.
pub fn to_llms_txt(routes: &[RouteInfo]) -> String {
    let mut out = String::new();

    // Filter out internal /__rapina/* routes so the document only describes
    // user-defined API surface.
    let user_routes: Vec<&RouteInfo> = routes
        .iter()
        .filter(|r| !r.path.starts_with("/__rapina"))
        .collect();

    writeln!(out, "# API").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "Built with [Rapina](https://rapina.rs) v{}.",
        env!("CARGO_PKG_VERSION")
    )
    .unwrap();
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
                for line in pretty.lines() {
                    writeln!(out, "  {}", line).unwrap();
                }
            }
        }

        if let Some(schema) = &route.response_schema {
            writeln!(out, "\nResponse:").unwrap();
            let pretty = serde_json::to_string_pretty(schema).unwrap_or_default();
            for line in pretty.lines() {
                writeln!(out, "  {}", line).unwrap();
            }
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
    use crate::error::ErrorVariant;
    use crate::introspection::RouteInfo;
    use serde_json::json;

    fn make_route() -> RouteInfo {
        RouteInfo::new(
            "POST",
            "/v1/users",
            "create_user",
            Some(json!({"type": "object", "properties": {"id": {"type": "number"}}})),
            Some(json!({"type": "object", "properties": {"email": {"type": "string"}}})),
            Some("application/json"),
            Some(true),
            vec![ErrorVariant {
                status: 409,
                code: "CONFLICT",
                description: "email already registered",
            }],
        )
    }

    #[test]
    fn test_to_llms_txt_contains_route_heading() {
        let routes = vec![make_route()];
        let output = to_llms_txt(&routes);
        assert!(
            output.contains("### POST /v1/users"),
            "missing route heading"
        );
    }

    #[test]
    fn test_to_llms_txt_contains_routes_section() {
        let routes = vec![make_route()];
        let output = to_llms_txt(&routes);
        assert!(output.contains("## Routes"));
    }

    #[test]
    fn test_to_llms_txt_contains_request_schema() {
        let routes = vec![make_route()];
        let output = to_llms_txt(&routes);
        assert!(output.contains("Request (application/json)"));
        assert!(output.contains("email"));
    }

    #[test]
    fn test_to_llms_txt_contains_error_variants() {
        let routes = vec![make_route()];
        let output = to_llms_txt(&routes);
        assert!(output.contains("409 CONFLICT"));
        assert!(output.contains("email already registered"));
    }

    #[test]
    fn test_to_llms_txt_filters_internal_routes() {
        let internal = RouteInfo::new(
            "GET",
            "/__rapina/routes",
            "list_routes",
            None,
            None,
            None::<String>,
            None,
            vec![],
        );
        let routes = vec![make_route(), internal];
        let output = to_llms_txt(&routes);
        assert!(
            !output.contains("/__rapina"),
            "internal routes must be filtered"
        );
    }

    #[test]
    fn test_to_llms_txt_empty_routes() {
        let output = to_llms_txt(&[]);
        assert!(output.contains("## Routes"));
        assert!(!output.contains("###"));
    }
}
