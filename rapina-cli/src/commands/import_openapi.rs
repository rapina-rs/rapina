//! Import OpenAPI 3.0 specs into Rapina project structure.
//!
//! Reads an OpenAPI spec file and generates handler stubs, DTOs, error types,
//! and module structure matching Rapina conventions. The developer fills in
//! business logic; everything else is scaffolded.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use colored::Colorize;
use openapiv3::{
    IntegerFormat, NumberFormat, OpenAPI, Operation, Parameter, ParameterData,
    ParameterSchemaOrContent, ReferenceOr, Schema, SchemaKind, StatusCode, StringFormat, Type,
    VariantOrUnknownOrEmpty,
};

// ---------------------------------------------------------------------------
// Intermediate types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct ParsedEndpoint {
    method: String,
    rapina_path: String,
    handler_name: String,
    summary: Option<String>,
    path_params: Vec<ParamInfo>,
    query_params: Vec<ParamInfo>,
    request_body: Option<String>,
    response_body: Option<String>,
    needs_validation: bool,
    is_public: bool,
}

#[derive(Debug, Clone)]
struct ParamInfo {
    name: String,
    rust_type: String,
}

#[derive(Debug, Clone)]
struct DtoField {
    name: String,
    rust_type: String,
    required: bool,
    validations: Vec<String>,
}

#[derive(Debug, Clone)]
struct DtoDefinition {
    name: String,
    fields: Vec<DtoField>,
    has_validations: bool,
    is_response: bool,
}

#[derive(Debug, Clone)]
struct ModuleDefinition {
    name: String,
    pascal: String,
    endpoints: Vec<ParsedEndpoint>,
    dtos: Vec<DtoDefinition>,
}

// ---------------------------------------------------------------------------
// Utility functions (mirrors add.rs patterns)
// ---------------------------------------------------------------------------

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let mut result = c.to_uppercase().to_string();
                    result.extend(chars);
                    result
                }
                None => String::new(),
            }
        })
        .collect()
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    let bytes = s.as_bytes();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                let prev_lower = bytes[i - 1].is_ascii_lowercase();
                let next_lower = bytes.get(i + 1).is_some_and(|b| b.is_ascii_lowercase());
                // Insert underscore before an uppercase char when:
                // - previous char is lowercase (camelCase boundary): listUsers -> list_users
                // - next char is lowercase and we're in an acronym run: APIKey -> api_key
                if prev_lower || next_lower {
                    result.push('_');
                }
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Best-effort singularization for common English plurals.
use super::codegen::singularize;

// ---------------------------------------------------------------------------
// Path and naming utilities
// ---------------------------------------------------------------------------

/// Convert OpenAPI path params `{id}` to Rapina style `:id`
fn to_rapina_path(openapi_path: &str) -> String {
    openapi_path.replace('{', ":").replace('}', "")
}

/// Derive a handler function name from an operation.
///
/// Priority: operationId -> method + path segments.
fn derive_handler_name(method: &str, path: &str, operation: &Operation) -> String {
    if let Some(ref op_id) = operation.operation_id {
        return to_snake_case(op_id);
    }

    // Fallback: build from method + path segments
    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with('{'))
        .collect();

    if segments.is_empty() {
        return format!("{}_root", method);
    }

    let resource = segments.last().unwrap();
    match method {
        "get" if !path.contains('{') => format!("list_{}", resource),
        "get" => format!("get_{}", singularize(resource)),
        "post" => format!("create_{}", singularize(resource)),
        "put" | "patch" => format!("update_{}", singularize(resource)),
        "delete" => format!("delete_{}", singularize(resource)),
        _ => format!("{}_{}", method, resource),
    }
}

// ---------------------------------------------------------------------------
// Schema resolution and type mapping
// ---------------------------------------------------------------------------

/// Resolve a $ref to a concrete schema. Only handles local component refs.
fn resolve_schema_ref<'a>(spec: &'a OpenAPI, r: &'a ReferenceOr<Schema>) -> Option<&'a Schema> {
    match r {
        ReferenceOr::Item(schema) => Some(schema),
        ReferenceOr::Reference { reference } => {
            let prefix = "#/components/schemas/";
            if let Some(name) = reference.strip_prefix(prefix) {
                spec.components
                    .as_ref()
                    .and_then(|c| c.schemas.get(name))
                    .and_then(|r| resolve_schema_ref(spec, r))
            } else {
                eprintln!(
                    "  {} External $ref not supported: {}",
                    "warn:".yellow(),
                    reference
                );
                None
            }
        }
    }
}

/// Get the schema name from a $ref string.
fn ref_schema_name(reference: &str) -> Option<&str> {
    reference.strip_prefix("#/components/schemas/")
}

/// Map an OpenAPI schema to a Rust type string.
fn map_openapi_type(spec: &OpenAPI, schema_or_ref: &ReferenceOr<Schema>) -> String {
    match schema_or_ref {
        ReferenceOr::Reference { reference } => {
            if let Some(name) = ref_schema_name(reference) {
                to_pascal_case(&to_snake_case(name))
            } else {
                "serde_json::Value".to_string()
            }
        }
        ReferenceOr::Item(schema) => map_schema_type(spec, schema),
    }
}

/// Unbox a `ReferenceOr<Box<Schema>>` into `ReferenceOr<Schema>` for uniform handling.
fn unbox_schema_ref(r: &ReferenceOr<Box<Schema>>) -> ReferenceOr<Schema> {
    match r {
        ReferenceOr::Item(boxed) => ReferenceOr::Item(boxed.as_ref().clone()),
        ReferenceOr::Reference { reference } => ReferenceOr::Reference {
            reference: reference.clone(),
        },
    }
}

fn format_matches_str(format: &VariantOrUnknownOrEmpty<StringFormat>, s: &str) -> bool {
    match format {
        VariantOrUnknownOrEmpty::Item(f) => matches!(
            (f, s),
            (StringFormat::Date, "date")
                | (StringFormat::DateTime, "date-time")
                | (StringFormat::Password, "password")
                | (StringFormat::Byte, "byte")
                | (StringFormat::Binary, "binary")
        ),
        VariantOrUnknownOrEmpty::Unknown(u) => u == s,
        VariantOrUnknownOrEmpty::Empty => false,
    }
}

