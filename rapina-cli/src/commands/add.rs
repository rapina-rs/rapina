use colored::Colorize;
use std::path::Path;

use super::{FieldInfo, codegen};
use crate::commands::{NormalizedType, ValidationContext};

fn print_next_steps(pascal: &str) {
    println!();
    println!("  {}:", "Next steps".bright_yellow());
    println!();
    println!("  1. Run {} to verify", "cargo build".cyan());
    println!();
    println!(
        "  Resource {} created successfully!",
        pascal.bright_green().bold()
    );
    println!();
}

pub fn resource(name: String, fields: Vec<FieldInfo>, with_timestamps: bool) -> Result<(), String> {
    ValidationContext::Resource.validate(&name)?;
    codegen::verify_rapina_project()?;

    if fields.is_empty() {
        return Err(
            "At least one field is required. Usage: rapina add resource <name> <field:type> ..."
                .to_string(),
        );
    }

    let plural = &codegen::pluralize(&name);
    let pascal = &codegen::to_pascal_case(&name);
    let pascal_plural = &codegen::to_pascal_case(plural);

    println!();
    println!("  {} {}", "Adding resource:".bright_cyan(), pascal.bold());
    println!();

    // verify if exists id field and get type
    // default is i32
    let pk_type = fields
        .iter()
        .find(|f| f.name == "id")
        .map_or(NormalizedType::I32, |f| f.normalized_type.clone());

    // None = no #[timestamps] attr → schema! macro adds both (default).
    // Some("none") = #[timestamps(none)] → macro skips them.
    let timestamps_attr = if with_timestamps { None } else { Some("none") };

    codegen::create_feature_module(&name, plural, pascal, &fields, &pk_type, false)?;
    codegen::update_entity_file(pascal, &fields, timestamps_attr, None, false)?;
    codegen::create_migration_file(plural, pascal_plural, &fields, with_timestamps)?;

    if let Err(e) = codegen::wire_main_rs(&[plural.as_str()], Path::new(".")) {
        eprintln!("  {} Could not auto-wire main.rs: {}", "!".yellow(), e);
    }
    print_next_steps(pascal);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::NormalizedType;

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(codegen::to_pascal_case("user"), "User");
        assert_eq!(codegen::to_pascal_case("blog_post"), "BlogPost");
        assert_eq!(codegen::to_pascal_case("my_long_name"), "MyLongName");
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
            "title:string".parse().unwrap(),
            "active:bool".parse().unwrap(),
        ];
        let content =
            codegen::generate_handlers("post", "posts", "Post", &fields, &NormalizedType::I32);

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
        let fields: Vec<FieldInfo> =
            vec!["name:string".parse().unwrap(), "age:i32".parse().unwrap()];
        let content = codegen::generate_dto("User", &fields);

        assert!(content.contains("pub struct CreateUser"));
        assert!(content.contains("pub struct UpdateUser"));
        assert!(content.contains("pub name: String,"));
        assert!(content.contains("pub age: i32,"));
        assert!(content.contains("pub name: Option<String>,"));
        assert!(content.contains("pub age: Option<i32>,"));
    }

    #[test]
    fn test_generate_dto_nullable_fields() {
        let mut fields: Vec<FieldInfo> = vec![
            "title:string".parse().unwrap(),
            "bio:string".parse().unwrap(),
        ];
        // Note: FieldInfo::from_str currently defaults nullable to false.
        // We manually set nullable for the test case until FromStr supports nullable syntax.
        fields[1].nullable = true;

        let content = codegen::generate_dto("User", &fields);

        // Non-nullable field: required in CreateDTO
        assert!(content.contains("pub title: String,"));
        // Nullable field: Option in CreateDTO
        assert!(content.contains("pub bio: Option<String>,"));
        // Both are Option in UpdateDTO
        assert!(content.contains("pub title: Option<String>,"));
    }

    #[test]
    fn test_generate_dto_uuid_decimal_imports() {
        let fields: Vec<FieldInfo> =
            vec!["id:uuid".parse().unwrap(), "price:decimal".parse().unwrap()];
        let content = codegen::generate_dto("Product", &fields);

        // Must use original crate paths, not sea_orm re-exports
        assert!(content.contains("use rapina::uuid::Uuid;"));
        assert!(content.contains("use rapina::rust_decimal::Decimal;"));
        // Must NOT use the glob import
        assert!(!content.contains("sea_orm::prelude::*"));
    }

    #[test]
    fn test_generate_dto_sea_orm_types_import() {
        let fields: Vec<FieldInfo> = vec![
            "created_at:datetimeutc".parse().unwrap(),
            "metadata:json".parse().unwrap(),
        ];
        // Manually set nullable for testing
        let mut fields = fields;
        fields[1].nullable = true;

        let content = codegen::generate_dto("Event", &fields);

        assert!(content.contains("use rapina::sea_orm::prelude::{DateTimeUtc, Json};"));
        assert!(!content.contains("sea_orm::prelude::*"));
    }

    #[test]
    fn test_generate_dto_sea_orm_date_import() {
        let fields = vec!["birthday:date".parse().unwrap()];
        let content = codegen::generate_dto("Person", &fields);

        assert!(content.contains("use rapina::sea_orm::prelude::{Date};"));
        assert!(!content.contains("sea_orm::prelude::*"));
    }

    #[test]
    fn test_generate_dto_mixed_types_imports() {
        let fields: Vec<FieldInfo> = vec![
            "id:uuid".parse().unwrap(),
            "amount:decimal".parse().unwrap(),
            "created_at:datetimeutc".parse().unwrap(),
            "name:string".parse().unwrap(),
        ];
        let content = codegen::generate_dto("Order", &fields);

        assert!(content.contains("use rapina::uuid::Uuid;"));
        assert!(content.contains("use rapina::rust_decimal::Decimal;"));
        assert!(content.contains("use rapina::sea_orm::prelude::{DateTimeUtc};"));
        assert!(!content.contains("sea_orm::prelude::*"));
    }

    #[test]
    fn test_generate_dto_primitives_no_extra_imports() {
        let fields: Vec<FieldInfo> = vec!["name:string".parse().unwrap()];
        let content = codegen::generate_dto("Simple", &fields);

        assert!(!content.contains("sea_orm"));
        assert!(!content.contains("uuid"));
        assert!(!content.contains("rust_decimal"));
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
        let fields: Vec<FieldInfo> = vec![
            "title:string".parse().unwrap(),
            "done:bool".parse().unwrap(),
        ];
        let content = codegen::generate_schema_block("Todo", &fields, None, None);

        assert!(content.contains("schema! {"));
        assert!(content.contains("Todo {"));
        assert!(content.contains("title: String,"));
        assert!(content.contains("done: bool,"));
    }

    #[test]
    fn test_generate_migration() {
        let fields: Vec<FieldInfo> = vec![
            "title:string".parse().unwrap(),
            "published:bool".parse().unwrap(),
        ];
        let content = codegen::generate_migration("posts", "Posts", &fields, false);

        assert!(content.contains("MigrationTrait for Migration"));
        assert!(content.contains("Posts::Table"));
        assert!(content.contains("Posts::Title"));
        assert!(content.contains("Posts::Published"));
        assert!(content.contains(".string().not_null()"));
        assert!(content.contains(".boolean().default(Expr::value(false)).not_null()"));
        assert!(content.contains("enum Posts {"));
        assert!(content.contains("drop_table"));
        // no timestamps when with_timestamps=false
        assert!(!content.contains("CreatedAt"));
        assert!(!content.contains("UpdatedAt"));
    }

    #[test]
    fn test_boolean_field_has_default_false() {
        let f = "active:bool".parse::<FieldInfo>().unwrap();
        assert!(
            f.generate_column("Active")
                .contains(".default(Expr::value(false))"),
        );

        let f2 = "enabled:boolean".parse::<FieldInfo>().unwrap();
        assert!(
            f2.generate_column("Enabled")
                .contains(".default(Expr::value(false))"),
        );
    }

    #[test]
    fn test_generate_migration_with_timestamps() {
        let fields = vec!["title:string".parse().unwrap()];
        let content = codegen::generate_migration("posts", "Posts", &fields, true);

        assert!(content.contains("Posts::CreatedAt"));
        assert!(content.contains("Posts::UpdatedAt"));
        assert!(content.contains("CreatedAt,"));
        assert!(content.contains("UpdatedAt,"));
        assert!(
            content.contains(
                ".timestamp_with_time_zone().not_null().default(Expr::current_timestamp())"
            )
        );
    }
}
