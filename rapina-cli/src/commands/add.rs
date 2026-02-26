use colored::Colorize;

use super::codegen::{self, FieldInfo};

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
        "datetime" => ("DateTimeUtc", "DateTime", ".date_time().not_null()"),
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
    codegen::verify_rapina_project()?;

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
    let plural = &codegen::pluralize(name);
    let pascal = &codegen::to_pascal_case(name);
    let pascal_plural = &codegen::to_pascal_case(plural);

    println!();
    println!("  {} {}", "Adding resource:".bright_cyan(), pascal.bold());
    println!();

    codegen::create_feature_module(singular, plural, pascal, &fields)?;
    codegen::update_entity_file(pascal, &fields, None)?;
    codegen::create_migration_file(plural, pascal_plural, &fields)?;

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
            ("x:datetime", "DateTimeUtc", "DateTime"),
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
        assert_eq!(codegen::to_pascal_case("user"), "User");
        assert_eq!(codegen::to_pascal_case("blog_post"), "BlogPost");
        assert_eq!(codegen::to_pascal_case("my_long_name"), "MyLongName");
    }

    #[test]
    fn test_pluralize() {
        assert_eq!(codegen::pluralize("user"), "users");
        assert_eq!(codegen::pluralize("post"), "posts");
        assert_eq!(codegen::pluralize("blog_post"), "blog_posts");
    }

    #[test]
    fn test_generate_mod_rs() {
        let content = codegen::generate_mod_rs();
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
        let content = codegen::generate_handlers("post", "posts", "Post", &fields);

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
        let content = codegen::generate_dto("User", &fields);

        assert!(content.contains("pub struct CreateUser"));
        assert!(content.contains("pub struct UpdateUser"));
        assert!(content.contains("pub name: String,"));
        assert!(content.contains("pub age: i32,"));
        assert!(content.contains("pub name: Option<String>,"));
        assert!(content.contains("pub age: Option<i32>,"));
    }

    #[test]
    fn test_generate_error() {
        let content = codegen::generate_error("User");

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
        let content = codegen::generate_schema_block("Todo", &fields, None);

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
        let content = codegen::generate_migration("posts", "Posts", &fields);

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
