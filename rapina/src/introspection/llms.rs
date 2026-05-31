//! llms.txt generation for Rapina applications.
//!
//! Converts the route registry into a Markdown document following the
//! llms.txt convention (<https://llmstxt.org/>), suitable for serving at
//! `/__rapina/llms.txt` so AI agents can discover all routes without
//! scraping HTML or guessing.

use std::fmt::Write as _;

use crate::introspection::RouteInfo;

/// Convert a snake_case handler name to a human-readable description.
///
/// Examples: `create_user` → `"Create user"`, `list_all_posts` → `"List all posts"`.
pub(crate) fn humanize_handler_name(name: &str) -> String {
    let words: Vec<&str> = name.split('_').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return name.to_string();
    }
    let mut result = String::new();
    for (i, word) in words.iter().enumerate() {
        if i == 0 {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                result.extend(first.to_uppercase());
                result.push_str(chars.as_str());
            }
        } else {
            result.push(' ');
            result.push_str(word);
        }
    }
    result
}

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

        let desc = route
            .description
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| humanize_handler_name(&route.handler_name));
        w!(out);
        w!(out, "{desc}");

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
            vec![],
            None::<String>,
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
            vec![],
            None::<String>,
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

    #[test]
    fn test_humanize_handler_name_single_word() {
        assert_eq!(humanize_handler_name("create"), "Create");
    }

    #[test]
    fn test_humanize_handler_name_multi_word() {
        assert_eq!(humanize_handler_name("create_user"), "Create user");
        assert_eq!(humanize_handler_name("list_all_posts"), "List all posts");
    }

    #[test]
    fn test_humanize_handler_name_empty() {
        assert_eq!(humanize_handler_name(""), "");
    }

    #[test]
    fn test_humanize_handler_name_leading_underscores() {
        assert_eq!(humanize_handler_name("_get_user"), "Get user");
    }

    #[test]
    fn test_to_llms_txt_uses_explicit_description() {
        let route = RouteInfo::new(
            "GET",
            "/users",
            "list_users",
            None,
            None,
            None::<String>,
            None,
            vec![],
            vec![],
            Some("Returns all users in the system"),
        );
        let output = to_llms_txt("My API", &[route]);
        assert!(output.contains("Returns all users in the system"));
        assert!(!output.contains("List users"));
    }

    #[test]
    fn test_to_llms_txt_falls_back_to_humanized_name() {
        let route = RouteInfo::new(
            "DELETE",
            "/users/:id",
            "delete_user",
            None,
            None,
            None::<String>,
            None,
            vec![],
            vec![],
            None::<String>,
        );
        let output = to_llms_txt("My API", &[route]);
        assert!(output.contains("Delete user"));
    }
}
