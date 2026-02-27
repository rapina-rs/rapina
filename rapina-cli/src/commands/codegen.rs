use colored::Colorize;
use std::fs;
use std::path::Path;

pub(crate) struct FieldInfo {
    pub name: String,
    pub rust_type: String,
    pub schema_type: String,
    pub column_method: String,
}

pub(crate) fn to_pascal_case(s: &str) -> String {
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

pub(crate) fn pluralize(s: &str) -> String {
    format!("{}s", s)
}

pub(crate) fn singularize(s: &str) -> String {
    if let Some(stem) = s.strip_suffix("ies") {
        format!("{}y", stem)
    } else if let Some(stem) = s.strip_suffix("sses") {
        // "bosses" -> "boss"
        format!("{}ss", stem)
    } else if let Some(stem) = s.strip_suffix("shes") {
        // "bushes" -> "bush"
        format!("{}sh", stem)
    } else if let Some(stem) = s.strip_suffix("ches") {
        // "watches" -> "watch"
        format!("{}ch", stem)
    } else if let Some(stem) = s.strip_suffix("xes") {
        // "boxes" -> "box"
        format!("{}x", stem)
    } else if let Some(stem) = s.strip_suffix("zes") {
        // "buzzes" -> "buzz"
        format!("{}z", stem)
    } else if let Some(stem) = s.strip_suffix("ses") {
        // "addresses" -> "address"
        format!("{}s", stem)
    } else if let Some(stem) = s.strip_suffix('s') {
        if stem.ends_with('s') {
            s.to_string() // "boss" -> "boss"
        } else {
            stem.to_string()
        }
    } else {
        s.to_string()
    }
}

pub(crate) fn verify_rapina_project() -> Result<(), String> {
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

pub(crate) fn generate_mod_rs() -> String {
    "pub mod dto;\npub mod error;\npub mod handlers;\n".to_string()
}

pub(crate) fn generate_handlers(
    singular: &str,
    plural: &str,
    pascal: &str,
    fields: &[FieldInfo],
) -> String {
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

pub(crate) fn generate_dto(pascal: &str, fields: &[FieldInfo]) -> String {
    let create_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("    pub {}: {},", f.name, f.rust_type))
        .collect();

    let update_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("    pub {}: Option<{}>,", f.name, f.rust_type))
        .collect();

    // Detect non-primitive types that need imports from sea_orm prelude
    let needs_sea_orm_import = fields.iter().any(|f| {
        matches!(
            f.rust_type.as_str(),
            "Uuid" | "DateTimeUtc" | "Date" | "Decimal" | "Json"
        )
    });

    let extra_import = if needs_sea_orm_import {
        "use rapina::sea_orm::prelude::*;\n"
    } else {
        ""
    };

    format!(
        r#"use rapina::schemars::{{self, JsonSchema}};
use serde::Deserialize;
{extra_import}
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
        extra_import = extra_import,
        create_fields = create_fields.join("\n"),
        update_fields = update_fields.join("\n"),
    )
}

pub(crate) fn generate_error(pascal: &str) -> String {
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

pub(crate) fn generate_schema_block(
    pascal: &str,
    fields: &[FieldInfo],
    timestamps: Option<&str>,
) -> String {
    let schema_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("        {}: {},", f.name, f.schema_type))
        .collect();

    let attr = match timestamps {
        None => String::new(),
        Some(ts) => format!("\n    #[timestamps({})]\n", ts),
    };

    format!(
        r#"
schema! {{
    {pascal} {{{attr}
{fields}
    }}
}}
"#,
        pascal = pascal,
        attr = attr,
        fields = schema_fields.join("\n"),
    )
}

pub(crate) fn generate_migration(
    plural: &str,
    pascal_plural: &str,
    fields: &[FieldInfo],
) -> String {
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

pub(crate) fn update_entity_file(
    pascal: &str,
    fields: &[FieldInfo],
    timestamps: Option<&str>,
) -> Result<(), String> {
    let entity_path = Path::new("src/entity.rs");
    let schema_block = generate_schema_block(pascal, fields, timestamps);

    if entity_path.exists() {
        let content = fs::read_to_string(entity_path)
            .map_err(|e| format!("Failed to read entity.rs: {}", e))?;

        // Ensure schema! macro is importable
        let needs_import =
            !content.contains("use rapina::prelude::*") && !content.contains("use rapina::schema");
        let prefix = if needs_import {
            "use rapina::schema;\n"
        } else {
            ""
        };

        let updated = format!("{}{}{}", prefix, content.trim_end(), schema_block);
        fs::write(entity_path, updated).map_err(|e| format!("Failed to write entity.rs: {}", e))?;
    } else {
        let content = format!("use rapina::prelude::*;\n{}", schema_block);
        fs::write(entity_path, content)
            .map_err(|e| format!("Failed to create entity.rs: {}", e))?;
    }

    println!("  {} Updated {}", "✓".green(), "src/entity.rs".cyan());
    Ok(())
}

pub(crate) fn create_migration_file(
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

pub(crate) fn create_feature_module(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singularize() {
        assert_eq!(singularize("users"), "user");
        assert_eq!(singularize("posts"), "post");
        assert_eq!(singularize("categories"), "category");
        assert_eq!(singularize("addresses"), "address");
        assert_eq!(singularize("boxes"), "box");
        assert_eq!(singularize("buzzes"), "buzz");
        assert_eq!(singularize("boss"), "boss");
        assert_eq!(singularize("status"), "statu"); // naive, acceptable
    }

    #[test]
    fn test_generate_schema_block_with_timestamps() {
        let fields = vec![FieldInfo {
            name: "title".to_string(),
            rust_type: "String".to_string(),
            schema_type: "String".to_string(),
            column_method: String::new(),
        }];

        let block = generate_schema_block("Post", &fields, None);
        assert!(block.contains("schema! {"));
        assert!(block.contains("Post {"));
        assert!(block.contains("title: String,"));
        assert!(!block.contains("#[timestamps"));

        let block = generate_schema_block("Post", &fields, Some("none"));
        assert!(block.contains("#[timestamps(none)]"));

        let block = generate_schema_block("Post", &fields, Some("created_at"));
        assert!(block.contains("#[timestamps(created_at)]"));
    }
}
