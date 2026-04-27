use super::{Colorize, FieldInfo, NormalizedType};
use std::fs;
use std::path::Path;

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

/// Irregular plural forms: (singular, plural)
const IRREGULARS: &[(&str, &str)] = &[
    // Common irregular plurals
    ("person", "people"),
    ("child", "children"),
    ("man", "men"),
    ("woman", "women"),
    ("mouse", "mice"),
    ("goose", "geese"),
    ("tooth", "teeth"),
    ("foot", "feet"),
    ("ox", "oxen"),
    // -f/-fe → -ves
    ("leaf", "leaves"),
    ("life", "lives"),
    ("knife", "knives"),
    ("wife", "wives"),
    ("half", "halves"),
    ("wolf", "wolves"),
    ("shelf", "shelves"),
    ("loaf", "loaves"),
    // Latin/Greek-origin
    ("datum", "data"),
    ("medium", "media"),
    ("criterion", "criteria"),
    ("phenomenon", "phenomena"),
    ("index", "indices"),
    ("vertex", "vertices"),
    ("matrix", "matrices"),
    ("appendix", "appendices"),
    ("analysis", "analyses"),
    ("base", "bases"),
    ("crisis", "crises"),
    ("thesis", "theses"),
    ("diagnosis", "diagnoses"),
    ("hypothesis", "hypotheses"),
    ("parenthesis", "parentheses"),
    ("synopsis", "synopses"),
    ("curriculum", "curricula"),
    ("formula", "formulae"),
    ("antenna", "antennae"),
    ("alumnus", "alumni"),
    ("cactus", "cacti"),
    ("fungus", "fungi"),
    ("nucleus", "nuclei"),
    ("radius", "radii"),
    ("stimulus", "stimuli"),
    ("syllabus", "syllabi"),
];

/// Words that are the same in singular and plural form.
const UNCOUNTABLE: &[&str] = &[
    "series",
    "species",
    "news",
    "info",
    "metadata",
    "sheep",
    "fish",
    "deer",
    "aircraft",
    "software",
    "hardware",
    "firmware",
    "middleware",
    "equipment",
    "feedback",
    "moose",
    "bison",
    "trout",
    "salmon",
    "shrimp",
];

/// Words ending in -us where the singular should not have -s stripped,
/// and the plural is formed by adding -es (e.g. status → statuses).
#[cfg(any(test, feature = "import", feature = "import-openapi"))]
const SINGULAR_US: &[&str] = &[
    "status",
    "campus",
    "virus",
    "census",
    "corpus",
    "opus",
    "genus",
    "apparatus",
    "nexus",
    "prospectus",
    "consensus",
];

pub(crate) fn pluralize(s: &str) -> String {
    if UNCOUNTABLE.contains(&s) {
        return s.to_string();
    }
    if IRREGULARS.iter().any(|(_, plural)| *plural == s) {
        return s.to_string();
    }
    if let Some((_, plural)) = IRREGULARS.iter().find(|(singular, _)| *singular == s) {
        return plural.to_string();
    }
    if s.ends_with("us") {
        return format!("{}es", s);
    }
    let cases = [
        ("ss", "sses"), //address -> addresses
        ("sh", "shes"), //bush -> bushes
        ("ch", "ches"), //watch -> watches
        ("x", "xes"),   //box -> boxes
        ("z", "zes"),   //gas -> gases
        ("s", "ses"),   //bus -> buses
        ("ay", "ays"),  //day -> days
        ("uy", "uys"),  //buy -> buys
        ("ey", "eys"),  //key -> keys
        ("oy", "oys"),  //boy -> boys
        ("y", "ies"),   //category -> categories
    ];
    for (suffix, replacement) in cases {
        if let Some(stem) = s.strip_suffix(suffix) {
            return format!("{}{}", stem, replacement);
        }
    }
    format!("{}s", s)
}

