//! Parser for `src/entity.rs` schema! blocks.
//!
//! Extracts entity definitions from the text of an entity file
//! without compiling the user's code. Used by `import database --diff`
//! to compare code definitions against a live database.

use std::fs;
use std::path::Path;

use super::codegen;

/// An entity definition extracted from src/entity.rs.
#[derive(Debug, Clone)]
pub struct ParsedEntity {
    /// PascalCase name as written in schema! (e.g., "User")
    pub name: String,
    /// Resolved table name (from #[table_name] or auto-pluralized)
    pub table_name: String,
    /// Fields declared in the entity (excludes auto-generated id, timestamps)
    pub fields: Vec<ParsedField>,
    /// Whether created_at is expected
    pub has_created_at: bool,
    /// Whether updated_at is expected
    pub has_updated_at: bool,
    /// Primary key columns (None = default auto i32 "id")
    pub primary_key: Option<Vec<String>>,
}

/// A field extracted from an entity definition.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParsedField {
    /// Field name as written in schema!
    pub name: String,
    /// The resolved column name (from #[column] or field name, with _id suffix for belongs_to)
    pub column_name: String,
    /// The schema type string (e.g., "String", "i32", "bool")
    pub schema_type: String,
    /// Whether the field is Option<T>
    pub optional: bool,
    /// Whether this is a belongs_to relationship (generates an _id FK column)
    pub is_belongs_to: bool,
    /// Whether this is a has_many relationship (Vec<T>, no column in DB)
    pub is_has_many: bool,
}

/// Known scalar types that correspond to DB columns.
const SCALAR_TYPES: &[&str] = &[
    "String",
    "Text",
    "i32",
    "i64",
    "f32",
    "f64",
    "bool",
    "Uuid",
    "DateTime",
    "NaiveDateTime",
    "Date",
    "Decimal",
    "Json",
];

/// Parse entity definitions from a file path.
pub fn parse_entity_file_at(path: &Path) -> Result<Vec<ParsedEntity>, String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    parse_entity_source(&content)
}

/// Parse entity definitions from source text.
pub fn parse_entity_source(content: &str) -> Result<Vec<ParsedEntity>, String> {
    let blocks = extract_schema_blocks(content);
    let mut entities = Vec::new();

    for block in blocks {
        entities.extend(parse_schema_block(&block)?);
    }

    Ok(entities)
}

/// Extract the text content of all `schema! { ... }` blocks.
fn extract_schema_blocks(content: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = content.lines().peekable();

    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("schema! {") {
            let mut block_lines = vec![line.to_string()];
            let mut depth: i32 =
                line.matches('{').count() as i32 - line.matches('}').count() as i32;

            while depth > 0 {
                if let Some(next) = lines.next() {
                    depth += next.matches('{').count() as i32 - next.matches('}').count() as i32;
                    block_lines.push(next.to_string());
                } else {
                    break;
                }
            }

            blocks.push(block_lines.join("\n"));
        }
    }

    blocks
}

/// Parse a single `schema! { ... }` block, which may contain multiple entities.
fn parse_schema_block(block: &str) -> Result<Vec<ParsedEntity>, String> {
    // Strip the outer `schema! { ... }` wrapper
    let inner = strip_schema_wrapper(block)?;
    let mut entities = Vec::new();
    let mut remaining = inner.as_str();

    while !remaining.trim().is_empty() {
        let (entity, rest) = parse_single_entity(remaining)?;
        entities.push(entity);
        remaining = rest;
    }

    Ok(entities)
}

/// Strip `schema! {` prefix and final `}` suffix.
fn strip_schema_wrapper(block: &str) -> Result<String, String> {
    let trimmed = block.trim();
    let inner = trimmed
        .strip_prefix("schema!")
        .ok_or("Expected schema! block")?
        .trim();
    let inner = inner
        .strip_prefix('{')
        .ok_or("Expected opening brace after schema!")?;
    let inner = inner
        .strip_suffix('}')
        .ok_or("Expected closing brace for schema!")?;
    Ok(inner.to_string())
}

