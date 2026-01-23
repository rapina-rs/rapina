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
    Inline(schemars::Schema),
}

/// Reusable components
#[derive(Debug, Clone, Serialize, Default)]
pub struct Components {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, schemars::Schema>,
}