#[cfg(any(test, feature = "import", feature = "import-openapi"))]
pub(crate) fn singularize(s: &str) -> String {
    if UNCOUNTABLE.contains(&s) {
        return s.to_string();
    }
    if SINGULAR_US.contains(&s) {
        return s.to_string();
    }
    if IRREGULARS.iter().any(|(singular, _)| *singular == s) {
        return s.to_string();
    }
    if let Some((singular, _)) = IRREGULARS.iter().find(|(_, plural)| *plural == s) {
        return singular.to_string();
    }
    if let Some(stem) = s.strip_suffix("uses") {
        let candidate = format!("{}us", stem);
        if SINGULAR_US.contains(&candidate.as_str()) {
            return candidate;
        }
    }
    if let Some(stem) = s.strip_suffix("ies") {
        format!("{}y", stem)
    } else if let Some(stem) = s.strip_suffix("sses") {
        format!("{}ss", stem)
    } else if let Some(stem) = s.strip_suffix("shes") {
        format!("{}sh", stem)
    } else if let Some(stem) = s.strip_suffix("ches") {
        format!("{}ch", stem)
    } else if let Some(stem) = s.strip_suffix("xes") {
        format!("{}x", stem)
    } else if let Some(stem) = s.strip_suffix("zes") {
        format!("{}z", stem)
    } else if let Some(stem) = s.strip_suffix("ses") {
        format!("{}s", stem)
    } else if let Some(stem) = s.strip_suffix('s') {
        if stem.ends_with('s') {
            s.to_string()
        } else {
            stem.to_string()
        }
    } else {
        s.to_string()
    }
}

