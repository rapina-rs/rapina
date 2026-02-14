//! OpenAPI 3.0 specification structures

use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: Info,
    pub paths: BTreeMap<String, PathItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<Components>,
}

impl OpenApiSpec {
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            openapi: "3.0.3".to_string(),
            info: Info {
                title: title.into(),
                version: version.into(),
                description: None,
            },
            paths: BTreeMap::new(),
            components: None,
        }
    }
}

/// API metadata
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub title: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Operations available on a single path
#[derive(Debug, Clone, Serialize, Default)]
pub struct PathItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub put: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete: Option<Operation>,
}

/// A single API operation (endpoint)
#[derive(Debug, Clone, Serialize)]
pub struct Operation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "operationId", skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<Parameter>,
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")]
    pub request_body: Option<RequestBody>,
    pub responses: BTreeMap<String, Response>,
}

impl Default for Operation {
    fn default() -> Self {
        let mut responses = BTreeMap::new();
        responses.insert(
            "200".to_string(),
            Response {
                description: "Success".to_string(),
                content: None,
            },
        );
        Self {
            summary: None,
            description: None,
            operation_id: None,
            parameters: Vec::new(),
            request_body: None,
            responses,
        }
    }
}

/// Path, Query, or header parameter
#[derive(Debug, Clone, Serialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: ParameterLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ParameterLocation {
    Path,
    Query,
    Header,
}

/// Request body definition
#[derive(Debug, Clone, Serialize)]
pub struct RequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
    pub content: BTreeMap<String, MediaType>,
}

/// Response definition
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<BTreeMap<String, MediaType>>,
}

/// MediaType with schema
#[derive(Debug, Clone, Serialize)]
pub struct MediaType {
    pub schema: Schema,
}

/// JSON Schema (simplified)
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Schema {
    Ref {
        #[serde(rename = "$ref")]
        reference: String,
    },
    Inline(serde_json::Value),
}

/// Reusable components
#[derive(Debug, Clone, Serialize, Default)]
pub struct Components {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, serde_json::Value>,
}

/// Create the standard Rapina error response schema
fn error_response_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["error", "trace_id"],
        "properties": {
        "error": {
                "type": "object",
                "required": ["code", "message"],
                "properties": {
                    "code": {"type": "string", "description": "Machine-readable error code"},
                    "message": {"type": "string", "description": "Human-readable error message"},
                    "details": {"type": "object", "description": "Optional additional details", "additionalProperties": true}
                }
            }
    }
    })
}

fn error_response_ref() -> Response {
    let mut content = BTreeMap::new();
    content.insert(
        "application/json".to_string(),
        MediaType {
            schema: Schema::Ref {
                reference: "#/components/schemas/ErrorResponse".to_string(),
            },
        },
    );
    Response {
        description: "Error response".to_string(),
        content: Some(content),
    }
}

/// Convert a snake_case handler name to a human-readable summary.
/// e.g., "list_todos" -> "List todos", "get_todo" -> "Get todo"
fn humanize_handler_name(name: &str) -> String {
    let words: Vec<&str> = name.split('_').collect();
    let mut result = String::new();
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            result.push(' ');
        }
        if i == 0 {
            let mut chars = word.chars();
            if let Some(c) = chars.next() {
                result.extend(c.to_uppercase());
                result.push_str(chars.as_str());
            }
        } else {
            result.push_str(word);
        }
    }
    result
}

