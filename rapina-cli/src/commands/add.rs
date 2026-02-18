use colored::Colorize;
use std::fs;
use std::path::Path;

struct FieldInfo {
    name: String,
    rust_type: String,
    schema_type: String,
    column_method: String,
}

fn parse_field(input: &str) -> Result<FieldInfo, String> {
    let parts: Vec<&str> = input.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid field format '{}'. Expected 'name:type' (e.g., 'title:string')",
            input
        ));
    }

    let name = parts[0].trim();
    let type_str = parts[1].trim();

    if name.is_empty() {
        return Err("Field name cannot be empty".to_string());
    }

    for c in name.chars() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_' {
            return Err(format!(
                "Field name must be lowercase alphanumeric with underscores, got '{}'",
                name
            ));
        }
    }

    let (rust_type, schema_type, column_method) = match type_str.to_lowercase().as_str() {
        "string" => ("String", "String", ".string().not_null()"),
        "text" => ("String", "Text", ".text().not_null()"),
        "i32" | "integer" => ("i32", "i32", ".integer().not_null()"),
        "i64" | "bigint" => ("i64", "i64", ".big_integer().not_null()"),
        "f32" | "float" => ("f32", "f32", ".float().not_null()"),
        "f64" | "double" => ("f64", "f64", ".double().not_null()"),
        "bool" | "boolean" => ("bool", "bool", ".boolean().not_null()"),
        "uuid" => ("Uuid", "Uuid", ".uuid().not_null()"),
        "datetime" => ("DateTime", "DateTime", ".date_time().not_null()"),
        "date" => ("Date", "Date", ".date().not_null()"),
        "decimal" => ("Decimal", "Decimal", ".decimal().not_null()"),
        "json" => ("Json", "Json", ".json().not_null()"),
        _ => {
            return Err(format!(
                "Unknown field type '{}'. Supported types: string, text, i32/integer, i64/bigint, \
                 f32/float, f64/double, bool/boolean, uuid, datetime, date, decimal, json",
                type_str
            ));
        }
    };

    Ok(FieldInfo {
        name: name.to_string(),
        rust_type: rust_type.to_string(),
        schema_type: schema_type.to_string(),
        column_method: column_method.to_string(),
    })
}

fn validate_resource_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Resource name cannot be empty".to_string());
    }

    for c in name.chars() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '_' {
            return Err(format!(
                "Resource name must be lowercase alphanumeric with underscores, got '{}'",
                c
            ));
        }
    }

    if name.starts_with('_') || name.ends_with('_') {
        return Err("Resource name cannot start or end with underscore".to_string());
    }

    let reserved = [
        "self", "super", "crate", "mod", "type", "fn", "struct", "enum", "impl",
    ];
    if reserved.contains(&name) {
        return Err(format!("'{}' is a reserved Rust keyword", name));
    }

    Ok(())
}

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

fn pluralize(s: &str) -> String {
    format!("{}s", s)
}

fn verify_rapina_project() -> Result<(), String> {
    let cargo_path = Path::new("Cargo.toml");
    if !cargo_path.exists() {
        return Err(
            "No Cargo.toml found. Run this command from the root of a Rapina project.".to_string(),
        );
    }

    let content =
        fs::read_to_string(cargo_path).map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;

    if !content.contains("rapina") {
        return Err("This doesn't appear to be a Rapina project (no rapina dependency found in Cargo.toml).".to_string());
    }

    Ok(())
}

fn generate_mod_rs() -> String {
    "pub mod dto;\npub mod error;\npub mod handlers;\n".to_string()
}