/// Parse a single entity definition from the text, returning the entity and remaining text.
fn parse_single_entity(text: &str) -> Result<(ParsedEntity, &str), String> {
    let text = text.trim();

    // Parse entity-level attributes
    let (attrs, after_attrs) = parse_entity_attributes(text);

    // Parse entity name
    let after_attrs = after_attrs.trim();
    let name_end = after_attrs
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .ok_or("Expected entity name")?;
    let name = after_attrs[..name_end].to_string();
    if name.is_empty() {
        return Err("Empty entity name".to_string());
    }
    let after_name = after_attrs[name_end..].trim();

    // Find the entity body between { ... }
    let after_brace = after_name
        .strip_prefix('{')
        .ok_or("Expected opening brace after entity name")?;

    // Find matching closing brace
    let mut depth = 1i32;
    let mut body_end = 0;
    for (i, c) in after_brace.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    body_end = i;
                    break;
                }
            }
            _ => {}
        }
    }

    if depth != 0 {
        return Err(format!("Unmatched braces in entity {}", name));
    }

    let body = &after_brace[..body_end];
    let remaining = &after_brace[body_end + 1..];

    // Parse fields
    let fields = parse_fields(body)?;

    // Resolve table name
    let table_name = attrs
        .table_name
        .unwrap_or_else(|| codegen::pluralize(&to_snake_case(&name)));

    Ok((
        ParsedEntity {
            name,
            table_name,
            fields,
            has_created_at: attrs.has_created_at,
            has_updated_at: attrs.has_updated_at,
            primary_key: attrs.primary_key,
        },
        remaining,
    ))
}

/// Intermediate entity attributes during parsing.
struct EntityAttributes {
    table_name: Option<String>,
    has_created_at: bool,
    has_updated_at: bool,
    primary_key: Option<Vec<String>>,
}

impl Default for EntityAttributes {
    fn default() -> Self {
        Self {
            table_name: None,
            has_created_at: true,
            has_updated_at: true,
            primary_key: None,
        }
    }
}

/// Parse entity-level attributes, returning the attributes and remaining text.
fn parse_entity_attributes(text: &str) -> (EntityAttributes, &str) {
    let mut attrs = EntityAttributes::default();
    let mut pos = text;

    loop {
        let trimmed = pos.trim_start();
        if !trimmed.starts_with("#[") {
            return (attrs, trimmed);
        }

        // Find the matching ]
        if let Some(bracket_end) = trimmed.find(']') {
            let attr_content = &trimmed[2..bracket_end];

            if let Some(value) = attr_content.strip_prefix("table_name") {
                // #[table_name = "people"]
                if let Some(start) = value.find('"') {
                    if let Some(end) = value[start + 1..].find('"') {
                        attrs.table_name = Some(value[start + 1..start + 1 + end].to_string());
                    }
                }
            } else if let Some(value) = attr_content.strip_prefix("timestamps(") {
                // #[timestamps(none)] or #[timestamps(created_at)] etc.
                let value = value.trim_end_matches(')').trim();
                match value {
                    "none" => {
                        attrs.has_created_at = false;
                        attrs.has_updated_at = false;
                    }
                    "created_at" => {
                        attrs.has_created_at = true;
                        attrs.has_updated_at = false;
                    }
                    "updated_at" => {
                        attrs.has_created_at = false;
                        attrs.has_updated_at = true;
                    }
                    _ => {}
                }
            } else if let Some(value) = attr_content.strip_prefix("primary_key(") {
                // #[primary_key(user_id, role_id)]
                let value = value.trim_end_matches(')');
                let cols: Vec<String> = value.split(',').map(|s| s.trim().to_string()).collect();
                if !cols.is_empty() {
                    attrs.primary_key = Some(cols);
                }
            }

            pos = &trimmed[bracket_end + 1..];
        } else {
            return (attrs, trimmed);
        }
    }
}