pub(crate) fn verify_rapina_project() -> Result<(), String> {
    super::verify_rapina_project()?;
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
    pk_type: &NormalizedType,
) -> String {
    let create_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("        {}: Set(input.{}),", f.name, f.name))
        .collect();
    let create_body = create_fields.join("\n");

    let update_checks: Vec<String> = fields
        .iter()
        .map(|f| {
            if f.nullable {
                 format!(
                    "    if let Some(val) = update.{name} {{\n        active.{name} = Set(Some(val));\n    }}",
                    name = f.name
                )
            } else {
                format!(
                    "    if let Some(val) = update.{name} {{\n        active.{name} = Set(val);\n    }}",
                    name = f.name
                )
            }
        })
        .collect();
    let update_body = update_checks.join("\n");

    let uuid_import = if pk_type == &NormalizedType::Uuid {
        "use rapina::uuid::Uuid;"
    } else {
        ""
    };

    format!(
        r#"use rapina::prelude::*;
use rapina::database::{{Db, DbError}};
use rapina::sea_orm::{{ActiveModelTrait, EntityTrait, IntoActiveModel, Set}};
{uuid_import}

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
pub async fn get_{singular}(db: Db, id: Path<{pk_type}>) -> Result<Json<Model>> {{
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
pub async fn update_{singular}(db: Db, id: Path<{pk_type}>, body: Json<Update{pascal}>) -> Result<Json<Model>> {{
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
pub async fn delete_{singular}(db: Db, id: Path<{pk_type}>) -> Result<Json<serde_json::Value>> {{
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
        pk_type = pk_type,
        uuid_import = uuid_import,
    )
}

pub(crate) fn generate_dto(pascal: &str, fields: &[FieldInfo]) -> String {
    let create_fields: Vec<String> = fields
        .iter()
        .map(|f| {
            if f.nullable {
                format!("    pub {}: Option<{}>,", f.name, f.normalized_type)
            } else {
                format!("    pub {}: {},", f.name, f.normalized_type)
            }
        })
        .collect();

    let update_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("    pub {}: Option<{}>,", f.name, f.normalized_type))
        .collect();

    // Build type-specific imports instead of sea_orm glob.
    // Uuid and Decimal must come from their original crates (not sea_orm re-exports)
    // because the sea_orm re-exports don't implement JsonSchema.
    let needs_uuid = fields
        .iter()
        .any(|f| f.normalized_type == NormalizedType::Uuid);

    let needs_decimal = fields
        .iter()
        .any(|f| f.normalized_type == NormalizedType::Decimal);

    let sea_orm_types: Vec<String> = fields
        .iter()
        .filter_map(|f| {
            f.normalized_type
                .sea_orm_import_name()
                .map(|s| s.to_string())
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut extra_imports = Vec::new();
    if needs_uuid {
        extra_imports.push("use rapina::uuid::Uuid;".to_string());
    }
    if needs_decimal {
        extra_imports.push("use rapina::rust_decimal::Decimal;".to_string());
    }
    if !sea_orm_types.is_empty() {
        extra_imports.push(format!(
            "use rapina::sea_orm::prelude::{{{}}};",
            sea_orm_types.join(", ")
        ));
    }

    let extra_import = if extra_imports.is_empty() {
        String::new()
    } else {
        format!("{}\n", extra_imports.join("\n"))
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
    primary_key: Option<&[String]>,
) -> String {
    let schema_fields: Vec<String> = fields
        .iter()
        .map(|f| format!("        {}: {},", f.name, f.schema_type_name()))
        .collect();

    let mut attrs = String::new();

    if let Some(pk_cols) = primary_key {
        attrs.push_str(&format!("\n    #[primary_key({})]", pk_cols.join(", ")));
    }

    if let Some(ts) = timestamps {
        attrs.push_str(&format!("\n    #[timestamps({})]", ts));
    }

    format!(
        r#"
schema! {{
    {attrs}
    {pascal} {{
{fields}
    }}
}}
"#,
        pascal = pascal,
        attrs = attrs,
        fields = schema_fields.join("\n"),
    )
}

pub(crate) fn generate_migration(
    plural: &str,
    pascal_plural: &str,
    fields: &[FieldInfo],
    with_timestamps: bool,
) -> String {
    let (mut column_defs, mut iden_variants): (Vec<String>, Vec<String>) = fields
        .iter()
        .map(|f| {
            (
                format!("                    {}", f.generate_column(pascal_plural)),
                format!("    {},", f.ident),
            )
        })
        .unzip();

    if with_timestamps {
        column_defs.push(format!(
            "                    .col(ColumnDef::new({pascal_plural}::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))"
        ));
        column_defs.push(format!(
            "                    .col(ColumnDef::new({pascal_plural}::UpdatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))"
        ));
        iden_variants.push("    CreatedAt,".to_string());
        iden_variants.push("    UpdatedAt,".to_string());
    }

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
{iden_variants}
}}
"#,
        readable_name = readable_name,
        pascal_plural = pascal_plural,
        column_defs = column_defs.join("\n"),
        iden_variants = iden_variants.join("\n"),
    )
}

pub(crate) fn remove_schema_block(content: &str, entity_name: &str) -> String {
    let mut result = String::new();
    let mut lines = content.lines().peekable();
    let entity_pattern = format!("{} {{", entity_name);

    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("schema! {") {
            // Collect the entire schema block
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

            // Check if this block contains our entity
            let block_text = block_lines.join("\n");
            if !block_text.contains(&entity_pattern) {
                result.push_str(&block_text);
                result.push('\n');
            }
            // else: skip the block entirely
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    // Clean up excessive blank lines left behind
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }

    result
}

pub(crate) fn update_entity_file(
    pascal: &str,
    fields: &[FieldInfo],
    timestamps: Option<&str>,
    primary_key: Option<&[String]>,
    force: bool,
) -> Result<(), String> {
    update_entity_file_in(
        pascal,
        fields,
        timestamps,
        primary_key,
        force,
        Path::new("src/entity.rs"),
    )
}

fn update_entity_file_in(
    pascal: &str,
    fields: &[FieldInfo],
    timestamps: Option<&str>,
    primary_key: Option<&[String]>,
    force: bool,
    entity_path: &Path,
) -> Result<(), String> {
    let schema_block = generate_schema_block(pascal, fields, timestamps, primary_key);

    if entity_path.exists() {
        let mut content = fs::read_to_string(entity_path)
            .map_err(|e| format!("Failed to read entity.rs: {}", e))?;

        if force {
            content = remove_schema_block(&content, pascal);
        }

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
    with_timestamps: bool,
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

    let template = generate_migration(plural, pascal_plural, fields, with_timestamps);
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
    pk_type: &NormalizedType,
    force: bool,
) -> Result<(), String> {
    create_feature_module_in(
        singular,
        plural,
        pascal,
        fields,
        pk_type,
        force,
        Path::new("src"),
    )
}

fn create_feature_module_in(
    singular: &str,
    plural: &str,
    pascal: &str,
    fields: &[FieldInfo],
    pk_type: &NormalizedType,
    force: bool,
    base: &Path,
) -> Result<(), String> {
    let module_dir = base.join(plural);

    if module_dir.exists() {
        if !force {
            return Err(format!(
                "Directory 'src/{}/' already exists. Remove it first, choose a different resource name, or use --force to overwrite.",
                plural
            ));
        }
        fs::remove_dir_all(&module_dir)
            .map_err(|e| format!("Failed to remove existing directory: {}", e))?;
        println!(
            "  {} Removed existing {}",
            "↻".yellow(),
            format!("src/{}/", plural).cyan()
        );
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
        generate_handlers(singular, plural, pascal, fields, pk_type),
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

/// Inserts `mod <name>;` declarations into `src/main.rs` for any modules not
/// already declared. Silently returns Ok if main.rs does not exist.
pub(crate) fn wire_main_rs(modules: &[&str], project_root: &Path) -> Result<(), String> {
    let main_path = project_root.join("src").join("main.rs");
    if !main_path.exists() {
        return Ok(());
    }

    let content =
        fs::read_to_string(&main_path).map_err(|e| format!("Failed to read main.rs: {e}"))?;

    // Filter out modules already declared.
    let new_modules: Vec<&str> = modules
        .iter()
        .copied()
        .filter(|m| {
            !content.lines().any(|l| {
                l.trim_start().starts_with("mod ")
                    && l.trim_end().ends_with(';')
                    && l.contains(&format!("mod {m};"))
            })
        })
        .collect();

    if new_modules.is_empty() {
        return Ok(());
    }

    let lines: Vec<&str> = content.lines().collect();

    // Find the index of the last `mod ...;` line.
    let last_mod_idx = lines
        .iter()
        .rposition(|l| l.trim_start().starts_with("mod ") && l.trim_end().ends_with(';'));

    let insertion_line = match last_mod_idx {
        Some(idx) => idx + 1,
        None => {
            // No existing mod declarations — insert before `fn main` or `#[tokio::main]`.
            lines
                .iter()
                .position(|l| l.contains("fn main") || l.contains("#[tokio::main]"))
                .unwrap_or(lines.len())
        }
    };

    let mut result = lines[..insertion_line].join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    for m in &new_modules {
        result.push_str(&format!("mod {m};\n"));
    }
    if insertion_line < lines.len() {
        result.push_str(&lines[insertion_line..].join("\n"));
        if content.ends_with('\n') {
            result.push('\n');
        }
    }

    fs::write(&main_path, &result).map_err(|e| format!("Failed to write main.rs: {e}"))?;

    for m in &new_modules {
        println!(
            "  {} Wired {} in {}",
            "✓".green(),
            format!("mod {m};").cyan(),
            "src/main.rs".cyan()
        );
    }

    Ok(())
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use tempfile::TempDir;

    fn write_main(dir: &TempDir, content: &str) -> std::path::PathBuf {
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let path = src.join("main.rs");
        std::fs::write(&path, content).unwrap();
        dir.path().to_path_buf()
    }

    #[test]
    fn inserts_after_last_mod() {
        let dir = TempDir::new().unwrap();
        let root = write_main(
            &dir,
            "\
use rapina::prelude::*;

mod entity;
mod migrations;

#[tokio::main]
async fn main() {}
",
        );
        wire_main_rs(&["todos"], &root).unwrap();
        let content = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(content.contains("mod todos;"));
        let mod_pos = content.find("mod todos;").unwrap();
        let main_pos = content.find("#[tokio::main]").unwrap();
        assert!(mod_pos < main_pos);
    }

    #[test]
    fn skips_duplicate() {
        let dir = TempDir::new().unwrap();
        let root = write_main(
            &dir,
            "\
mod entity;
mod todos;

fn main() {}
",
        );
        wire_main_rs(&["todos"], &root).unwrap();
        let content = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert_eq!(content.matches("mod todos;").count(), 1);
    }

    #[test]
    fn inserts_multiple_modules() {
        let dir = TempDir::new().unwrap();
        let root = write_main(
            &dir,
            "\
mod entity;
mod migrations;

fn main() {}
",
        );
        wire_main_rs(&["users", "posts"], &root).unwrap();
        let content = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(content.contains("mod users;"));
        assert!(content.contains("mod posts;"));
    }

    #[test]
    fn no_main_rs_is_silent() {
        let dir = TempDir::new().unwrap();
        let result = wire_main_rs(&["todos"], dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn no_existing_mods_inserts_before_fn_main() {
        let dir = TempDir::new().unwrap();
        let root = write_main(
            &dir,
            "\
use rapina::prelude::*;

fn main() {}
",
        );
        wire_main_rs(&["todos"], &root).unwrap();
        let content = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(content.contains("mod todos;"));
        let mod_pos = content.find("mod todos;").unwrap();
        let main_pos = content.find("fn main()").unwrap();
        assert!(mod_pos < main_pos);
    }

    #[test]
    fn no_double_blank_line() {
        let dir = TempDir::new().unwrap();
        let root = write_main(&dir, "mod entity;\nmod migrations;\n\nfn main() {}\n");
        wire_main_rs(&["todos"], &root).unwrap();
        let content = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        assert!(
            !content.contains("\n\n\n"),
            "triple newline found (double blank line)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_singularize() {
        // Regular plurals (already working)
        assert_eq!(singularize("users"), "user");
        assert_eq!(singularize("posts"), "post");
        assert_eq!(singularize("categories"), "category");
        assert_eq!(singularize("addresses"), "address");
        assert_eq!(singularize("boxes"), "box");
        assert_eq!(singularize("buzzes"), "buzz");
        assert_eq!(singularize("boss"), "boss");
        assert_eq!(singularize("buses"), "bus");
        assert_eq!(singularize("watches"), "watch");
        assert_eq!(singularize("bushes"), "bush");

        // Irregular plurals
        assert_eq!(singularize("people"), "person");
        assert_eq!(singularize("children"), "child");
        assert_eq!(singularize("men"), "man");
        assert_eq!(singularize("women"), "woman");
        assert_eq!(singularize("mice"), "mouse");
        assert_eq!(singularize("geese"), "goose");
        assert_eq!(singularize("teeth"), "tooth");
        assert_eq!(singularize("feet"), "foot");
        assert_eq!(singularize("oxen"), "ox");
        assert_eq!(singularize("leaves"), "leaf");
        assert_eq!(singularize("lives"), "life");
        assert_eq!(singularize("knives"), "knife");
        assert_eq!(singularize("wives"), "wife");
        assert_eq!(singularize("halves"), "half");
        assert_eq!(singularize("wolves"), "wolf");
        assert_eq!(singularize("shelves"), "shelf");
        assert_eq!(singularize("loaves"), "loaf");

        // Latin/Greek-origin plurals common in tech
        assert_eq!(singularize("data"), "datum");
        assert_eq!(singularize("media"), "medium");
        assert_eq!(singularize("criteria"), "criterion");
        assert_eq!(singularize("phenomena"), "phenomenon");
        assert_eq!(singularize("indices"), "index");
        assert_eq!(singularize("vertices"), "vertex");
        assert_eq!(singularize("matrices"), "matrix");
        assert_eq!(singularize("appendices"), "appendix");
        assert_eq!(singularize("analyses"), "analysis");
        assert_eq!(singularize("bases"), "base");
        assert_eq!(singularize("crises"), "crisis");
        assert_eq!(singularize("theses"), "thesis");
        assert_eq!(singularize("diagnoses"), "diagnosis");
        assert_eq!(singularize("hypotheses"), "hypothesis");
        assert_eq!(singularize("parentheses"), "parenthesis");
        assert_eq!(singularize("synopses"), "synopsis");
        assert_eq!(singularize("curricula"), "curriculum");
        assert_eq!(singularize("formulae"), "formula");
        assert_eq!(singularize("antennae"), "antenna");
        assert_eq!(singularize("alumni"), "alumnus");
        assert_eq!(singularize("cacti"), "cactus");
        assert_eq!(singularize("fungi"), "fungus");
        assert_eq!(singularize("nuclei"), "nucleus");
        assert_eq!(singularize("radii"), "radius");
        assert_eq!(singularize("stimuli"), "stimulus");
        assert_eq!(singularize("syllabi"), "syllabus");

        // Words ending in -us (should NOT strip the s)
        assert_eq!(singularize("statuses"), "status");
        assert_eq!(singularize("status"), "status");
        assert_eq!(singularize("campus"), "campus");
        assert_eq!(singularize("virus"), "virus");
        assert_eq!(singularize("census"), "census");
        assert_eq!(singularize("corpus"), "corpus");
        assert_eq!(singularize("opus"), "opus");
        assert_eq!(singularize("genus"), "genus");
        assert_eq!(singularize("apparatus"), "apparatus");
        assert_eq!(singularize("nexus"), "nexus");
        assert_eq!(singularize("prospectus"), "prospectus");
        assert_eq!(singularize("consensus"), "consensus");

        // Uncountable / identity words
        assert_eq!(singularize("series"), "series");
        assert_eq!(singularize("species"), "species");
        assert_eq!(singularize("news"), "news");
        assert_eq!(singularize("info"), "info");
        assert_eq!(singularize("metadata"), "metadata");
        assert_eq!(singularize("sheep"), "sheep");
        assert_eq!(singularize("fish"), "fish");
        assert_eq!(singularize("deer"), "deer");
        assert_eq!(singularize("aircraft"), "aircraft");
        assert_eq!(singularize("software"), "software");
        assert_eq!(singularize("hardware"), "hardware");
        assert_eq!(singularize("firmware"), "firmware");
        assert_eq!(singularize("middleware"), "middleware");
        assert_eq!(singularize("equipment"), "equipment");
        assert_eq!(singularize("feedback"), "feedback");
        assert_eq!(singularize("moose"), "moose");
        assert_eq!(singularize("bison"), "bison");
        assert_eq!(singularize("trout"), "trout");
        assert_eq!(singularize("salmon"), "salmon");
        assert_eq!(singularize("shrimp"), "shrimp");

        // Already singular — should be idempotent
        assert_eq!(singularize("user"), "user");
        assert_eq!(singularize("post"), "post");
        assert_eq!(singularize("category"), "category");
        assert_eq!(singularize("person"), "person");
        assert_eq!(singularize("child"), "child");
    }

    #[test]
    fn test_pluralize() {
        // Regular plurals (already working)
        assert_eq!(pluralize("user"), "users");
        assert_eq!(pluralize("post"), "posts");
        assert_eq!(pluralize("category"), "categories");
        assert_eq!(pluralize("address"), "addresses");
        assert_eq!(pluralize("box"), "boxes");
        assert_eq!(pluralize("buzz"), "buzzes");
        assert_eq!(pluralize("boss"), "bosses");
        assert_eq!(pluralize("monkey"), "monkeys");
        assert_eq!(pluralize("boy"), "boys");
        assert_eq!(pluralize("day"), "days");
        assert_eq!(pluralize("guy"), "guys");
        assert_eq!(pluralize("watch"), "watches");
        assert_eq!(pluralize("bush"), "bushes");
        assert_eq!(pluralize("bus"), "buses");

        // Irregular plurals
        assert_eq!(pluralize("person"), "people");
        assert_eq!(pluralize("child"), "children");
        assert_eq!(pluralize("man"), "men");
        assert_eq!(pluralize("woman"), "women");
        assert_eq!(pluralize("mouse"), "mice");
        assert_eq!(pluralize("goose"), "geese");
        assert_eq!(pluralize("tooth"), "teeth");
        assert_eq!(pluralize("foot"), "feet");
        assert_eq!(pluralize("ox"), "oxen");
        assert_eq!(pluralize("leaf"), "leaves");
        assert_eq!(pluralize("life"), "lives");
        assert_eq!(pluralize("knife"), "knives");
        assert_eq!(pluralize("wife"), "wives");
        assert_eq!(pluralize("half"), "halves");
        assert_eq!(pluralize("wolf"), "wolves");
        assert_eq!(pluralize("shelf"), "shelves");
        assert_eq!(pluralize("loaf"), "loaves");

        // Latin/Greek-origin
        assert_eq!(pluralize("datum"), "data");
        assert_eq!(pluralize("medium"), "media");
        assert_eq!(pluralize("criterion"), "criteria");
        assert_eq!(pluralize("phenomenon"), "phenomena");
        assert_eq!(pluralize("index"), "indices");
        assert_eq!(pluralize("vertex"), "vertices");
        assert_eq!(pluralize("matrix"), "matrices");
        assert_eq!(pluralize("appendix"), "appendices");
        assert_eq!(pluralize("analysis"), "analyses");
        assert_eq!(pluralize("base"), "bases");
        assert_eq!(pluralize("crisis"), "crises");
        assert_eq!(pluralize("thesis"), "theses");
        assert_eq!(pluralize("diagnosis"), "diagnoses");
        assert_eq!(pluralize("hypothesis"), "hypotheses");
        assert_eq!(pluralize("parenthesis"), "parentheses");
        assert_eq!(pluralize("synopsis"), "synopses");
        assert_eq!(pluralize("curriculum"), "curricula");
        assert_eq!(pluralize("formula"), "formulae");
        assert_eq!(pluralize("antenna"), "antennae");
        assert_eq!(pluralize("alumnus"), "alumni");
        assert_eq!(pluralize("cactus"), "cacti");
        assert_eq!(pluralize("fungus"), "fungi");
        assert_eq!(pluralize("nucleus"), "nuclei");
        assert_eq!(pluralize("radius"), "radii");
        assert_eq!(pluralize("stimulus"), "stimuli");
        assert_eq!(pluralize("syllabus"), "syllabi");

        // Words ending in -us
        assert_eq!(pluralize("status"), "statuses");
        assert_eq!(pluralize("campus"), "campuses");
        assert_eq!(pluralize("virus"), "viruses");
        assert_eq!(pluralize("census"), "censuses");
        assert_eq!(pluralize("corpus"), "corpuses");
        assert_eq!(pluralize("opus"), "opuses");
        assert_eq!(pluralize("genus"), "genuses");
        assert_eq!(pluralize("apparatus"), "apparatuses");
        assert_eq!(pluralize("nexus"), "nexuses");
        assert_eq!(pluralize("prospectus"), "prospectuses");
        assert_eq!(pluralize("consensus"), "consensuses");

        // Uncountable / identity words
        assert_eq!(pluralize("series"), "series");
        assert_eq!(pluralize("species"), "species");
        assert_eq!(pluralize("news"), "news");
        assert_eq!(pluralize("info"), "info");
        assert_eq!(pluralize("metadata"), "metadata");
        assert_eq!(pluralize("sheep"), "sheep");
        assert_eq!(pluralize("fish"), "fish");
        assert_eq!(pluralize("deer"), "deer");
        assert_eq!(pluralize("aircraft"), "aircraft");
        assert_eq!(pluralize("software"), "software");
        assert_eq!(pluralize("hardware"), "hardware");
        assert_eq!(pluralize("firmware"), "firmware");
        assert_eq!(pluralize("middleware"), "middleware");
        assert_eq!(pluralize("equipment"), "equipment");
        assert_eq!(pluralize("feedback"), "feedback");
        assert_eq!(pluralize("moose"), "moose");
        assert_eq!(pluralize("bison"), "bison");
        assert_eq!(pluralize("trout"), "trout");
        assert_eq!(pluralize("salmon"), "salmon");
        assert_eq!(pluralize("shrimp"), "shrimp");

        // Already plural (irregular) — should be idempotent
        assert_eq!(pluralize("people"), "people");
        assert_eq!(pluralize("children"), "children");
        assert_eq!(pluralize("men"), "men");
        assert_eq!(pluralize("data"), "data");
        assert_eq!(pluralize("indices"), "indices");
    }

    #[test]
    fn test_generate_handlers() {
        let fields = vec![
            "title:string".parse().unwrap(),
            "active:bool".parse().unwrap(),
        ];
        let content = generate_handlers("post", "posts", "Post", &fields, &NormalizedType::I32);

        assert!(content.contains("pub async fn list_posts"));
        assert!(content.contains("pub async fn get_post"));
        assert!(content.contains("pub async fn create_post"));
        assert!(content.contains("pub async fn update_post"));
        assert!(content.contains("pub async fn delete_post"));
        assert!(content.contains("title: Set(input.title),"));
        assert!(content.contains("active: Set(input.active),"));
    }

    #[test]
    fn test_generate_dto() {
        let fields = vec!["name:string".parse().unwrap(), "age:i32".parse().unwrap()];
        let content = generate_dto("User", &fields);

        assert!(content.contains("pub struct CreateUser"));
        assert!(content.contains("pub struct UpdateUser"));
        assert!(content.contains("pub name: String,"));
        assert!(content.contains("pub age: i32,"));
    }

    #[test]
    fn test_generate_dto_nullable_fields() {
        let mut fields = vec![
            "title:string".parse::<FieldInfo>().unwrap(),
            "bio:string".parse::<FieldInfo>().unwrap(),
        ];
        fields[1].nullable = true;

        let content = generate_dto("User", &fields);

        assert!(content.contains("pub title: String,"));
        assert!(content.contains("pub bio: Option<String>,"));
    }

    #[test]
    fn test_generate_dto_uuid_decimal_imports() {
        let fields = vec!["id:uuid".parse().unwrap(), "price:decimal".parse().unwrap()];
        let content = generate_dto("Product", &fields);

        assert!(content.contains("use rapina::uuid::Uuid;"));
        assert!(content.contains("use rapina::rust_decimal::Decimal;"));
    }

    #[test]
    fn test_generate_dto_sea_orm_types_import() {
        let mut fields = vec![
            "created_at:datetimeutc".parse::<FieldInfo>().unwrap(),
            "metadata:json".parse::<FieldInfo>().unwrap(),
        ];
        fields[1].nullable = true;

        let content = generate_dto("Event", &fields);

        assert!(content.contains("use rapina::sea_orm::prelude::{DateTimeUtc, Json};"));
    }

    #[test]
    fn test_generate_schema_block() {
        let fields = vec![
            "title:string".parse().unwrap(),
            "done:bool".parse().unwrap(),
        ];
        let content = generate_schema_block("Todo", &fields, None, None);

        assert!(content.contains("title: String,"));
        assert!(content.contains("done: bool,"));
    }

    #[test]
    fn test_generate_migration() {
        let fields = vec![
            "title:string".parse().unwrap(),
            "published:bool".parse().unwrap(),
        ];
        let content = generate_migration("posts", "Posts", &fields, false);

        assert!(content.contains(".string().not_null()"));
        assert!(content.contains(".boolean().default(Expr::value(false)).not_null()"));
    }

    #[test]
    fn test_generate_schema_block_with_timestamps() {
        let fields = vec!["title:string".parse().unwrap()];

        let block = generate_schema_block("Post", &fields, None, None);
        assert!(block.contains("schema! {"));
        assert!(block.contains("Post {"));
        assert!(block.contains("title: String,"));
        assert!(!block.contains("#[timestamps"));

        let block = generate_schema_block("Post", &fields, Some("none"), None);
        assert!(block.contains("#[timestamps(none)]"));

        let block = generate_schema_block("Post", &fields, Some("created_at"), None);
        assert!(block.contains("#[timestamps(created_at)]"));
    }

    #[test]
    fn test_generate_schema_block_with_primary_key() {
        let fields = vec![
            "user_id:i32".parse().unwrap(),
            "role_id:i32".parse().unwrap(),
        ];

        let pk = vec!["user_id".to_string(), "role_id".to_string()];
        let block = generate_schema_block("UsersRole", &fields, Some("none"), Some(&pk));
        assert!(block.contains("#[primary_key(user_id, role_id)]"));
        assert!(block.contains("#[timestamps(none)]"));
        assert!(block.contains("user_id: i32,"));
        assert!(block.contains("role_id: i32,"));
    }

    #[test]
    fn test_remove_schema_block_removes_matching_entity() {
        let content = r#"use rapina::prelude::*;

schema! {
    Post {
        title: String,
    }
}

schema! {
    Comment {
        body: String,
    }
}
"#;
        let result = remove_schema_block(content, "Post");
        assert!(!result.contains("Post {"));
        assert!(result.contains("Comment {"));
        assert!(result.contains("schema! {"));
    }

    #[test]
    fn test_remove_schema_block_no_match_returns_unchanged() {
        let content = r#"use rapina::prelude::*;

schema! {
    Post {
        title: String,
    }
}
"#;
        let result = remove_schema_block(content, "User");
        assert_eq!(result.trim(), content.trim());
    }

    #[test]
    fn test_create_feature_module_errors_without_force_when_exists() {
        let dir = tempfile::tempdir().unwrap();
        let module_dir = dir.path().join("users");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(module_dir.join("mod.rs"), "old content").unwrap();

        let fields = vec!["email:string".parse().unwrap()];

        let result = create_feature_module_in(
            "user",
            "users",
            "User",
            &fields,
            &NormalizedType::I32,
            false,
            dir.path(),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already exists"));
    }

    #[test]
    fn test_create_feature_module_overwrites_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let module_dir = dir.path().join("users");
        fs::create_dir_all(&module_dir).unwrap();
        fs::write(module_dir.join("mod.rs"), "old content").unwrap();

        let fields = vec!["email:string".parse().unwrap()];

        let result = create_feature_module_in(
            "user",
            "users",
            "User",
            &fields,
            &NormalizedType::I32,
            true,
            dir.path(),
        );
        assert!(result.is_ok());
        let mod_content = fs::read_to_string(module_dir.join("mod.rs")).unwrap();
        assert!(mod_content.contains("pub mod"));
    }

    #[test]
    fn test_update_entity_file_deduplicates_with_force() {
        let dir = tempfile::tempdir().unwrap();
        let entity_path = dir.path().join("entity.rs");
        fs::write(
            &entity_path,
            r#"use rapina::prelude::*;

schema! {
    Post {
        title: String,
    }
}
"#,
        )
        .unwrap();

        let fields = vec!["title:string".parse().unwrap()];

        update_entity_file_in("Post", &fields, None, None, true, &entity_path).unwrap();
        let content = fs::read_to_string(&entity_path).unwrap();
        // Should have exactly one schema! block for Post, not two
        assert_eq!(content.matches("Post {").count(), 1);
        assert!(content.contains("schema! {"));
    }

    #[test]
    fn test_update_entity_file_appends_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let entity_path = dir.path().join("entity.rs");
        fs::write(
            &entity_path,
            r#"use rapina::prelude::*;

schema! {
    Post {
        title: String,
    }
}
"#,
        )
        .unwrap();

        let fields = vec!["title:string".parse().unwrap()];

        update_entity_file_in("Post", &fields, None, None, false, &entity_path).unwrap();
        let content = fs::read_to_string(&entity_path).unwrap();
        // Without force, should have two schema! blocks (duplicate)
        assert_eq!(content.matches("Post {").count(), 2);
    }

    #[test]
    fn test_remove_schema_block_with_attributes() {
        let content = r#"use rapina::prelude::*;

schema! {
    #[primary_key(user_id, role_id)]
    #[timestamps(none)]
    UsersRole {
        user_id: i32,
        role_id: i32,
    }
}
"#;
        let result = remove_schema_block(content, "UsersRole");
        assert!(!result.contains("UsersRole"));
        assert!(!result.contains("schema!"));
    }
}