fn map_schema_type(spec: &OpenAPI, schema: &Schema) -> String {
    match &schema.schema_kind {
        SchemaKind::Type(typ) => match typ {
            Type::String(s) => {
                if format_matches_str(&s.format, "uuid") {
                    "Uuid".to_string()
                } else if format_matches_str(&s.format, "date-time") {
                    "chrono::DateTime<chrono::Utc>".to_string()
                } else if format_matches_str(&s.format, "date") {
                    "chrono::NaiveDate".to_string()
                } else {
                    "String".to_string()
                }
            }
            Type::Integer(i) => match &i.format {
                VariantOrUnknownOrEmpty::Item(IntegerFormat::Int64) => "i64".to_string(),
                _ => "i32".to_string(),
            },
            Type::Number(n) => match &n.format {
                VariantOrUnknownOrEmpty::Item(NumberFormat::Float) => "f32".to_string(),
                _ => "f64".to_string(),
            },
            Type::Boolean(_) => "bool".to_string(),
            Type::Array(arr) => {
                let inner = arr
                    .items
                    .as_ref()
                    .map(|item| {
                        let unboxed = unbox_schema_ref(item);
                        map_openapi_type(spec, &unboxed)
                    })
                    .unwrap_or_else(|| "serde_json::Value".to_string());
                format!("Vec<{}>", inner)
            }
            // Named objects become DTOs via schema_to_dto; here we only
            // produce a type string, so all objects map to Value.
            Type::Object(_) => "serde_json::Value".to_string(),
        },
        SchemaKind::OneOf { .. } | SchemaKind::AnyOf { .. } | SchemaKind::AllOf { .. } => {
            eprintln!(
                "  {} oneOf/anyOf/allOf schemas not fully supported, using serde_json::Value",
                "warn:".yellow(),
            );
            "serde_json::Value".to_string()
        }
        _ => "serde_json::Value".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Validation extraction
// ---------------------------------------------------------------------------

fn extract_validations(schema: &Schema) -> Vec<String> {
    let mut validations = Vec::new();

    if let SchemaKind::Type(typ) = &schema.schema_kind {
        match typ {
            Type::String(s) => {
                if format_matches_str(&s.format, "email") {
                    validations.push("#[validate(email)]".to_string());
                } else if format_matches_str(&s.format, "uri")
                    || format_matches_str(&s.format, "url")
                {
                    validations.push("#[validate(url)]".to_string());
                }
                let has_min = s.min_length.is_some();
                let has_max = s.max_length.is_some();
                if has_min || has_max {
                    let mut parts = Vec::new();
                    if let Some(min) = s.min_length {
                        parts.push(format!("min = {}", min));
                    }
                    if let Some(max) = s.max_length {
                        parts.push(format!("max = {}", max));
                    }
                    validations.push(format!("#[validate(length({}))]", parts.join(", ")));
                }
                if let Some(ref pattern) = s.pattern {
                    validations.push(format!("#[validate(regex(path = \"{}\"))]", pattern));
                }
            }
            Type::Integer(i) => {
                let has_min = i.minimum.is_some();
                let has_max = i.maximum.is_some();
                if has_min || has_max {
                    let mut parts = Vec::new();
                    if let Some(min) = i.minimum {
                        parts.push(format!("min = {}", min));
                    }
                    if let Some(max) = i.maximum {
                        parts.push(format!("max = {}", max));
                    }
                    validations.push(format!("#[validate(range({}))]", parts.join(", ")));
                }
            }
            Type::Number(n) => {
                let has_min = n.minimum.is_some();
                let has_max = n.maximum.is_some();
                if has_min || has_max {
                    let mut parts = Vec::new();
                    if let Some(min) = n.minimum {
                        parts.push(format!("min = {:.1}", min));
                    }
                    if let Some(max) = n.maximum {
                        parts.push(format!("max = {:.1}", max));
                    }
                    validations.push(format!("#[validate(range({}))]", parts.join(", ")));
                }
            }
            _ => {}
        }
    }

    validations
}

// ---------------------------------------------------------------------------
// DTO extraction from schemas
// ---------------------------------------------------------------------------

fn schema_to_dto(
    spec: &OpenAPI,
    name: &str,
    schema: &Schema,
    is_response: bool,
) -> Option<DtoDefinition> {
    let SchemaKind::Type(Type::Object(obj)) = &schema.schema_kind else {
        return None;
    };

    let required_set: std::collections::HashSet<&str> =
        obj.required.iter().map(|s| s.as_str()).collect();

    let mut fields = Vec::new();
    let mut has_validations = false;

    for (field_name, field_schema_ref) in &obj.properties {
        let unboxed = unbox_schema_ref(field_schema_ref);
        let rust_type = map_openapi_type(spec, &unboxed);
        let required = required_set.contains(field_name.as_str());

        let validations = if let Some(field_schema) = resolve_schema_ref(spec, &unboxed) {
            let v = extract_validations(field_schema);
            if !v.is_empty() {
                has_validations = true;
            }
            v
        } else {
            Vec::new()
        };

        fields.push(DtoField {
            name: to_snake_case(field_name),
            rust_type,
            required,
            validations,
        });
    }

    // Sort fields for stable output
    fields.sort_by(|a, b| a.name.cmp(&b.name));

    Some(DtoDefinition {
        name: name.to_string(),
        fields,
        has_validations,
        is_response,
    })
}

// ---------------------------------------------------------------------------
// Spec parsing and grouping
// ---------------------------------------------------------------------------

fn parse_spec_file(path: &str) -> Result<OpenAPI, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read spec file '{}': {}", path, e))?;

    // Try JSON first, then YAML
    if let Ok(spec) = serde_json::from_str::<OpenAPI>(&content) {
        return Ok(spec);
    }

    serde_yaml_ng::from_str::<OpenAPI>(&content)
        .map_err(|e| format!("Failed to parse spec file '{}': {}", path, e))
}

fn extract_param_data(param: &ReferenceOr<Parameter>) -> Option<&ParameterData> {
    match param {
        ReferenceOr::Item(p) => match p {
            Parameter::Query { parameter_data, .. } => Some(parameter_data),
            Parameter::Path { parameter_data, .. } => Some(parameter_data),
            _ => None,
        },
        ReferenceOr::Reference { .. } => None,
    }
}

fn param_rust_type(spec: &OpenAPI, data: &ParameterData) -> String {
    match &data.format {
        ParameterSchemaOrContent::Schema(schema_ref) => map_openapi_type(spec, schema_ref),
        _ => "String".to_string(),
    }
}

fn is_path_param(param: &ReferenceOr<Parameter>) -> bool {
    matches!(param, ReferenceOr::Item(Parameter::Path { .. }))
}

fn is_query_param(param: &ReferenceOr<Parameter>) -> bool {
    matches!(param, ReferenceOr::Item(Parameter::Query { .. }))
}

/// Check if an operation requires authentication (has security requirements).
fn operation_is_public(spec: &OpenAPI, operation: &Operation) -> bool {
    // If operation explicitly sets security to empty array, it's public
    if let Some(ref sec) = operation.security {
        return sec.is_empty();
    }
    // If no operation-level security and no global security, it's public
    spec.security.as_ref().is_none_or(|s| s.is_empty())
}

/// Extract the request body DTO name from an operation.
fn extract_request_body_name(
    spec: &OpenAPI,
    operation: &Operation,
    handler_name: &str,
    dtos: &mut BTreeMap<String, DtoDefinition>,
) -> Option<String> {
    let body_ref = operation.request_body.as_ref()?;

    let body = match body_ref {
        ReferenceOr::Item(b) => b,
        ReferenceOr::Reference { reference } => {
            // Resolve request body $ref
            let prefix = "#/components/requestBodies/";
            if let Some(_name) = reference.strip_prefix(prefix) {
                eprintln!(
                    "  {} Request body $ref not yet supported: {}",
                    "warn:".yellow(),
                    reference
                );
            }
            return None;
        }
    };

    let json_content = body.content.get("application/json")?;
    let schema_ref = json_content.schema.as_ref()?;

    // If it's a $ref to a named schema, use that name
    if let ReferenceOr::Reference { reference } = schema_ref {
        if let Some(name) = ref_schema_name(reference) {
            let dto_name = to_pascal_case(&to_snake_case(name));
            // Ensure we have the DTO definition
            if !dtos.contains_key(&dto_name) {
                if let Some(schema) = resolve_schema_ref(spec, schema_ref) {
                    if let Some(dto) = schema_to_dto(spec, &dto_name, schema, false) {
                        dtos.insert(dto_name.clone(), dto);
                    }
                }
            }
            return Some(dto_name);
        }
    }

    // Inline schema — generate a name from the handler
    let dto_name = format!("{}Request", to_pascal_case(handler_name));
    if let Some(schema) = resolve_schema_ref(spec, schema_ref) {
        if let Some(dto) = schema_to_dto(spec, &dto_name, schema, false) {
            dtos.insert(dto_name.clone(), dto);
            return Some(dto_name);
        }
    }
    None
}

/// Extract the primary success response DTO name.
fn extract_response_body_name(
    spec: &OpenAPI,
    operation: &Operation,
    handler_name: &str,
    dtos: &mut BTreeMap<String, DtoDefinition>,
) -> Option<String> {
    // Look for 200 or 201 response
    let response_ref = operation
        .responses
        .responses
        .get(&StatusCode::Code(200))
        .or_else(|| operation.responses.responses.get(&StatusCode::Code(201)))?;

    let response = match response_ref {
        ReferenceOr::Item(r) => r,
        ReferenceOr::Reference { .. } => return None,
    };

    let json_content = response.content.get("application/json")?;
    let schema_ref = json_content.schema.as_ref()?;

    // Check if it's an array response
    if let ReferenceOr::Item(schema) = schema_ref {
        if let SchemaKind::Type(Type::Array(arr)) = &schema.schema_kind {
            if let Some(ref items) = arr.items {
                let unboxed = unbox_schema_ref(items);
                let inner = map_openapi_type(spec, &unboxed);
                return Some(format!("Vec<{}>", inner));
            }
        }
    }

    if let ReferenceOr::Reference { reference } = schema_ref {
        if let Some(name) = ref_schema_name(reference) {
            let dto_name = to_pascal_case(&to_snake_case(name));
            if !dtos.contains_key(&dto_name) {
                if let Some(schema) = resolve_schema_ref(spec, schema_ref) {
                    if let Some(dto) = schema_to_dto(spec, &dto_name, schema, true) {
                        dtos.insert(dto_name.clone(), dto);
                    }
                }
            }
            return Some(dto_name);
        }
    }

    let dto_name = format!("{}Response", to_pascal_case(handler_name));
    if let Some(schema) = resolve_schema_ref(spec, schema_ref) {
        if let Some(dto) = schema_to_dto(spec, &dto_name, schema, true) {
            dtos.insert(dto_name.clone(), dto);
            return Some(dto_name);
        }
    }
    None
}

fn parse_operation(
    spec: &OpenAPI,
    method: &str,
    path: &str,
    operation: &Operation,
    path_item_params: &[ReferenceOr<Parameter>],
    dtos: &mut BTreeMap<String, DtoDefinition>,
) -> ParsedEndpoint {
    let handler_name = derive_handler_name(method, path, operation);
    let rapina_path = to_rapina_path(path);

    // Merge path-level and operation-level parameters
    let all_params: Vec<&ReferenceOr<Parameter>> = path_item_params
        .iter()
        .chain(operation.parameters.iter())
        .collect();

    let path_params: Vec<ParamInfo> = all_params
        .iter()
        .filter(|p| is_path_param(p))
        .filter_map(|p| {
            extract_param_data(p).map(|data| ParamInfo {
                name: data.name.clone(),
                rust_type: param_rust_type(spec, data),
            })
        })
        .collect();

    let query_params: Vec<ParamInfo> = all_params
        .iter()
        .filter(|p| is_query_param(p))
        .filter_map(|p| {
            extract_param_data(p).map(|data| ParamInfo {
                name: data.name.clone(),
                rust_type: param_rust_type(spec, data),
            })
        })
        .collect();

    let request_body = extract_request_body_name(spec, operation, &handler_name, dtos);
    let response_body = extract_response_body_name(spec, operation, &handler_name, dtos);

    // Check if request body DTO has validations
    let needs_validation = request_body
        .as_ref()
        .and_then(|name| dtos.get(name))
        .is_some_and(|dto| dto.has_validations);

    let is_public = operation_is_public(spec, operation);

    ParsedEndpoint {
        method: method.to_string(),
        rapina_path,
        handler_name,
        summary: operation.summary.clone(),
        path_params,
        query_params,
        request_body,
        response_body,
        needs_validation,
        is_public,
    }
}

/// Group endpoints by tag or first path segment into modules.
fn group_by_module(
    spec: &OpenAPI,
    tag_filter: Option<&[String]>,
) -> Result<(Vec<ModuleDefinition>, Vec<String>), String> {
    let mut modules: BTreeMap<String, (Vec<ParsedEndpoint>, BTreeMap<String, DtoDefinition>)> =
        BTreeMap::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut seen_handlers: BTreeMap<String, u32> = BTreeMap::new();

    for (path, path_ref) in &spec.paths.paths {
        let path_item = match path_ref {
            ReferenceOr::Item(item) => item,
            ReferenceOr::Reference { reference } => {
                warnings.push(format!("Skipping external path $ref: {}", reference));
                continue;
            }
        };

        let operations: Vec<(&str, &Operation)> = [
            ("get", &path_item.get),
            ("post", &path_item.post),
            ("put", &path_item.put),
            ("delete", &path_item.delete),
            ("patch", &path_item.patch),
        ]
        .iter()
        .filter_map(|(method, op)| op.as_ref().map(|o| (*method, o)))
        .collect();

        for (method, operation) in operations {
            // Determine module name from first tag or first path segment
            let module_name = operation
                .tags
                .first()
                .map(|t| to_snake_case(t))
                .unwrap_or_else(|| {
                    path.split('/')
                        .find(|s| !s.is_empty() && !s.starts_with('{'))
                        .unwrap_or("root")
                        .to_lowercase()
                });

            // Apply tag filter
            if let Some(filter) = tag_filter {
                let has_matching_tag = operation
                    .tags
                    .iter()
                    .any(|t| filter.iter().any(|f| f.eq_ignore_ascii_case(t)));
                if !has_matching_tag {
                    continue;
                }
            }

            let entry = modules
                .entry(module_name)
                .or_insert_with(|| (Vec::new(), BTreeMap::new()));

            let mut endpoint = parse_operation(
                spec,
                method,
                path,
                operation,
                &path_item.parameters,
                &mut entry.1,
            );

            // Handle PATCH -> PUT mapping
            if method == "patch" {
                endpoint.method = "put".to_string();
                endpoint.summary = Some(format!(
                    "{} (mapped from PATCH)",
                    endpoint.summary.as_deref().unwrap_or("Update")
                ));
            }

            // Handle duplicate handler names
            let count = seen_handlers
                .entry(endpoint.handler_name.clone())
                .or_insert(0);
            *count += 1;
            if *count > 1 {
                endpoint.handler_name = format!("{}_{}", endpoint.handler_name, count);
            }

            entry.0.push(endpoint);
        }
    }

    // Generate query DTOs for endpoints that need them
    for (endpoints, dtos) in modules.values_mut() {
        for endpoint in endpoints.iter() {
            if !endpoint.query_params.is_empty() {
                let dto_name = format!("{}Query", to_pascal_case(&endpoint.handler_name));
                let fields: Vec<DtoField> = endpoint
                    .query_params
                    .iter()
                    .map(|p| DtoField {
                        name: to_snake_case(&p.name),
                        rust_type: p.rust_type.clone(),
                        required: false, // query params are always optional in the DTO
                        validations: Vec::new(),
                    })
                    .collect();
                dtos.insert(
                    dto_name.clone(),
                    DtoDefinition {
                        name: dto_name,
                        fields,
                        has_validations: false,
                        is_response: false,
                    },
                );
            }
        }
    }

    let result: Vec<ModuleDefinition> = modules
        .into_iter()
        .map(|(name, (endpoints, dtos))| {
            let pascal = to_pascal_case(&singularize(&name));
            ModuleDefinition {
                name,
                pascal,
                endpoints,
                dtos: dtos.into_values().collect(),
            }
        })
        .collect();

    Ok((result, warnings))
}

// ---------------------------------------------------------------------------
// Code generation
// ---------------------------------------------------------------------------

fn generate_handler_stubs(module: &ModuleDefinition) -> String {
    let mut out = String::new();

    // Imports
    out.push_str("use rapina::prelude::*;\n");

    let has_validation = module.endpoints.iter().any(|e| e.needs_validation);

    if has_validation {
        out.push_str("use rapina::validation::Validated;\n");
    }

    out.push('\n');
    out.push_str("use super::dto::*;\n");
    out.push_str(&format!("use super::error::{}Error;\n", module.pascal));
    out.push('\n');

    for endpoint in &module.endpoints {
        // Doc comment
        if let Some(ref summary) = endpoint.summary {
            out.push_str(&format!("/// {}\n", summary));
        }

        // #[public] attribute
        if endpoint.is_public {
            out.push_str("#[public]\n");
        }

        // Route macro
        let macro_name = match endpoint.method.as_str() {
            "get" => "get",
            "post" => "post",
            "put" => "put",
            "delete" => "delete",
            _ => "get",
        };
        out.push_str(&format!(
            "#[{}(\"{}\")]\n",
            macro_name, endpoint.rapina_path
        ));

        // Error type
        out.push_str(&format!("#[errors({}Error)]\n", module.pascal));

        // Function signature
        let mut params = Vec::new();

        if !endpoint.path_params.is_empty() {
            if endpoint.path_params.len() == 1 {
                let p = &endpoint.path_params[0];
                params.push(format!("{}: Path<{}>", p.name, p.rust_type));
            } else {
                let types: Vec<String> = endpoint
                    .path_params
                    .iter()
                    .map(|p| p.rust_type.clone())
                    .collect();
                params.push(format!("path: Path<({})>", types.join(", ")));
            }
        }

        if !endpoint.query_params.is_empty() {
            let query_dto = format!("{}Query", to_pascal_case(&endpoint.handler_name));
            params.push(format!("query: Query<{}>", query_dto));
        }

        if let Some(ref body_type) = endpoint.request_body {
            if endpoint.needs_validation {
                params.push(format!("body: Validated<Json<{}>>", body_type));
            } else {
                params.push(format!("body: Json<{}>", body_type));
            }
        }

        let return_type = endpoint
            .response_body
            .as_ref()
            .map(|t| format!("Json<{}>", t))
            .unwrap_or_else(|| "Json<serde_json::Value>".to_string());

        out.push_str(&format!(
            "pub async fn {}({}) -> Result<{}, {}Error> {{\n",
            endpoint.handler_name,
            params.join(", "),
            return_type,
            module.pascal,
        ));
        out.push_str(&format!(
            "    todo!(\"implement {}\")\n",
            endpoint.handler_name
        ));
        out.push_str("}\n\n");
    }

    out
}

fn generate_dtos(module: &ModuleDefinition) -> String {
    let mut out = String::new();

    out.push_str("use rapina::schemars::{self, JsonSchema};\n");
    out.push_str("use serde::{Deserialize, Serialize};\n");

    let has_validation = module.dtos.iter().any(|d| d.has_validations);
    if has_validation {
        out.push_str("use validator::Validate;\n");
    }
    out.push('\n');

    for dto in &module.dtos {
        let mut derives = Vec::new();
        if dto.is_response {
            derives.push("Serialize");
        } else {
            derives.push("Deserialize");
        }
        derives.push("JsonSchema");
        if dto.has_validations {
            derives.push("Validate");
        }

        out.push_str(&format!("#[derive({})]\n", derives.join(", ")));
        out.push_str(&format!("pub struct {} {{\n", dto.name));

        for field in &dto.fields {
            for v in &field.validations {
                out.push_str(&format!("    {}\n", v));
            }

            let ty = if field.required {
                field.rust_type.clone()
            } else {
                format!("Option<{}>", field.rust_type)
            };

            out.push_str(&format!("    pub {}: {},\n", field.name, ty));
        }

        out.push_str("}\n\n");
    }

    out
}

fn generate_error_stub(pascal: &str) -> String {
    format!(
        r#"use rapina::prelude::*;

pub enum {pascal}Error {{
    NotFound(String),
    Internal(String),
}}

impl IntoApiError for {pascal}Error {{
    fn into_api_error(self) -> Error {{
        match self {{
            {pascal}Error::NotFound(msg) => Error::not_found(msg),
            {pascal}Error::Internal(msg) => Error::internal(msg),
        }}
    }}
}}

impl DocumentedError for {pascal}Error {{
    fn error_variants() -> Vec<ErrorVariant> {{
        vec![
            ErrorVariant {{
                status: 404,
                code: "NOT_FOUND",
                description: "{pascal} not found",
            }},
            ErrorVariant {{
                status: 500,
                code: "INTERNAL_ERROR",
                description: "Internal server error",
            }},
        ]
    }}
}}
"#,
        pascal = pascal,
    )
}

fn generate_mod_rs() -> String {
    "pub mod dto;\npub mod error;\npub mod handlers;\n".to_string()
}

// ---------------------------------------------------------------------------
// File I/O
// ---------------------------------------------------------------------------

fn write_module(module: &ModuleDefinition) -> Result<(), String> {
    let module_dir = Path::new("src").join(&module.name);

    if module_dir.exists() {
        return Err(format!(
            "Directory 'src/{}/' already exists. Remove it first or choose different tags.",
            module.name
        ));
    }

    fs::create_dir_all(&module_dir)
        .map_err(|e| format!("Failed to create directory src/{}/: {}", module.name, e))?;

    fs::write(module_dir.join("mod.rs"), generate_mod_rs())
        .map_err(|e| format!("Failed to write mod.rs: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/mod.rs", module.name).cyan()
    );

    fs::write(
        module_dir.join("handlers.rs"),
        generate_handler_stubs(module),
    )
    .map_err(|e| format!("Failed to write handlers.rs: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/handlers.rs", module.name).cyan()
    );

    fs::write(module_dir.join("dto.rs"), generate_dtos(module))
        .map_err(|e| format!("Failed to write dto.rs: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/dto.rs", module.name).cyan()
    );

    fs::write(
        module_dir.join("error.rs"),
        generate_error_stub(&module.pascal),
    )
    .map_err(|e| format!("Failed to write error.rs: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/error.rs", module.name).cyan()
    );

    Ok(())
}

fn dry_run_report(modules: &[ModuleDefinition], warnings: &[String]) {
    println!();
    println!("  {} (no files written)", "--dry-run".bright_yellow());
    println!();

    for warning in warnings {
        println!("  {} {}", "warn:".yellow(), warning);
    }

    for module in modules {
        println!(
            "  {} module {} ({} endpoints, {} DTOs)",
            "→".cyan(),
            module.name.bold(),
            module.endpoints.len(),
            module.dtos.len(),
        );

        println!("    Files:");
        println!("      src/{}/mod.rs", module.name);
        println!("      src/{}/handlers.rs", module.name);
        println!("      src/{}/dto.rs", module.name);
        println!("      src/{}/error.rs", module.name);

        println!("    Handlers:");
        for ep in &module.endpoints {
            let public_marker = if ep.is_public { " [public]" } else { "" };
            println!(
                "      {} {} {}{}",
                ep.method.to_uppercase().bright_cyan(),
                ep.rapina_path,
                ep.handler_name.dimmed(),
                public_marker.dimmed(),
            );
        }

        println!("    DTOs:");
        for dto in &module.dtos {
            let kind = if dto.is_response {
                "response"
            } else {
                "request"
            };
            let val = if dto.has_validations {
                " +validate"
            } else {
                ""
            };
            println!(
                "      {} ({}{})",
                dto.name.bright_cyan(),
                kind,
                val.dimmed(),
            );
        }
        println!();
    }
}

fn print_next_steps(modules: &[ModuleDefinition]) {
    println!();
    println!("  {}:", "Next steps".bright_yellow());
    println!();
    println!("  1. Add module declarations to {}:", "src/main.rs".cyan());
    println!();
    for module in modules {
        println!("     mod {};", module.name);
    }
    println!();
    println!(
        "  2. Register handler functions in your {}",
        "Router".cyan()
    );
    println!();
    println!(
        "  3. Fill in the {} bodies in each handler",
        "todo!()".cyan()
    );
    println!();
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn openapi(file: &str, tag_filter: Option<&[String]>, dry_run: bool) -> Result<(), String> {
    if !dry_run {
        super::verify_rapina_project()?;
    }

    println!();
    println!(
        "  {} {}",
        "Importing OpenAPI spec:".bright_cyan(),
        file.bold()
    );

    let spec = parse_spec_file(file)?;

    let title = spec.info.title.as_str();
    let version = spec.info.version.as_str();
    println!("  {} {} v{}", "Spec:".dimmed(), title, version);

    let (modules, warnings) = group_by_module(&spec, tag_filter)?;

    if modules.is_empty() {
        return Err("No endpoints found in spec (check --tags filter if used).".to_string());
    }

    for w in &warnings {
        println!("  {} {}", "warn:".yellow(), w);
    }

    if dry_run {
        dry_run_report(&modules, &warnings);
        return Ok(());
    }

    println!();
    for module in &modules {
        println!(
            "  {} Generating module {}...",
            "→".cyan(),
            module.name.bold()
        );
        write_module(module)?;
    }

    let total_endpoints: usize = modules.iter().map(|m| m.endpoints.len()).sum();
    let total_dtos: usize = modules.iter().map(|m| m.dtos.len()).sum();

    println!();
    println!(
        "  {} Imported {} endpoints across {} modules ({} DTOs)",
        "✓".green().bold(),
        total_endpoints,
        modules.len(),
        total_dtos,
    );

    print_next_steps(&modules);

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_rapina_path() {
        assert_eq!(to_rapina_path("/users/{id}"), "/users/:id");
        assert_eq!(
            to_rapina_path("/users/{user_id}/posts/{post_id}"),
            "/users/:user_id/posts/:post_id"
        );
        assert_eq!(to_rapina_path("/users"), "/users");
        assert_eq!(to_rapina_path("/"), "/");
    }

    #[test]
    fn test_to_snake_case() {
        assert_eq!(to_snake_case("listUsers"), "list_users");
        assert_eq!(to_snake_case("getUserById"), "get_user_by_id");
        assert_eq!(to_snake_case("already_snake"), "already_snake");
        assert_eq!(to_snake_case("HTML"), "html");
        assert_eq!(to_snake_case("APIKey"), "api_key");
        assert_eq!(to_snake_case("HTTPSUrl"), "https_url");
        assert_eq!(to_snake_case("CreateUser"), "create_user");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("user"), "User");
        assert_eq!(to_pascal_case("blog_post"), "BlogPost");
    }

    #[test]
    fn test_singularize() {
        assert_eq!(singularize("users"), "user");
        assert_eq!(singularize("posts"), "post");
        assert_eq!(singularize("categories"), "category");
        assert_eq!(singularize("boxes"), "box");
        assert_eq!(singularize("class"), "class"); // ends in 'ss'
        assert_eq!(singularize("buses"), "bus");
    }

    fn make_test_spec() -> OpenAPI {
        let json = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "Test API", "version": "1.0.0" },
            "paths": {
                "/users": {
                    "get": {
                        "tags": ["Users"],
                        "operationId": "listUsers",
                        "summary": "List all users",
                        "parameters": [
                            {
                                "name": "page",
                                "in": "query",
                                "schema": { "type": "integer", "format": "int32" }
                            }
                        ],
                        "responses": {
                            "200": {
                                "description": "OK",
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "type": "array",
                                            "items": { "$ref": "#/components/schemas/User" }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    "post": {
                        "tags": ["Users"],
                        "operationId": "createUser",
                        "summary": "Create a user",
                        "requestBody": {
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/CreateUser" }
                                }
                            }
                        },
                        "responses": {
                            "201": {
                                "description": "Created",
                                "content": {
                                    "application/json": {
                                        "schema": { "$ref": "#/components/schemas/User" }
                                    }
                                }
                            }
                        }
                    }
                },
                "/users/{id}": {
                    "get": {
                        "tags": ["Users"],
                        "operationId": "getUser",
                        "summary": "Get a user by ID",
                        "parameters": [
                            {
                                "name": "id",
                                "in": "path",
                                "required": true,
                                "schema": { "type": "integer", "format": "int32" }
                            }
                        ],
                        "responses": {
                            "200": {
                                "description": "OK",
                                "content": {
                                    "application/json": {
                                        "schema": { "$ref": "#/components/schemas/User" }
                                    }
                                }
                            }
                        }
                    },
                    "delete": {
                        "tags": ["Users"],
                        "operationId": "deleteUser",
                        "summary": "Delete a user",
                        "parameters": [
                            {
                                "name": "id",
                                "in": "path",
                                "required": true,
                                "schema": { "type": "integer", "format": "int32" }
                            }
                        ],
                        "responses": {
                            "200": { "description": "Deleted" }
                        }
                    }
                },
                "/posts": {
                    "get": {
                        "tags": ["Posts"],
                        "summary": "List posts",
                        "security": [],
                        "responses": {
                            "200": { "description": "OK" }
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "User": {
                        "type": "object",
                        "required": ["id", "name", "email"],
                        "properties": {
                            "id": { "type": "integer", "format": "int32" },
                            "name": { "type": "string" },
                            "email": { "type": "string", "format": "email" }
                        }
                    },
                    "CreateUser": {
                        "type": "object",
                        "required": ["name", "email"],
                        "properties": {
                            "name": { "type": "string", "minLength": 1, "maxLength": 100 },
                            "email": { "type": "string", "format": "email" }
                        }
                    }
                }
            },
            "security": [{ "bearerAuth": [] }]
        });
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn test_derive_handler_name_from_operation_id() {
        let spec = make_test_spec();
        let path_item = match &spec.paths.paths["/users"] {
            ReferenceOr::Item(item) => item,
            _ => panic!("expected item"),
        };
        let op = path_item.get.as_ref().unwrap();
        assert_eq!(derive_handler_name("get", "/users", op), "list_users");
    }

    #[test]
    fn test_derive_handler_name_fallback() {
        let op = Operation::default();
        assert_eq!(derive_handler_name("get", "/users", &op), "list_users");
        assert_eq!(derive_handler_name("get", "/users/{id}", &op), "get_user");
        assert_eq!(derive_handler_name("post", "/users", &op), "create_user");
        assert_eq!(
            derive_handler_name("put", "/users/{id}", &op),
            "update_user"
        );
        assert_eq!(
            derive_handler_name("delete", "/users/{id}", &op),
            "delete_user"
        );
    }

    #[test]
    fn test_map_openapi_type_primitives() {
        let spec = make_test_spec();

        // String
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::String(Default::default())),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "String");

        // Integer
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::Integer(Default::default())),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "i32");

        // Boolean
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::Boolean(Default::default())),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "bool");
    }

    #[test]
    fn test_map_openapi_type_ref() {
        let spec = make_test_spec();
        let schema = ReferenceOr::Reference {
            reference: "#/components/schemas/User".to_string(),
        };
        assert_eq!(map_openapi_type(&spec, &schema), "User");
    }

    #[test]
    fn test_extract_validations_email() {
        let schema = Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::String(openapiv3::StringType {
                format: VariantOrUnknownOrEmpty::Unknown("email".to_string()),
                ..Default::default()
            })),
        };
        let validations = extract_validations(&schema);
        assert!(validations.contains(&"#[validate(email)]".to_string()));
    }

    #[test]
    fn test_extract_validations_length() {
        let schema = Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::String(openapiv3::StringType {
                min_length: Some(1),
                max_length: Some(100),
                ..Default::default()
            })),
        };
        let validations = extract_validations(&schema);
        assert!(
            validations
                .iter()
                .any(|v| v.contains("length") && v.contains("min = 1") && v.contains("max = 100"))
        );
    }

    #[test]
    fn test_extract_validations_range() {
        let schema = Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::Integer(openapiv3::IntegerType {
                minimum: Some(0),
                maximum: Some(100),
                ..Default::default()
            })),
        };
        let validations = extract_validations(&schema);
        assert!(
            validations
                .iter()
                .any(|v| v.contains("range") && v.contains("min = 0") && v.contains("max = 100"))
        );
    }

    #[test]
    fn test_group_by_tag() {
        let spec = make_test_spec();
        let (modules, _warnings) = group_by_module(&spec, None).unwrap();

        let user_module = modules.iter().find(|m| m.name == "users").unwrap();
        assert_eq!(user_module.endpoints.len(), 4); // list, create, get, delete
        assert!(!user_module.dtos.is_empty());

        let post_module = modules.iter().find(|m| m.name == "posts").unwrap();
        assert_eq!(post_module.endpoints.len(), 1);
        assert!(post_module.endpoints[0].is_public);
    }

    #[test]
    fn test_group_with_tag_filter() {
        let spec = make_test_spec();
        let tags = vec!["Users".to_string()];
        let (modules, _) = group_by_module(&spec, Some(&tags)).unwrap();

        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "users");
    }

    #[test]
    fn test_group_by_path_segment() {
        // Spec with no tags — should group by first path segment
        let json = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "Test", "version": "1.0.0" },
            "paths": {
                "/items": {
                    "get": {
                        "responses": { "200": { "description": "OK" } }
                    }
                },
                "/items/{id}": {
                    "get": {
                        "parameters": [{
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "integer" }
                        }],
                        "responses": { "200": { "description": "OK" } }
                    }
                }
            }
        });
        let spec: OpenAPI = serde_json::from_value(json).unwrap();
        let (modules, _) = group_by_module(&spec, None).unwrap();

        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name, "items");
        assert_eq!(modules[0].endpoints.len(), 2);
    }

    #[test]
    fn test_generate_handler_stubs() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let user_module = modules.iter().find(|m| m.name == "users").unwrap();

        let output = generate_handler_stubs(user_module);

        assert!(output.contains("pub async fn list_users"));
        assert!(output.contains("pub async fn create_user"));
        assert!(output.contains("pub async fn get_user"));
        assert!(output.contains("pub async fn delete_user"));
        assert!(output.contains("#[get(\"/users\")]"));
        assert!(output.contains("#[post(\"/users\")]"));
        assert!(output.contains("#[delete(\"/users/:id\")]"));
        assert!(output.contains("todo!(\"implement"));
    }

    #[test]
    fn test_generate_handler_public_attribute() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let post_module = modules.iter().find(|m| m.name == "posts").unwrap();

        let output = generate_handler_stubs(post_module);
        assert!(output.contains("#[public]"));
    }

    #[test]
    fn test_generate_dtos() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let user_module = modules.iter().find(|m| m.name == "users").unwrap();

        let output = generate_dtos(user_module);

        assert!(output.contains("pub struct User"));
        assert!(output.contains("pub struct CreateUser"));
        assert!(output.contains("Serialize"));
        assert!(output.contains("Deserialize"));
        assert!(output.contains("JsonSchema"));
    }

    #[test]
    fn test_generate_dtos_with_validation() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let user_module = modules.iter().find(|m| m.name == "users").unwrap();

        let output = generate_dtos(user_module);

        // CreateUser has email format and length constraints
        assert!(output.contains("Validate"));
        assert!(output.contains("#[validate(email)]"));
        assert!(output.contains("#[validate(length("));
    }

    #[test]
    fn test_generate_error_stub() {
        let output = generate_error_stub("User");
        assert!(output.contains("pub enum UserError"));
        assert!(output.contains("impl IntoApiError for UserError"));
        assert!(output.contains("impl DocumentedError for UserError"));
        assert!(output.contains("\"User not found\""));
    }

    #[test]
    fn test_dto_required_vs_optional() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let user_module = modules.iter().find(|m| m.name == "users").unwrap();

        // User schema has id, name, email as required
        let user_dto = user_module.dtos.iter().find(|d| d.name == "User").unwrap();
        for field in &user_dto.fields {
            assert!(field.required, "field {} should be required", field.name);
        }
    }

    #[test]
    fn test_handler_uses_validated_extractor_when_dto_has_validations() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let user_module = modules.iter().find(|m| m.name == "users").unwrap();
        let output = generate_handler_stubs(user_module);

        // create_user has CreateUser body which has validations (email, length)
        // so it must use Validated<Json<CreateUser>>, not plain Json<CreateUser>
        assert!(
            output.contains("Validated<Json<CreateUser>>"),
            "create_user should use Validated extractor, got:\n{}",
            output
        );

        // get_user has no body, should not mention Validated at function level
        // (the import is module-wide, that's fine)
        let get_user_line = output
            .lines()
            .find(|l| l.contains("fn get_user"))
            .expect("get_user handler not found");
        assert!(
            !get_user_line.contains("Validated"),
            "get_user should not use Validated extractor"
        );
    }

    #[test]
    fn test_handler_path_extractor() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let user_module = modules.iter().find(|m| m.name == "users").unwrap();
        let output = generate_handler_stubs(user_module);

        // get_user takes a path param {id} as i32
        let get_user_line = output.lines().find(|l| l.contains("fn get_user")).unwrap();
        assert!(
            get_user_line.contains("id: Path<i32>"),
            "get_user should have Path<i32> extractor, got: {}",
            get_user_line
        );

        // list_users has no path param
        let list_line = output
            .lines()
            .find(|l| l.contains("fn list_users"))
            .unwrap();
        assert!(
            !list_line.contains("Path<"),
            "list_users should not have Path extractor"
        );
    }

    #[test]
    fn test_handler_query_extractor() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let user_module = modules.iter().find(|m| m.name == "users").unwrap();
        let output = generate_handler_stubs(user_module);

        // list_users has a query param (page)
        let list_line = output
            .lines()
            .find(|l| l.contains("fn list_users"))
            .unwrap();
        assert!(
            list_line.contains("Query<ListUsersQuery>"),
            "list_users should have Query extractor, got: {}",
            list_line
        );

        // Verify the query DTO was generated
        let dto_output = generate_dtos(user_module);
        assert!(
            dto_output.contains("pub struct ListUsersQuery"),
            "ListUsersQuery DTO should be generated"
        );
    }

    #[test]
    fn test_duplicate_handler_name_deduplication() {
        // Two GET endpoints with same tag that would both derive "get_item"
        let json = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "Test", "version": "1.0.0" },
            "paths": {
                "/items/{id}": {
                    "get": {
                        "tags": ["Items"],
                        "parameters": [{
                            "name": "id", "in": "path", "required": true,
                            "schema": { "type": "integer" }
                        }],
                        "responses": { "200": { "description": "OK" } }
                    }
                },
                "/v2/items/{id}": {
                    "get": {
                        "tags": ["Items"],
                        "parameters": [{
                            "name": "id", "in": "path", "required": true,
                            "schema": { "type": "integer" }
                        }],
                        "responses": { "200": { "description": "OK" } }
                    }
                }
            }
        });
        let spec: OpenAPI = serde_json::from_value(json).unwrap();
        let (modules, _) = group_by_module(&spec, None).unwrap();

        let items = modules.iter().find(|m| m.name == "items").unwrap();
        assert_eq!(items.endpoints.len(), 2);
        let names: Vec<&str> = items
            .endpoints
            .iter()
            .map(|e| e.handler_name.as_str())
            .collect();
        // Should have deduplicated: one is "get_item", other is "get_item_2"
        assert_ne!(names[0], names[1], "handler names should be unique");
        assert!(
            names.iter().any(|n| n.contains("_2")),
            "duplicate should get a suffix, got: {:?}",
            names
        );
    }

    #[test]
    fn test_patch_mapped_to_put() {
        let json = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "Test", "version": "1.0.0" },
            "paths": {
                "/items/{id}": {
                    "patch": {
                        "operationId": "patchItem",
                        "parameters": [{
                            "name": "id", "in": "path", "required": true,
                            "schema": { "type": "integer" }
                        }],
                        "responses": { "200": { "description": "OK" } }
                    }
                }
            }
        });
        let spec: OpenAPI = serde_json::from_value(json).unwrap();
        let (modules, _) = group_by_module(&spec, None).unwrap();

        let items = modules.iter().find(|m| m.name == "items").unwrap();
        let ep = &items.endpoints[0];
        assert_eq!(ep.method, "put", "PATCH should be mapped to PUT");
        assert!(
            ep.summary.as_ref().unwrap().contains("PATCH"),
            "summary should mention PATCH origin"
        );

        let output = generate_handler_stubs(items);
        assert!(
            output.contains("#[put("),
            "generated code should use #[put()] macro"
        );
    }

    #[test]
    fn test_operation_is_public_logic() {
        // No global security, no operation security -> public
        let json = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "Test", "version": "1.0.0" },
            "paths": {
                "/open": {
                    "get": {
                        "responses": { "200": { "description": "OK" } }
                    }
                }
            }
        });
        let spec: OpenAPI = serde_json::from_value(json).unwrap();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        assert!(modules[0].endpoints[0].is_public);

        // Global security set, no operation override -> not public
        let json = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "Test", "version": "1.0.0" },
            "security": [{ "bearerAuth": [] }],
            "paths": {
                "/protected": {
                    "get": {
                        "responses": { "200": { "description": "OK" } }
                    }
                }
            }
        });
        let spec: OpenAPI = serde_json::from_value(json).unwrap();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        assert!(!modules[0].endpoints[0].is_public);

        // Global security set, operation overrides with empty -> public
        let json = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "Test", "version": "1.0.0" },
            "security": [{ "bearerAuth": [] }],
            "paths": {
                "/health": {
                    "get": {
                        "security": [],
                        "responses": { "200": { "description": "OK" } }
                    }
                }
            }
        });
        let spec: OpenAPI = serde_json::from_value(json).unwrap();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        assert!(modules[0].endpoints[0].is_public);
    }

    #[test]
    fn test_map_openapi_type_formats() {
        let spec = make_test_spec();

        // UUID
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::String(openapiv3::StringType {
                format: VariantOrUnknownOrEmpty::Unknown("uuid".to_string()),
                ..Default::default()
            })),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "Uuid");

        // DateTime
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::String(openapiv3::StringType {
                format: VariantOrUnknownOrEmpty::Item(StringFormat::DateTime),
                ..Default::default()
            })),
        });
        assert_eq!(
            map_openapi_type(&spec, &schema),
            "chrono::DateTime<chrono::Utc>"
        );

        // Date
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::String(openapiv3::StringType {
                format: VariantOrUnknownOrEmpty::Item(StringFormat::Date),
                ..Default::default()
            })),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "chrono::NaiveDate");

        // Int64
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::Integer(openapiv3::IntegerType {
                format: VariantOrUnknownOrEmpty::Item(IntegerFormat::Int64),
                ..Default::default()
            })),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "i64");

        // Float
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::Number(openapiv3::NumberType {
                format: VariantOrUnknownOrEmpty::Item(NumberFormat::Float),
                ..Default::default()
            })),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "f32");

        // Double (default)
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::Number(Default::default())),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "f64");

        // Array of $ref
        let schema = ReferenceOr::Item(Schema {
            schema_data: Default::default(),
            schema_kind: SchemaKind::Type(Type::Array(openapiv3::ArrayType {
                items: Some(ReferenceOr::Reference {
                    reference: "#/components/schemas/User".to_string(),
                }),
                min_items: None,
                max_items: None,
                unique_items: false,
            })),
        });
        assert_eq!(map_openapi_type(&spec, &schema), "Vec<User>");
    }

    #[test]
    fn test_dto_optional_fields_in_generated_code() {
        let spec = make_test_spec();
        let (modules, _) = group_by_module(&spec, None).unwrap();
        let user_module = modules.iter().find(|m| m.name == "users").unwrap();
        let output = generate_dtos(user_module);

        // CreateUser: name and email are required, no Option wrapper
        // Find the CreateUser struct block
        let create_user_block: String = output
            .lines()
            .skip_while(|l| !l.contains("pub struct CreateUser"))
            .take_while(|l| !l.starts_with('}') || l.contains("pub struct CreateUser"))
            .chain(std::iter::once("}"))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            create_user_block.contains("pub email: String"),
            "required field should not be wrapped in Option, got:\n{}",
            create_user_block
        );
        assert!(
            create_user_block.contains("pub name: String"),
            "required field should not be wrapped in Option"
        );

        // User (response): all fields are required
        let user_block: String = output
            .lines()
            .skip_while(|l| !l.contains("pub struct User {"))
            .take_while(|l| !l.starts_with('}') || l.contains("pub struct User"))
            .chain(std::iter::once("}"))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !user_block.contains("Option<"),
            "all-required User should have no Option fields, got:\n{}",
            user_block
        );
    }

    #[test]
    fn test_yaml_parsing() {
        // Verify serde_yaml_ng can parse an OpenAPI spec from YAML.
        // parse_spec_file does file I/O so we test the parser directly.
        let yaml = "openapi: '3.0.0'\ninfo:\n  title: YAML Test\n  version: '1.0'\npaths: {}";
        let spec: OpenAPI = serde_yaml_ng::from_str(yaml).unwrap();
        assert_eq!(spec.info.title, "YAML Test");
    }

    #[test]
    fn test_warning_on_unsupported_schema() {
        let json = serde_json::json!({
            "openapi": "3.0.0",
            "info": { "title": "Test", "version": "1.0.0" },
            "paths": {
                "/things": {
                    "get": {
                        "responses": {
                            "200": {
                                "description": "OK",
                                "content": {
                                    "application/json": {
                                        "schema": {
                                            "oneOf": [
                                                { "type": "string" },
                                                { "type": "integer" }
                                            ]
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        let spec: OpenAPI = serde_json::from_value(json).unwrap();
        let (modules, _) = group_by_module(&spec, None).unwrap();

        let things = modules.iter().find(|m| m.name == "things").unwrap();
        // The oneOf schema should not generate a structured DTO — it falls back,
        // and the handler return type should reference serde_json::Value somewhere
        let output = generate_handler_stubs(things);
        assert!(
            output.contains("serde_json::Value"),
            "oneOf should fall back to serde_json::Value in handler output"
        );
    }
}