fn generate_handlers(singular: &str, plural: &str, pascal: &str, fields: &[FieldInfo]) -> String {
    let create_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("        {}: Set(input.{}),", f.name, f.name))
        .collect();
    let create_body = create_fields.join("\n");

    let update_checks: Vec<String> = fields
        .iter()
        .map(|f| {
            format!(
                "    if let Some(val) = update.{name} {{\n        active.{name} = Set(val);\n    }}",
                name = f.name
            )
        })
        .collect();
    let update_body = update_checks.join("\n");

    format!(
        r#"use rapina::prelude::*;
use rapina::database::{{Db, DbError}};
use rapina::sea_orm::{{ActiveModelTrait, EntityTrait, IntoActiveModel, Set}};

use crate::entity::{pascal};
use crate::entity::{singular}::{{ActiveModel, Model}};

use super::dto::{{Create{pascal}, Update{pascal}}};
use super::error::{pascal}Error;

#[get("/{plural}")]
#[errors({pascal}Error)]
pub async fn list_{plural}(db: Db) -> Result<Json<Vec<Model>>> {{
    let items = {pascal}::find().all(db.conn()).await.map_err(DbError)?;
    Ok(Json(items))
}}

#[get("/{plural}/:id")]
#[errors({pascal}Error)]
pub async fn get_{singular}(db: Db, id: Path<i32>) -> Result<Json<Model>> {{
    let id = id.into_inner();
    let item = {pascal}::find_by_id(id)
        .one(db.conn())
        .await
        .map_err(DbError)?
        .ok_or_else(|| Error::not_found(format!("{pascal} {{}} not found", id)))?;
    Ok(Json(item))
}}

#[post("/{plural}")]
#[errors({pascal}Error)]
pub async fn create_{singular}(db: Db, body: Json<Create{pascal}>) -> Result<Json<Model>> {{
    let input = body.into_inner();
    let item = ActiveModel {{
{create_body}
        ..Default::default()
    }};
    let result = item.insert(db.conn()).await.map_err(DbError)?;
    Ok(Json(result))
}}

#[put("/{plural}/:id")]
#[errors({pascal}Error)]
pub async fn update_{singular}(db: Db, id: Path<i32>, body: Json<Update{pascal}>) -> Result<Json<Model>> {{
    let id = id.into_inner();
    let item = {pascal}::find_by_id(id)
        .one(db.conn())
        .await
        .map_err(DbError)?
        .ok_or_else(|| Error::not_found(format!("{pascal} {{}} not found", id)))?;

    let update = body.into_inner();
    let mut active: ActiveModel = item.into_active_model();
{update_body}

    let result = active.update(db.conn()).await.map_err(DbError)?;
    Ok(Json(result))
}}

#[delete("/{plural}/:id")]
#[errors({pascal}Error)]
pub async fn delete_{singular}(db: Db, id: Path<i32>) -> Result<Json<serde_json::Value>> {{
    let id = id.into_inner();
    let result = {pascal}::delete_by_id(id)
        .exec(db.conn())
        .await
        .map_err(DbError)?;
    if result.rows_affected == 0 {{
        return Err(Error::not_found(format!("{pascal} {{}} not found", id)));
    }}
    Ok(Json(serde_json::json!({{ "deleted": id }})))
}}
"#,
        pascal = pascal,
        singular = singular,
        plural = plural,
        create_body = create_body,
        update_body = update_body,
    )
}

fn generate_dto(pascal: &str, fields: &[FieldInfo]) -> String {
    let create_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("    pub {}: {},", f.name, f.rust_type))
        .collect();

    let update_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("    pub {}: Option<{}>,", f.name, f.rust_type))
        .collect();

    format!(
        r#"use rapina::schemars::{{self, JsonSchema}};
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub struct Create{pascal} {{
{create_fields}
}}

#[derive(Deserialize, JsonSchema)]
pub struct Update{pascal} {{
{update_fields}
}}
"#,
        pascal = pascal,
        create_fields = create_fields.join("\n"),
        update_fields = update_fields.join("\n"),
    )
}

