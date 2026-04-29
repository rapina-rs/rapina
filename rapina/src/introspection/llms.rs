//! llms.txt generation for Rapina applications.
//!
//! Converts the route registry into a Markdown document following the
//! llms.txt convention (<https://llmstxt.org/>), suitable for serving at
//! `/__rapina/llms.txt` so AI agents can discover all routes without
//! scraping HTML or guessing.

use std::fmt::Write as _;

use crate::introspection::RouteInfo;

macro_rules! w {
    ($dst:expr) => { writeln!($dst).unwrap() };
    ($dst:expr, $($t:tt)*) => { writeln!($dst, $($t)*).unwrap() };
}

/// Render the route registry as an llms.txt Markdown document.
pub fn to_llms_txt(title: &str, routes: &[RouteInfo]) -> String {
    let mut out = String::new();

    let user_routes: Vec<&RouteInfo> = routes.iter().filter(|r| !r.is_internal()).collect();

    w!(out, "# {title}");
    w!(out);
    w!(
        out,
        "Built with [Rapina](https://rapina.rs) v{}.",
        env!("CARGO_PKG_VERSION")
    );
    w!(out);
    w!(out, "## Routes");

    for route in &user_routes {
        w!(out);
        w!(out, "### {} {}", route.method, route.path);

        if let Some(ct) = &route.request_content_type {
            if let Some(schema) = &route.request_schema {
                let required = route.request_body_required.unwrap_or(true);
                w!(
                    out,
                    "\nRequest ({}){}:",
                    ct,
                    if required { "" } else { " (optional)" }
                );
                let pretty = serde_json::to_string_pretty(schema).unwrap_or_default();
                w!(out, "```json");
                w!(out, "{pretty}");
                w!(out, "```");
            }
        }

        if let Some(schema) = &route.response_schema {
            w!(out, "\nResponse:");
            let pretty = serde_json::to_string_pretty(schema).unwrap_or_default();
            w!(out, "```json");
            w!(out, "{pretty}");
            w!(out, "```");
        }

        if !route.error_responses.is_empty() {
            w!(out, "\nErrors:");
            for err in &route.error_responses {
                w!(out, "  - {} {}: {}", err.status, err.code, err.description);
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
    fn test_to_llms_txt_snapshot() {
        let routes = vec![make_route()];
        let output = to_llms_txt("My API", &routes);
        insta::assert_snapshot!(output);
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
        let output = to_llms_txt("My API", &routes);
        assert!(
            !output.contains("/__rapina"),
            "internal routes must be filtered"
        );
    }

    #[test]
    fn test_to_llms_txt_empty_routes() {
        let output = to_llms_txt("My API", &[]);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn test_to_llms_txt_title_used_as_heading() {
        let output = to_llms_txt("Custom Title", &[]);
        assert!(output.starts_with("# Custom Title\n"));
    }
}