pub fn build_openapi_spec(
    title: &str,
    version: &str,
    routes: &[crate::introspection::RouteInfo],
) -> OpenApiSpec {
    let mut spec = OpenApiSpec::new(title, version);

    let mut schemas = BTreeMap::new();
    schemas.insert("ErrorResponse".to_string(), error_response_schema());

    spec.components = Some(Components { schemas });

    for route in routes {
        // skip internal rapina routes
        if route.path.starts_with("/__rapina") {
            continue;
        }
        // Extract path parameters (e.g., :id -> id)
        let params: Vec<Parameter> = route
            .path
            .split('/')
            .filter(|s| s.starts_with(':'))
            .map(|s| Parameter {
                name: s.trim_start_matches(':').to_string(),
                location: ParameterLocation::Path,
                description: None,
                required: true,
                schema: None,
            })
            .collect();

        // Convert :param to {param} for OpenAPI format
        let openapi_path = route
            .path
            .split('/')
            .map(|s| {
                if s.starts_with(':') {
                    format!("{{{}}}", s.trim_start_matches(':'))
                } else {
                    s.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("/");

        let success_response = if let Some(schema) = &route.response_schema {
            let mut content = BTreeMap::new();
            content.insert(
                "application/json".to_string(),
                MediaType {
                    schema: Schema::Inline(schema.clone()),
                },
            );
            Response {
                description: "Success".to_string(),
                content: Some(content),
            }
        } else {
            Response {
                description: "Success".to_string(),
                content: None,
            }
        };

        let summary = humanize_handler_name(&route.handler_name);

        let mut operation = Operation {
            summary: Some(summary),
            operation_id: Some(route.handler_name.clone()),
            parameters: params,
            ..Default::default()
        };

        operation
            .responses
            .insert("200".to_string(), success_response);

        // Add documented error responses
        for error in &route.error_responses {
            let status_key = error.status.to_string();
            let error_desc = error.description.to_string();
            operation.responses.entry(status_key).or_insert_with(|| {
                let mut content = BTreeMap::new();
                content.insert(
                    "application/json".to_string(),
                    MediaType {
                        schema: Schema::Ref {
                            reference: "#/components/schemas/ErrorResponse".to_string(),
                        },
                    },
                );
                Response {
                    description: error_desc,
                    content: Some(content),
                }
            });
        }

        // Add default error response for undocumented errors
        operation
            .responses
            .insert("default".to_string(), error_response_ref());

        let path_item = spec.paths.entry(openapi_path).or_default();

        match route.method.to_uppercase().as_str() {
            "GET" => path_item.get = Some(operation),
            "POST" => path_item.post = Some(operation),
            "PUT" => path_item.put = Some(operation),
            "DELETE" => path_item.delete = Some(operation),
            _ => {}
        }
    }

    spec
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorVariant;
    use crate::introspection::RouteInfo;

    #[test]
    fn test_build_openapi_spec_basic() {
        let routes = vec![RouteInfo::new(
            "GET",
            "/users",
            "list_users",
            None,
            Vec::new(),
        )];
        let spec = build_openapi_spec("Test API", "1.0.0", &routes);

        assert_eq!(spec.info.title, "Test API");
        assert_eq!(spec.info.version, "1.0.0");
        assert!(spec.paths.contains_key("/users"));
    }

    #[test]
    fn test_build_openapi_spec_with_error_responses() {
        let errors = vec![
            ErrorVariant {
                status: 404,
                code: "NOT_FOUND",
                description: "User not found",
            },
            ErrorVariant {
                status: 409,
                code: "CONFLICT",
                description: "Email already taken",
            },
        ];
        let routes = vec![RouteInfo::new(
            "GET",
            "/users/:id",
            "get_user",
            None,
            errors,
        )];
        let spec = build_openapi_spec("Test API", "1.0.0", &routes);

        let path = spec.paths.get("/users/{id}").unwrap();
        let get_op = path.get.as_ref().unwrap();

        // Should have 200, 404, 409, and default responses
        assert!(get_op.responses.contains_key("200"));
        assert!(get_op.responses.contains_key("404"));
        assert!(get_op.responses.contains_key("409"));
        assert!(get_op.responses.contains_key("default"));

        // Check descriptions
        assert_eq!(
            get_op.responses.get("404").unwrap().description,
            "User not found"
        );
        assert_eq!(
            get_op.responses.get("409").unwrap().description,
            "Email already taken"
        );
    }

    #[test]
    fn test_build_openapi_spec_skips_internal_routes() {
        let routes = vec![
            RouteInfo::new("GET", "/__rapina/routes", "internal", None, Vec::new()),
            RouteInfo::new("GET", "/users", "list_users", None, Vec::new()),
        ];
        let spec = build_openapi_spec("Test API", "1.0.0", &routes);

        assert!(!spec.paths.contains_key("/__rapina/routes"));
        assert!(spec.paths.contains_key("/users"));
    }
}