fn generate_error(pascal: &str) -> String {
    format!(
        r#"use rapina::database::DbError;
use rapina::prelude::*;

pub enum {pascal}Error {{
    DbError(DbError),
}}

impl IntoApiError for {pascal}Error {{
    fn into_api_error(self) -> Error {{
        match self {{
            {pascal}Error::DbError(e) => e.into_api_error(),
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
                code: "DATABASE_ERROR",
                description: "Database operation failed",
            }},
        ]
    }}
}}

impl From<DbError> for {pascal}Error {{
    fn from(e: DbError) -> Self {{
        {pascal}Error::DbError(e)
    }}
}}
"#,
        pascal = pascal,
    )
}

fn generate_schema_block(pascal: &str, fields: &[FieldInfo]) -> String {
    let schema_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("        {}: {},", f.name, f.schema_type))
        .collect();

    format!(
        r#"
schema! {{
    {pascal} {{
{fields}
    }}
}}
"#,
        pascal = pascal,
        fields = schema_fields.join("\n"),
    )
}

fn generate_migration(plural: &str, pascal_plural: &str, fields: &[FieldInfo]) -> String {
    let column_defs: Vec<String> = fields
        .iter()
        .map(|f| {
            let iden = to_pascal_case(&f.name);
            format!(
                "                    .col(ColumnDef::new({pascal_plural}::{iden}){col})",
                pascal_plural = pascal_plural,
                iden = iden,
                col = f.column_method,
            )
        })
        .collect();

    let iden_variants: Vec<String> = fields
        .iter()
        .map(|f| format!("    {},", to_pascal_case(&f.name)))
        .collect();

    let readable_name = format!("create {}", plural);

    format!(
        r#"//! Migration: {readable_name}

use rapina::sea_orm_migration;
use rapina::migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {{
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {{
        manager
            .create_table(
                Table::create()
                    .table({pascal_plural}::Table)
                    .col(
                        ColumnDef::new({pascal_plural}::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
{column_defs}
                    .to_owned(),
            )
            .await
    }}

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {{
        manager
            .drop_table(Table::drop().table({pascal_plural}::Table).to_owned())
            .await
    }}
}}

#[derive(DeriveIden)]
enum {pascal_plural} {{
    Table,
    Id,
{iden_variants}
}}
"#,
        readable_name = readable_name,
        pascal_plural = pascal_plural,
        column_defs = column_defs.join("\n"),
        iden_variants = iden_variants.join("\n"),
    )
}

fn update_entity_file(pascal: &str, fields: &[FieldInfo]) -> Result<(), String> {
    let entity_path = Path::new("src/entity.rs");
    let schema_block = generate_schema_block(pascal, fields);

    if entity_path.exists() {
        let content = fs::read_to_string(entity_path)
            .map_err(|e| format!("Failed to read entity.rs: {}", e))?;
        let updated = format!("{}{}", content.trim_end(), schema_block);
        fs::write(entity_path, updated).map_err(|e| format!("Failed to write entity.rs: {}", e))?;
    } else {
        let content = format!("use rapina::prelude::*;\n{}", schema_block);
        fs::write(entity_path, content)
            .map_err(|e| format!("Failed to create entity.rs: {}", e))?;
    }

    println!("  {} Updated {}", "✓".green(), "src/entity.rs".cyan());
    Ok(())
}

fn create_migration_file(
    plural: &str,
    pascal_plural: &str,
    fields: &[FieldInfo],
) -> Result<(), String> {
    let migrations_dir = Path::new("src/migrations");

    if !migrations_dir.exists() {
        fs::create_dir_all(migrations_dir)
            .map_err(|e| format!("Failed to create migrations directory: {}", e))?;
        println!("  {} Created {}", "✓".green(), "src/migrations/".cyan());
    }

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let migration_name = format!("create_{}", plural);
    let module_name = format!("m{}_{}", timestamp, migration_name);
    let filename = format!("{}.rs", module_name);
    let filepath = migrations_dir.join(&filename);

    let template = generate_migration(plural, pascal_plural, fields);
    fs::write(&filepath, template).map_err(|e| format!("Failed to write migration file: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/migrations/{}", filename).cyan()
    );

    super::migrate::update_mod_rs(migrations_dir, &module_name)?;

    Ok(())
}

fn create_feature_module(
    singular: &str,
    plural: &str,
    pascal: &str,
    fields: &[FieldInfo],
) -> Result<(), String> {
    let module_dir = Path::new("src").join(plural);

    if module_dir.exists() {
        return Err(format!(
            "Directory 'src/{}/' already exists. Remove it first or choose a different resource name.",
            plural
        ));
    }

    fs::create_dir_all(&module_dir)
        .map_err(|e| format!("Failed to create module directory: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/", plural).cyan()
    );

    fs::write(module_dir.join("mod.rs"), generate_mod_rs())
        .map_err(|e| format!("Failed to write mod.rs: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/mod.rs", plural).cyan()
    );

    fs::write(
        module_dir.join("handlers.rs"),
        generate_handlers(singular, plural, pascal, fields),
    )
    .map_err(|e| format!("Failed to write handlers.rs: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/handlers.rs", plural).cyan()
    );

    fs::write(module_dir.join("dto.rs"), generate_dto(pascal, fields))
        .map_err(|e| format!("Failed to write dto.rs: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/dto.rs", plural).cyan()
    );

    fs::write(module_dir.join("error.rs"), generate_error(pascal))
        .map_err(|e| format!("Failed to write error.rs: {}", e))?;
    println!(
        "  {} Created {}",
        "✓".green(),
        format!("src/{}/error.rs", plural).cyan()
    );

    Ok(())
}

fn print_next_steps(singular: &str, plural: &str, pascal: &str) {
    println!();
    println!("  {}:", "Next steps".bright_yellow());
    println!();
    println!(
        "  1. Add the module declaration to {}:",
        "src/main.rs".cyan()
    );
    println!();
    println!("     mod {};", plural);
    println!("     mod entity;");
    println!("     mod migrations;");
    println!();
    println!("  2. Register the routes in your {}:", "Router".cyan());
    println!();
    println!(
        "     use {plural}::handlers::{{list_{plural}, get_{singular}, create_{singular}, update_{singular}, delete_{singular}}};",
        plural = plural,
        singular = singular,
    );
    println!();
    println!("     let router = Router::new()");
    println!(
        "         .get(\"/{plural}\", list_{plural})",
        plural = plural
    );
    println!(
        "         .get(\"/{plural}/:id\", get_{singular})",
        plural = plural,
        singular = singular,
    );
    println!(
        "         .post(\"/{plural}\", create_{singular})",
        plural = plural,
        singular = singular,
    );
    println!(
        "         .put(\"/{plural}/:id\", update_{singular})",
        plural = plural,
        singular = singular,
    );
    println!(
        "         .delete(\"/{plural}/:id\", delete_{singular});",
        plural = plural,
        singular = singular,
    );
    println!();
    println!(
        "  3. Enable the database feature in {}:",
        "Cargo.toml".cyan()
    );
    println!();
    println!("     rapina = {{ version = \"...\", features = [\"postgres\"] }}");
    println!();
    println!(
        "  Resource {} created successfully!",
        pascal.bright_green().bold()
    );
    println!();
}

pub fn resource(name: &str, field_args: &[String]) -> Result<(), String> {
    validate_resource_name(name)?;
    verify_rapina_project()?;

    if field_args.is_empty() {
        return Err(
            "At least one field is required. Usage: rapina add resource <name> <field:type> ..."
                .to_string(),
        );
    }

    let fields: Vec<FieldInfo> = field_args
        .iter()
        .map(|arg| parse_field(arg))
        .collect::<Result<Vec<_>, _>>()?;

    let singular = name;
    let plural = &pluralize(name);
    let pascal = &to_pascal_case(name);
    let pascal_plural = &to_pascal_case(plural);

    println!();
    println!("  {} {}", "Adding resource:".bright_cyan(), pascal.bold());
    println!();

    create_feature_module(singular, plural, pascal, &fields)?;
    update_entity_file(pascal, &fields)?;
    create_migration_file(plural, pascal_plural, &fields)?;

    print_next_steps(singular, plural, pascal);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_field_valid() {
        let f = parse_field("name:string").unwrap();
        assert_eq!(f.name, "name");
        assert_eq!(f.rust_type, "String");
        assert_eq!(f.schema_type, "String");

        let f = parse_field("active:bool").unwrap();
        assert_eq!(f.name, "active");
        assert_eq!(f.rust_type, "bool");

        let f = parse_field("age:i32").unwrap();
        assert_eq!(f.name, "age");
        assert_eq!(f.rust_type, "i32");

        let f = parse_field("count:integer").unwrap();
        assert_eq!(f.name, "count");
        assert_eq!(f.rust_type, "i32");

        let f = parse_field("score:f64").unwrap();
        assert_eq!(f.name, "score");
        assert_eq!(f.rust_type, "f64");

        let f = parse_field("external_id:uuid").unwrap();
        assert_eq!(f.name, "external_id");
        assert_eq!(f.rust_type, "Uuid");
    }

    #[test]
    fn test_parse_field_all_types() {
        let cases = vec![
            ("x:string", "String", "String"),
            ("x:text", "String", "Text"),
            ("x:i32", "i32", "i32"),
            ("x:integer", "i32", "i32"),
            ("x:i64", "i64", "i64"),
            ("x:bigint", "i64", "i64"),
            ("x:f32", "f32", "f32"),
            ("x:float", "f32", "f32"),
            ("x:f64", "f64", "f64"),
            ("x:double", "f64", "f64"),
            ("x:bool", "bool", "bool"),
            ("x:boolean", "bool", "bool"),
            ("x:uuid", "Uuid", "Uuid"),
            ("x:datetime", "DateTime", "DateTime"),
            ("x:date", "Date", "Date"),
            ("x:decimal", "Decimal", "Decimal"),
            ("x:json", "Json", "Json"),
        ];
        for (input, expected_rust, expected_schema) in cases {
            let f = parse_field(input).unwrap();
            assert_eq!(f.rust_type, expected_rust, "failed for {}", input);
            assert_eq!(f.schema_type, expected_schema, "failed for {}", input);
        }
    }

    #[test]
    fn test_parse_field_invalid() {
        assert!(parse_field("name").is_err());
        assert!(parse_field(":string").is_err());
        assert!(parse_field("name:unknown").is_err());
        assert!(parse_field("Name:string").is_err());
    }

    #[test]
    fn test_validate_resource_name_valid() {
        assert!(validate_resource_name("user").is_ok());
        assert!(validate_resource_name("blog_post").is_ok());
        assert!(validate_resource_name("item123").is_ok());
    }

    #[test]
    fn test_validate_resource_name_invalid() {
        assert!(validate_resource_name("").is_err());
        assert!(validate_resource_name("User").is_err());
        assert!(validate_resource_name("_user").is_err());
        assert!(validate_resource_name("user_").is_err());
        assert!(validate_resource_name("self").is_err());
        assert!(validate_resource_name("user-name").is_err());
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("user"), "User");
        assert_eq!(to_pascal_case("blog_post"), "BlogPost");
        assert_eq!(to_pascal_case("my_long_name"), "MyLongName");
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(pluralize("user"), "users");
        assert_eq!(pluralize("post"), "posts");
        assert_eq!(pluralize("blog_post"), "blog_posts");
    }

    #[test]
    fn test_generate_mod_rs() {
        let content = generate_mod_rs();
        assert!(content.contains("pub mod dto;"));
        assert!(content.contains("pub mod error;"));
        assert!(content.contains("pub mod handlers;"));
    }

    #[test]
    fn test_generate_handlers() {
        let fields = vec![
            FieldInfo {
                name: "title".to_string(),
                rust_type: "String".to_string(),
                schema_type: "String".to_string(),
                column_method: ".string().not_null()".to_string(),
            },
            FieldInfo {
                name: "active".to_string(),
                rust_type: "bool".to_string(),
                schema_type: "bool".to_string(),
                column_method: ".boolean().not_null()".to_string(),
            },
        ];
        let content = generate_handlers("post", "posts", "Post", &fields);

        assert!(content.contains("use crate::entity::Post;"));
        assert!(content.contains("use crate::entity::post::{ActiveModel, Model};"));
        assert!(content.contains("pub async fn list_posts"));
        assert!(content.contains("pub async fn get_post"));
        assert!(content.contains("pub async fn create_post"));
        assert!(content.contains("pub async fn update_post"));
        assert!(content.contains("pub async fn delete_post"));
        assert!(content.contains("#[get(\"/posts\")]"));
        assert!(content.contains("#[post(\"/posts\")]"));
        assert!(content.contains("#[put(\"/posts/:id\")]"));
        assert!(content.contains("#[delete(\"/posts/:id\")]"));
        assert!(content.contains("title: Set(input.title),"));
        assert!(content.contains("active: Set(input.active),"));
        assert!(content.contains("if let Some(val) = update.title"));
        assert!(content.contains("if let Some(val) = update.active"));
    }

    #[test]
    fn test_generate_dto() {
        let fields = vec![
            FieldInfo {
                name: "name".to_string(),
                rust_type: "String".to_string(),
                schema_type: "String".to_string(),
                column_method: String::new(),
            },
            FieldInfo {
                name: "age".to_string(),
                rust_type: "i32".to_string(),
                schema_type: "i32".to_string(),
                column_method: String::new(),
            },
        ];
        let content = generate_dto("User", &fields);

        assert!(content.contains("pub struct CreateUser"));
        assert!(content.contains("pub struct UpdateUser"));
        assert!(content.contains("pub name: String,"));
        assert!(content.contains("pub age: i32,"));
        assert!(content.contains("pub name: Option<String>,"));
        assert!(content.contains("pub age: Option<i32>,"));
    }

    #[test]
    fn test_generate_error() {
        let content = generate_error("User");

        assert!(content.contains("pub enum UserError"));
        assert!(content.contains("impl IntoApiError for UserError"));
        assert!(content.contains("impl DocumentedError for UserError"));
        assert!(content.contains("impl From<DbError> for UserError"));
        assert!(content.contains("\"User not found\""));
    }

    #[test]
    fn test_generate_schema_block() {
        let fields = vec![
            FieldInfo {
                name: "title".to_string(),
                rust_type: "String".to_string(),
                schema_type: "String".to_string(),
                column_method: String::new(),
            },
            FieldInfo {
                name: "done".to_string(),
                rust_type: "bool".to_string(),
                schema_type: "bool".to_string(),
                column_method: String::new(),
            },
        ];
        let content = generate_schema_block("Todo", &fields);

        assert!(content.contains("schema! {"));
        assert!(content.contains("Todo {"));
        assert!(content.contains("title: String,"));
        assert!(content.contains("done: bool,"));
    }

    #[test]
    fn test_generate_migration() {
        let fields = vec![
            FieldInfo {
                name: "title".to_string(),
                rust_type: "String".to_string(),
                schema_type: "String".to_string(),
                column_method: ".string().not_null()".to_string(),
            },
            FieldInfo {
                name: "published".to_string(),
                rust_type: "bool".to_string(),
                schema_type: "bool".to_string(),
                column_method: ".boolean().not_null()".to_string(),
            },
        ];
        let content = generate_migration("posts", "Posts", &fields);

        assert!(content.contains("MigrationTrait for Migration"));
        assert!(content.contains("Posts::Table"));
        assert!(content.contains("Posts::Id"));
        assert!(content.contains("Posts::Title"));
        assert!(content.contains("Posts::Published"));
        assert!(content.contains(".string().not_null()"));
        assert!(content.contains(".boolean().not_null()"));
        assert!(content.contains("enum Posts {"));
        assert!(content.contains("drop_table"));
    }
}