/// Parse field definitions from the body of an entity.
fn parse_fields(body: &str) -> Result<Vec<ParsedField>, String> {
    let mut fields = Vec::new();
    let mut remaining = body.trim();

    while !remaining.is_empty() {
        // Skip whitespace and commas
        remaining = remaining.trim_start();
        if remaining.is_empty() {
            break;
        }

        // Parse optional field attributes
        let (field_attrs, after_field_attrs) = parse_field_attributes(remaining);
        remaining = after_field_attrs.trim_start();

        if remaining.is_empty() {
            break;
        }

        // Parse field name
        let name_end = remaining
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(remaining.len());
        let field_name = remaining[..name_end].to_string();
        if field_name.is_empty() {
            break;
        }
        remaining = remaining[name_end..].trim_start();

        // Expect colon
        remaining = remaining
            .strip_prefix(':')
            .unwrap_or(remaining)
            .trim_start();

        // Parse type
        let (type_str, optional, is_vec, after_type) = parse_field_type(remaining);
        remaining = after_type;

        // Skip trailing comma
        remaining = remaining.trim_start();
        if remaining.starts_with(',') {
            remaining = &remaining[1..];
        }

        // Classify the field
        let is_scalar = SCALAR_TYPES.contains(&type_str.as_str());
        let is_has_many = is_vec;
        let is_belongs_to = !is_scalar && !is_has_many;

        let column_name = if let Some(ref col) = field_attrs.column_name {
            col.clone()
        } else if is_belongs_to {
            format!("{}_id", field_name)
        } else {
            field_name.clone()
        };

        fields.push(ParsedField {
            name: field_name,
            column_name,
            schema_type: type_str,
            optional,
            is_belongs_to,
            is_has_many,
        });
    }

    Ok(fields)
}

/// Intermediate field attributes during parsing.
#[derive(Default)]
struct FieldAttributes {
    column_name: Option<String>,
}

/// Parse field-level attributes, returning the attributes and remaining text.
fn parse_field_attributes(text: &str) -> (FieldAttributes, &str) {
    let mut attrs = FieldAttributes::default();
    let mut pos = text;

    loop {
        let trimmed = pos.trim_start();
        if !trimmed.starts_with("#[") {
            return (attrs, trimmed);
        }

        if let Some(bracket_end) = trimmed.find(']') {
            let attr_content = &trimmed[2..bracket_end];

            if let Some(value) = attr_content.strip_prefix("column") {
                // #[column = "email_address"]
                if let Some(start) = value.find('"') {
                    if let Some(end) = value[start + 1..].find('"') {
                        attrs.column_name = Some(value[start + 1..start + 1 + end].to_string());
                    }
                }
            }
            // #[unique] and #[index] are ignored for diff purposes

            pos = &trimmed[bracket_end + 1..];
        } else {
            return (attrs, trimmed);
        }
    }
}

/// Parse a field type, returning (type_name, is_optional, is_vec, remaining_text).
fn parse_field_type(text: &str) -> (String, bool, bool, &str) {
    let text = text.trim_start();

    // Option<T>
    if let Some(after) = text.strip_prefix("Option<") {
        let (inner, rest) = extract_until_angle_close(after);
        return (inner, true, false, rest);
    }

    // Vec<T>
    if let Some(after) = text.strip_prefix("Vec<") {
        let (inner, rest) = extract_until_angle_close(after);
        return (inner, false, true, rest);
    }

    // Plain type
    let end = text
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(text.len());
    let type_name = text[..end].to_string();
    (type_name, false, false, &text[end..])
}

/// Extract text until matching `>`, returning (content, rest_after_close).
fn extract_until_angle_close(text: &str) -> (String, &str) {
    let mut depth = 1i32;
    for (i, c) in text.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth == 0 {
                    return (text[..i].trim().to_string(), &text[i + 1..]);
                }
            }
            _ => {}
        }
    }
    (text.to_string(), "")
}

/// Convert PascalCase to snake_case.
fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_lowercase().next().unwrap());
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // to_snake_case
    // -----------------------------------------------------------------------

    #[test]
    fn test_snake_case_simple() {
        assert_eq!(to_snake_case("User"), "user");
        assert_eq!(to_snake_case("BlogPost"), "blog_post");
        assert_eq!(to_snake_case("HTMLParser"), "h_t_m_l_parser");
    }

    // -----------------------------------------------------------------------
    // extract_schema_blocks
    // -----------------------------------------------------------------------

    #[test]
    fn test_extract_single_schema_block() {
        let content = r#"use rapina::prelude::*;

schema! {
    User {
        email: String,
    }
}
"#;
        let blocks = extract_schema_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("User"));
    }

    #[test]
    fn test_extract_multiple_schema_blocks() {
        let content = r#"use rapina::prelude::*;

schema! {
    User {
        email: String,
    }
}

schema! {
    Post {
        title: String,
    }
}
"#;
        let blocks = extract_schema_blocks(content);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].contains("User"));
        assert!(blocks[1].contains("Post"));
    }

    #[test]
    fn test_extract_no_schema_blocks() {
        let content = "use rapina::prelude::*;\n\nfn main() {}\n";
        let blocks = extract_schema_blocks(content);
        assert!(blocks.is_empty());
    }

    // -----------------------------------------------------------------------
    // Simple entity parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_simple_entity() {
        let source = r#"
schema! {
    User {
        email: String,
        name: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities.len(), 1);

        let user = &entities[0];
        assert_eq!(user.name, "User");
        assert_eq!(user.table_name, "users");
        assert_eq!(user.fields.len(), 2);
        assert!(user.has_created_at);
        assert!(user.has_updated_at);
        assert!(user.primary_key.is_none());

        assert_eq!(user.fields[0].name, "email");
        assert_eq!(user.fields[0].column_name, "email");
        assert_eq!(user.fields[0].schema_type, "String");
        assert!(!user.fields[0].optional);
        assert!(!user.fields[0].is_belongs_to);
        assert!(!user.fields[0].is_has_many);
    }

    #[test]
    fn test_parse_all_scalar_types() {
        let source = r#"
schema! {
    #[timestamps(none)]
    AllTypes {
        a: String,
        b: Text,
        c: i32,
        d: i64,
        e: f32,
        f: f64,
        g: bool,
        h: Uuid,
        i: DateTime,
        j: NaiveDateTime,
        k: Date,
        l: Decimal,
        m: Json,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities[0].fields.len(), 13);
        for field in &entities[0].fields {
            assert!(
                !field.is_belongs_to,
                "field {} should be scalar",
                field.name
            );
            assert!(!field.is_has_many, "field {} should be scalar", field.name);
        }
    }

    // -----------------------------------------------------------------------
    // Entity attributes
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_table_name_override() {
        let source = r#"
schema! {
    #[table_name = "people"]
    Person {
        name: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities[0].name, "Person");
        assert_eq!(entities[0].table_name, "people");
    }

    #[test]
    fn test_parse_timestamps_none() {
        let source = r#"
schema! {
    #[timestamps(none)]
    Token {
        value: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert!(!entities[0].has_created_at);
        assert!(!entities[0].has_updated_at);
    }

    #[test]
    fn test_parse_timestamps_created_at_only() {
        let source = r#"
schema! {
    #[timestamps(created_at)]
    AuditLog {
        action: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert!(entities[0].has_created_at);
        assert!(!entities[0].has_updated_at);
    }

    #[test]
    fn test_parse_timestamps_updated_at_only() {
        let source = r#"
schema! {
    #[timestamps(updated_at)]
    Config {
        value: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert!(!entities[0].has_created_at);
        assert!(entities[0].has_updated_at);
    }

    #[test]
    fn test_parse_default_timestamps() {
        let source = r#"
schema! {
    User {
        name: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert!(entities[0].has_created_at);
        assert!(entities[0].has_updated_at);
    }

    #[test]
    fn test_parse_primary_key() {
        let source = r#"
schema! {
    #[primary_key(user_id, role_id)]
    #[timestamps(none)]
    UsersRole {
        user_id: i32,
        role_id: i32,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(
            entities[0].primary_key,
            Some(vec!["user_id".to_string(), "role_id".to_string()])
        );
    }

    #[test]
    fn test_parse_uuid_primary_key() {
        let source = r#"
schema! {
    #[primary_key(id)]
    Event {
        id: Uuid,
        name: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities[0].primary_key, Some(vec!["id".to_string()]));
        // id should appear as a regular field
        assert!(entities[0].fields.iter().any(|f| f.name == "id"));
    }

    // -----------------------------------------------------------------------
    // Field attributes
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_column_override() {
        let source = r#"
schema! {
    User {
        #[column = "email_address"]
        email: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities[0].fields[0].name, "email");
        assert_eq!(entities[0].fields[0].column_name, "email_address");
    }

    #[test]
    fn test_parse_unique_and_index_ignored() {
        let source = r#"
schema! {
    User {
        #[unique]
        #[index]
        email: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities[0].fields.len(), 1);
        assert_eq!(entities[0].fields[0].name, "email");
    }

    // -----------------------------------------------------------------------
    // Relationships
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_belongs_to() {
        let source = r#"
schema! {
    Post {
        title: String,
        author: User,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        let author_field = &entities[0].fields[1];
        assert_eq!(author_field.name, "author");
        assert_eq!(author_field.column_name, "author_id");
        assert!(author_field.is_belongs_to);
        assert!(!author_field.is_has_many);
    }

    #[test]
    fn test_parse_optional_belongs_to() {
        let source = r#"
schema! {
    Post {
        title: String,
        reviewer: Option<User>,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        let reviewer = &entities[0].fields[1];
        assert_eq!(reviewer.name, "reviewer");
        assert_eq!(reviewer.column_name, "reviewer_id");
        assert!(reviewer.is_belongs_to);
        assert!(reviewer.optional);
    }

    #[test]
    fn test_parse_has_many() {
        let source = r#"
schema! {
    User {
        name: String,
        posts: Vec<Post>,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        let posts_field = &entities[0].fields[1];
        assert_eq!(posts_field.name, "posts");
        assert!(posts_field.is_has_many);
        assert!(!posts_field.is_belongs_to);
    }

    // -----------------------------------------------------------------------
    // Optional scalar fields
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_optional_scalar() {
        let source = r#"
schema! {
    User {
        name: String,
        bio: Option<Text>,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        let bio = &entities[0].fields[1];
        assert_eq!(bio.schema_type, "Text");
        assert!(bio.optional);
        assert!(!bio.is_belongs_to);
    }

    // -----------------------------------------------------------------------
    // Multiple entities
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_multiple_entities_separate_blocks() {
        let source = r#"use rapina::prelude::*;

schema! {
    User {
        email: String,
    }
}

schema! {
    Post {
        title: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].name, "User");
        assert_eq!(entities[1].name, "Post");
    }

    #[test]
    fn test_parse_multiple_entities_same_block() {
        let source = r#"
schema! {
    User {
        email: String,
    }

    Post {
        title: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].name, "User");
        assert_eq!(entities[1].name, "Post");
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_empty_source() {
        let entities = parse_entity_source("").unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn test_parse_no_schema_blocks() {
        let source = "use rapina::prelude::*;\n\nfn main() {}\n";
        let entities = parse_entity_source(source).unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn test_parse_entity_file_not_found() {
        let result = parse_entity_file_at(Path::new("/nonexistent/entity.rs"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_todo_app_entity() {
        // Matches the real example: rapina/examples/todo-app/src/entity.rs
        let source = r#"use rapina::prelude::*;

schema! {
    #[timestamps(none)]
    Todo {
        title: String,
        done: bool,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].name, "Todo");
        assert_eq!(entities[0].table_name, "todos");
        assert!(!entities[0].has_created_at);
        assert!(!entities[0].has_updated_at);
        assert_eq!(entities[0].fields.len(), 2);
        assert_eq!(entities[0].fields[0].name, "title");
        assert_eq!(entities[0].fields[0].schema_type, "String");
        assert_eq!(entities[0].fields[1].name, "done");
        assert_eq!(entities[0].fields[1].schema_type, "bool");
    }

    // -----------------------------------------------------------------------
    // Table name resolution
    // -----------------------------------------------------------------------

    #[test]
    fn test_table_name_pluralization() {
        let source = r#"
schema! {
    Category {
        name: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities[0].table_name, "categories");
    }

    #[test]
    fn test_table_name_override_takes_precedence() {
        let source = r#"
schema! {
    #[table_name = "my_custom_table"]
    Widget {
        label: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        assert_eq!(entities[0].table_name, "my_custom_table");
    }

    // -----------------------------------------------------------------------
    // Combined attributes
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_all_entity_attrs_combined() {
        let source = r#"
schema! {
    #[table_name = "team_members"]
    #[primary_key(team_id, user_id)]
    #[timestamps(none)]
    TeamMember {
        team_id: i32,
        user_id: i32,
        role: String,
    }
}
"#;
        let entities = parse_entity_source(source).unwrap();
        let e = &entities[0];
        assert_eq!(e.name, "TeamMember");
        assert_eq!(e.table_name, "team_members");
        assert_eq!(
            e.primary_key,
            Some(vec!["team_id".to_string(), "user_id".to_string()])
        );
        assert!(!e.has_created_at);
        assert!(!e.has_updated_at);
        assert_eq!(e.fields.len(), 3);
    }
}
