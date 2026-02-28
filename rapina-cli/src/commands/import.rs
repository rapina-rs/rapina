use std::collections::HashMap;

use colored::Colorize;

use super::codegen::{self, FieldInfo};

// ---------------------------------------------------------------------------
// Intermediate representation
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct IntrospectedTable {
    name: String,
    columns: Vec<IntrospectedColumn>,
    primary_key_columns: Vec<String>,
    foreign_keys: Vec<IntrospectedForeignKey>,
}

#[derive(Debug)]
struct IntrospectedColumn {
    name: String,
    col_type: NormalizedType,
    is_nullable: bool,
}

#[derive(Debug)]
struct IntrospectedForeignKey {
    columns: Vec<String>,
    referenced_table: String,
    #[allow(dead_code)]
    referenced_columns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum NormalizedType {
    Str,
    Text,
    I32,
    I64,
    F32,
    F64,
    Bool,
    Uuid,
    DateTimeUtc,
    NaiveDateTime,
    Date,
    Decimal,
    Json,
    Unmappable(String),
}

// ---------------------------------------------------------------------------
// Type mappers
// ---------------------------------------------------------------------------

#[cfg(feature = "import-postgres")]
fn map_pg_type(col_type: &sea_schema::postgres::def::Type) -> NormalizedType {
    use sea_schema::postgres::def::Type;
    match col_type {
        Type::SmallInt | Type::Integer | Type::Serial | Type::SmallSerial => NormalizedType::I32,
        Type::BigInt | Type::BigSerial => NormalizedType::I64,
        Type::Real => NormalizedType::F32,
        Type::DoublePrecision => NormalizedType::F64,
        Type::Money => NormalizedType::Decimal,
        Type::Varchar(_) | Type::Char(_) => NormalizedType::Str,
        Type::Text => NormalizedType::Text,
        Type::Bytea => NormalizedType::Unmappable("bytea".to_string()),
        Type::Boolean => NormalizedType::Bool,
        Type::Uuid => NormalizedType::Uuid,
        Type::TimestampWithTimeZone(_) => NormalizedType::DateTimeUtc,
        Type::Timestamp(_) => NormalizedType::NaiveDateTime,
        Type::Date => NormalizedType::Date,
        Type::Decimal(_) | Type::Numeric(_) => NormalizedType::Decimal,
        Type::Json | Type::JsonBinary => NormalizedType::Json,
        other => NormalizedType::Unmappable(format!("{:?}", other)),
    }
}

#[cfg(feature = "import-mysql")]
fn map_mysql_type(col_type: &sea_schema::mysql::def::Type) -> NormalizedType {
    use sea_schema::mysql::def::Type;
    match col_type {
        Type::TinyInt(_) | Type::SmallInt(_) | Type::MediumInt(_) | Type::Int(_) => {
            NormalizedType::I32
        }
        Type::BigInt(_) | Type::Serial => NormalizedType::I64,
        Type::Float(_) => NormalizedType::F32,
        Type::Double(_) => NormalizedType::F64,
        Type::Char(_) | Type::NChar(_) | Type::Varchar(_) | Type::NVarchar(_) => {
            NormalizedType::Str
        }
        Type::Text(_) | Type::TinyText(_) | Type::MediumText(_) | Type::LongText(_) => {
            NormalizedType::Text
        }
        Type::Bool => NormalizedType::Bool,
        Type::Timestamp(_) => NormalizedType::DateTimeUtc,
        Type::DateTime(_) => NormalizedType::NaiveDateTime,
        Type::Date => NormalizedType::Date,
        Type::Decimal(_) => NormalizedType::Decimal,
        Type::Json => NormalizedType::Json,
        other => NormalizedType::Unmappable(format!("{:?}", other)),
    }
}

#[cfg(feature = "import-sqlite")]
fn map_sqlite_type(col_type: &sea_schema::sea_query::ColumnType) -> NormalizedType {
    use sea_schema::sea_query::ColumnType;
    match col_type {
        ColumnType::TinyInteger | ColumnType::SmallInteger | ColumnType::Integer => {
            NormalizedType::I32
        }
        ColumnType::BigInteger => NormalizedType::I64,
        ColumnType::Float => NormalizedType::F32,
        ColumnType::Double => NormalizedType::F64,
        ColumnType::String(_) | ColumnType::Char(_) => NormalizedType::Str,
        ColumnType::Text => NormalizedType::Text,
        ColumnType::Boolean => NormalizedType::Bool,
        ColumnType::Uuid => NormalizedType::Uuid,
        ColumnType::TimestampWithTimeZone => NormalizedType::DateTimeUtc,
        ColumnType::DateTime | ColumnType::Timestamp => NormalizedType::NaiveDateTime,
        ColumnType::Date => NormalizedType::Date,
        ColumnType::Decimal(_) | ColumnType::Money(_) => NormalizedType::Decimal,
        ColumnType::Json | ColumnType::JsonBinary => NormalizedType::Json,
        other => NormalizedType::Unmappable(format!("{:?}", other)),
    }
}

// ---------------------------------------------------------------------------
// NormalizedType -> FieldInfo conversion
// ---------------------------------------------------------------------------

fn normalized_to_field_info(
    col_name: &str,
    col_type: &NormalizedType,
    is_nullable: bool,
) -> Option<FieldInfo> {
    let null_suffix = if is_nullable {
        ".null()"
    } else {
        ".not_null()"
    };

    let (rust_type, schema_type, column_base) = match col_type {
        NormalizedType::Str => ("String", "String", ".string()"),
        NormalizedType::Text => ("String", "Text", ".text()"),
        NormalizedType::I32 => ("i32", "i32", ".integer()"),
        NormalizedType::I64 => ("i64", "i64", ".big_integer()"),
        NormalizedType::F32 => ("f32", "f32", ".float()"),
        NormalizedType::F64 => ("f64", "f64", ".double()"),
        NormalizedType::Bool => ("bool", "bool", ".boolean()"),
        NormalizedType::Uuid => ("Uuid", "Uuid", ".uuid()"),
        NormalizedType::DateTimeUtc => ("DateTimeUtc", "DateTime", ".timestamp_with_time_zone()"),
        NormalizedType::NaiveDateTime => ("DateTime", "NaiveDateTime", ".date_time()"),
        NormalizedType::Date => ("Date", "Date", ".date()"),
        NormalizedType::Decimal => ("Decimal", "Decimal", ".decimal()"),
        NormalizedType::Json => ("Json", "Json", ".json()"),
        NormalizedType::Unmappable(_) => return None,
    };

    Some(FieldInfo {
        name: col_name.to_string(),
        rust_type: rust_type.to_string(),
        schema_type: schema_type.to_string(),
        column_method: format!("{}{}", column_base, null_suffix),
    })
}

// ---------------------------------------------------------------------------
// Backend introspection
// ---------------------------------------------------------------------------

#[cfg(feature = "import-postgres")]
async fn introspect_postgres(
    url: &str,
    schema_name: &str,
) -> Result<Vec<IntrospectedTable>, String> {
    let pool = sqlx::PgPool::connect(url)
        .await
        .map_err(|e| format!("Failed to connect to Postgres: {}", e))?;

    let discovery = sea_schema::postgres::discovery::SchemaDiscovery::new(pool, schema_name);
    let schema = discovery
        .discover()
        .await
        .map_err(|e| format!("Failed to discover schema: {}", e))?;

    let mut tables = Vec::new();
    for table_def in schema.tables {
        let pk_columns: Vec<String> = table_def
            .primary_key_constraints
            .iter()
            .flat_map(|pk| pk.columns.iter().cloned())
            .collect();

        let foreign_keys: Vec<IntrospectedForeignKey> = table_def
            .reference_constraints
            .iter()
            .map(|fk| IntrospectedForeignKey {
                columns: fk.columns.clone(),
                referenced_table: fk.table.clone(),
                referenced_columns: fk.foreign_columns.clone(),
            })
            .collect();

        let columns: Vec<IntrospectedColumn> = table_def
            .columns
            .iter()
            .map(|col| IntrospectedColumn {
                name: col.name.clone(),
                col_type: map_pg_type(&col.col_type),
                is_nullable: col.not_null.is_none(),
            })
            .collect();

        tables.push(IntrospectedTable {
            name: table_def.info.name.clone(),
            columns,
            primary_key_columns: pk_columns,
            foreign_keys,
        });
    }

    Ok(tables)
}

#[cfg(feature = "import-mysql")]
async fn introspect_mysql(url: &str, schema_name: &str) -> Result<Vec<IntrospectedTable>, String> {
    let pool = sqlx::MySqlPool::connect(url)
        .await
        .map_err(|e| format!("Failed to connect to MySQL: {}", e))?;

    let discovery = sea_schema::mysql::discovery::SchemaDiscovery::new(pool, schema_name);
    let schema = discovery
        .discover()
        .await
        .map_err(|e| format!("Failed to discover schema: {}", e))?;

    let mut tables = Vec::new();
    for table_def in schema.tables {
        let pk_columns: Vec<String> = table_def
            .columns
            .iter()
            .filter(|col| col.key == sea_schema::mysql::def::ColumnKey::Primary)
            .map(|col| col.name.clone())
            .collect();

        let foreign_keys: Vec<IntrospectedForeignKey> = table_def
            .foreign_keys
            .iter()
            .map(|fk| IntrospectedForeignKey {
                columns: fk.columns.clone(),
                referenced_table: fk.referenced_table.clone(),
                referenced_columns: fk.referenced_columns.clone(),
            })
            .collect();

        let columns: Vec<IntrospectedColumn> = table_def
            .columns
            .iter()
            .map(|col| IntrospectedColumn {
                name: col.name.clone(),
                col_type: map_mysql_type(&col.col_type),
                is_nullable: col.null,
            })
            .collect();

        tables.push(IntrospectedTable {
            name: table_def.info.name.clone(),
            columns,
            primary_key_columns: pk_columns,
            foreign_keys,
        });
    }

    Ok(tables)
}

#[cfg(feature = "import-sqlite")]
async fn introspect_sqlite(url: &str) -> Result<Vec<IntrospectedTable>, String> {
    let pool = sqlx::SqlitePool::connect(url)
        .await
        .map_err(|e| format!("Failed to connect to SQLite: {}", e))?;

    let discovery = sea_schema::sqlite::discovery::SchemaDiscovery::new(pool);
    let schema: sea_schema::sqlite::def::Schema = discovery
        .discover()
        .await
        .map_err(|e| format!("Failed to discover schema: {}", e))?;

    let mut tables = Vec::new();
    for table_def in schema.tables {
        let pk_columns: Vec<String> = table_def
            .columns
            .iter()
            .filter(|col| col.primary_key)
            .map(|col| col.name.clone())
            .collect();

        // SQLite ForeignKeysInfo fields are pub(crate), so we can't
        // extract FK details from outside the crate. FK resolution
        // is skipped for SQLite imports.
        let columns: Vec<IntrospectedColumn> = table_def
            .columns
            .iter()
            .map(|col| IntrospectedColumn {
                name: col.name.clone(),
                col_type: map_sqlite_type(&col.r#type),
                is_nullable: !col.not_null,
            })
            .collect();

        tables.push(IntrospectedTable {
            name: table_def.name.clone(),
            columns,
            primary_key_columns: pk_columns,
            foreign_keys: Vec::new(),
        });
    }

    Ok(tables)
}

// ---------------------------------------------------------------------------
// Filtering and validation
// ---------------------------------------------------------------------------

const INTERNAL_TABLES: &[&str] = &[
    "seaql_migrations",
    "sqlx_migrations",
    "__diesel_schema_migrations",
];

fn filter_and_validate_tables(
    tables: Vec<IntrospectedTable>,
    table_filter: Option<&[String]>,
) -> Vec<IntrospectedTable> {
    let mut result = Vec::new();

    for table in tables {
        // Skip internal / system tables
        if INTERNAL_TABLES.contains(&table.name.as_str()) || table.name.starts_with('_') {
            continue;
        }

        // Apply user filter
        if let Some(filter) = table_filter {
            if !filter.iter().any(|f| f == &table.name) {
                continue;
            }
        }

        // Must have a primary key
        if table.primary_key_columns.is_empty() {
            eprintln!(
                "  {} table {:?} skipped -- no primary key found",
                "warn:".yellow(),
                table.name
            );
            continue;
        }

        // For single PK: must be named "id" and be i32
        // For composite PK: all columns must be i32
        if table.primary_key_columns.len() == 1 {
            if table.primary_key_columns[0] != "id" {
                eprintln!(
                    "  {} table {:?} skipped -- PK column is {:?} (schema! requires column named \"id\" for single PK)",
                    "warn:".yellow(),
                    table.name,
                    table.primary_key_columns[0]
                );
                continue;
            }

            if let Some(pk_col) = table.columns.iter().find(|c| c.name == "id") {
                match &pk_col.col_type {
                    NormalizedType::I32 => {}
                    other => {
                        eprintln!(
                            "  {} table {:?} skipped -- PK is {:?} (schema! requires i32)",
                            "warn:".yellow(),
                            table.name,
                            other
                        );
                        continue;
                    }
                }
            }
        }

        result.push(table);
    }

    result
}

// ---------------------------------------------------------------------------
// FK relationship resolution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct RelationshipInfo {
    field_name: String,
    related_pascal: String,
    kind: RelationKind,
}

#[derive(Debug, Clone)]
enum RelationKind {
    BelongsTo,
    HasMany,
}

fn resolve_relationships(tables: &[IntrospectedTable]) -> HashMap<String, Vec<RelationshipInfo>> {
    let table_names: std::collections::HashSet<&str> =
        tables.iter().map(|t| t.name.as_str()).collect();
    let mut relationships: HashMap<String, Vec<RelationshipInfo>> = HashMap::new();

    for table in tables {
        for fk in &table.foreign_keys {
            // Only resolve if the referenced table is also being imported
            if !table_names.contains(fk.referenced_table.as_str()) {
                continue;
            }

            // Only handle single-column FKs (e.g., author_id -> users.id)
            if fk.columns.len() != 1 {
                continue;
            }

            let fk_column = &fk.columns[0];
            let field_name = fk_column.strip_suffix("_id").unwrap_or(fk_column);
            let ref_singular = codegen::singularize(&fk.referenced_table);
            let ref_pascal = codegen::to_pascal_case(&ref_singular);

            // BelongsTo on the FK side
            relationships
                .entry(table.name.clone())
                .or_default()
                .push(RelationshipInfo {
                    field_name: field_name.to_string(),
                    related_pascal: ref_pascal.clone(),
                    kind: RelationKind::BelongsTo,
                });

            // HasMany on the referenced side
            let owner_singular = codegen::singularize(&table.name);
            let owner_pascal = codegen::to_pascal_case(&owner_singular);
            relationships
                .entry(fk.referenced_table.clone())
                .or_default()
                .push(RelationshipInfo {
                    field_name: table.name.clone(),
                    related_pascal: owner_pascal,
                    kind: RelationKind::HasMany,
                });
        }
    }

    relationships
}

// ---------------------------------------------------------------------------
// Timestamp detection
// ---------------------------------------------------------------------------

fn detect_timestamps(table: &IntrospectedTable) -> Option<&'static str> {
    let has_created = table.columns.iter().any(|c| c.name == "created_at");
    let has_updated = table.columns.iter().any(|c| c.name == "updated_at");

    match (has_created, has_updated) {
        (true, true) => None, // default behavior, no attribute needed
        (true, false) => Some("created_at"),
        (false, true) => Some("updated_at"),
        (false, false) => Some("none"),
    }
}

// ---------------------------------------------------------------------------
// Per-table generation
// ---------------------------------------------------------------------------

fn generate_for_table(
    table: &IntrospectedTable,
    _relationships: &HashMap<String, Vec<RelationshipInfo>>,
) -> Result<(), String> {
    let singular = codegen::singularize(&table.name);
    let plural = &table.name;
    let pascal = codegen::to_pascal_case(&singular);
    let pascal_plural = codegen::to_pascal_case(plural);

    let is_composite_pk = table.primary_key_columns.len() > 1;

    // For composite PK, skip only timestamps. PK columns become regular fields.
    // For single PK, skip id and timestamps as before.
    let skip_columns: Vec<&str> = if is_composite_pk {
        vec!["created_at", "updated_at"]
    } else {
        vec!["id", "created_at", "updated_at"]
    };

    let mut fields = Vec::new();
    let mut skipped = 0;

    for col in &table.columns {
        if skip_columns.contains(&col.name.as_str()) {
            continue;
        }

        match normalized_to_field_info(&col.name, &col.col_type, col.is_nullable) {
            Some(fi) => fields.push(fi),
            None => {
                if let NormalizedType::Unmappable(ref type_name) = col.col_type {
                    eprintln!(
                        "    {} column {:?}.{:?} ({}) has no schema! equivalent -- skipped",
                        "warn:".yellow(),
                        table.name,
                        col.name,
                        type_name
                    );
                }
                skipped += 1;
            }
        }
    }

    let timestamps = detect_timestamps(table);

    let primary_key = if is_composite_pk {
        Some(table.primary_key_columns.clone())
    } else {
        None
    };

    codegen::update_entity_file(&pascal, &fields, timestamps, primary_key.as_deref())?;
    codegen::create_migration_file(plural, &pascal_plural, &fields)?;
    codegen::create_feature_module(&singular, plural, &pascal, &fields)?;

    println!(
        "  {} Imported table {:?} as {} ({} columns, {} skipped)",
        "✓".green(),
        table.name,
        pascal.bright_cyan(),
        fields.len(),
        skipped
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn database(
    url: &str,
    table_filter: Option<&[String]>,
    schema_name: Option<&str>,
) -> Result<(), String> {
    codegen::verify_rapina_project()?;

    println!();
    println!("  {} Connecting to database...", "->".bright_cyan());

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create async runtime: {}", e))?;

    let tables = rt.block_on(async {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            #[cfg(feature = "import-postgres")]
            {
                let schema = schema_name.unwrap_or("public");
                introspect_postgres(url, schema).await
            }
            #[cfg(not(feature = "import-postgres"))]
            {
                let _ = schema_name;
                Err("Postgres support requires the import-postgres feature. \
                     Reinstall with: cargo install rapina-cli --features import-postgres"
                    .to_string())
            }
        } else if url.starts_with("mysql://") || url.starts_with("mariadb://") {
            #[cfg(feature = "import-mysql")]
            {
                let schema = schema_name
                    .or_else(|| url.rsplit('/').next())
                    .ok_or_else(|| "Could not determine database name from URL. Use --schema to specify it.".to_string())?;
                introspect_mysql(url, schema).await
            }
            #[cfg(not(feature = "import-mysql"))]
            {
                let _ = schema_name;
                Err("MySQL support requires the import-mysql feature. \
                     Reinstall with: cargo install rapina-cli --features import-mysql"
                    .to_string())
            }
        } else if url.starts_with("sqlite://") || url.starts_with("sqlite:") {
            #[cfg(feature = "import-sqlite")]
            {
                let _ = schema_name;
                introspect_sqlite(url).await
            }
            #[cfg(not(feature = "import-sqlite"))]
            {
                let _ = schema_name;
                Err("SQLite support requires the import-sqlite feature. \
                     Reinstall with: cargo install rapina-cli --features import-sqlite"
                    .to_string())
            }
        } else {
            Err(format!(
                "Unsupported database URL scheme. Expected postgres://, mysql://, or sqlite:// -- got {:?}",
                url.split("://").next().unwrap_or("unknown")
            ))
        }
    })?;

    let total_discovered = tables.len();
    println!("  {} Discovered {} table(s)", "✓".green(), total_discovered);

    let tables = filter_and_validate_tables(tables, table_filter);

    println!(
        "  {} {} table(s) passed validation",
        "✓".green(),
        tables.len()
    );
    println!();

    if tables.is_empty() {
        println!("  No tables to import.");
        return Ok(());
    }

    let relationships = resolve_relationships(&tables);
    let mut imported = Vec::new();

    for table in &tables {
        let singular = codegen::singularize(&table.name);
        let pascal = codegen::to_pascal_case(&singular);
        generate_for_table(table, &relationships)?;
        imported.push((table.name.clone(), pascal));
    }

    // Summary
    println!();
    println!(
        "  {} Imported {} table(s):",
        "Summary:".bright_yellow(),
        imported.len()
    );
    for (table_name, pascal) in &imported {
        println!("    - {} -> {}", table_name, pascal.bright_cyan());
    }

    // Next steps
    println!();
    println!("  {}:", "Next steps".bright_yellow());
    println!();
    println!("  1. Review generated files in {}", "src/".cyan());
    println!("  2. Add module declarations to {}", "src/main.rs".cyan());
    println!("  3. Register routes in your Router");
    println!("  4. Run {} to verify", "cargo build".cyan());
    println!();

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalized_to_field_info_string_not_null() {
        let fi = normalized_to_field_info("name", &NormalizedType::Str, false).unwrap();
        assert_eq!(fi.name, "name");
        assert_eq!(fi.rust_type, "String");
        assert_eq!(fi.schema_type, "String");
        assert_eq!(fi.column_method, ".string().not_null()");
    }

    #[test]
    fn test_normalized_to_field_info_nullable() {
        let fi = normalized_to_field_info("bio", &NormalizedType::Text, true).unwrap();
        assert_eq!(fi.column_method, ".text().null()");
    }

    #[test]
    fn test_normalized_to_field_info_unmappable() {
        let result = normalized_to_field_info(
            "geom",
            &NormalizedType::Unmappable("geometry".into()),
            false,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_normalized_to_field_info_all_types() {
        let cases = vec![
            (NormalizedType::Str, "String", "String", ".string()"),
            (NormalizedType::Text, "String", "Text", ".text()"),
            (NormalizedType::I32, "i32", "i32", ".integer()"),
            (NormalizedType::I64, "i64", "i64", ".big_integer()"),
            (NormalizedType::F32, "f32", "f32", ".float()"),
            (NormalizedType::F64, "f64", "f64", ".double()"),
            (NormalizedType::Bool, "bool", "bool", ".boolean()"),
            (NormalizedType::Uuid, "Uuid", "Uuid", ".uuid()"),
            (
                NormalizedType::DateTimeUtc,
                "DateTimeUtc",
                "DateTime",
                ".timestamp_with_time_zone()",
            ),
            (
                NormalizedType::NaiveDateTime,
                "DateTime",
                "NaiveDateTime",
                ".date_time()",
            ),
            (NormalizedType::Date, "Date", "Date", ".date()"),
            (NormalizedType::Decimal, "Decimal", "Decimal", ".decimal()"),
            (NormalizedType::Json, "Json", "Json", ".json()"),
        ];

        for (norm_type, expected_rust, expected_schema, expected_col_base) in cases {
            let fi = normalized_to_field_info("x", &norm_type, false).unwrap();
            assert_eq!(fi.rust_type, expected_rust, "rust_type for {:?}", norm_type);
            assert_eq!(
                fi.schema_type, expected_schema,
                "schema_type for {:?}",
                norm_type
            );
            assert_eq!(
                fi.column_method,
                format!("{}.not_null()", expected_col_base),
                "column_method for {:?}",
                norm_type
            );
        }
    }

    #[test]
    fn test_detect_timestamps_both() {
        let table = IntrospectedTable {
            name: "users".into(),
            columns: vec![
                IntrospectedColumn {
                    name: "id".into(),
                    col_type: NormalizedType::I32,
                    is_nullable: false,
                },
                IntrospectedColumn {
                    name: "created_at".into(),
                    col_type: NormalizedType::DateTimeUtc,
                    is_nullable: false,
                },
                IntrospectedColumn {
                    name: "updated_at".into(),
                    col_type: NormalizedType::DateTimeUtc,
                    is_nullable: false,
                },
            ],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![],
        };
        assert_eq!(detect_timestamps(&table), None);
    }

    #[test]
    fn test_detect_timestamps_none() {
        let table = IntrospectedTable {
            name: "tokens".into(),
            columns: vec![IntrospectedColumn {
                name: "id".into(),
                col_type: NormalizedType::I32,
                is_nullable: false,
            }],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![],
        };
        assert_eq!(detect_timestamps(&table), Some("none"));
    }

    #[test]
    fn test_detect_timestamps_created_only() {
        let table = IntrospectedTable {
            name: "logs".into(),
            columns: vec![
                IntrospectedColumn {
                    name: "id".into(),
                    col_type: NormalizedType::I32,
                    is_nullable: false,
                },
                IntrospectedColumn {
                    name: "created_at".into(),
                    col_type: NormalizedType::DateTimeUtc,
                    is_nullable: false,
                },
            ],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![],
        };
        assert_eq!(detect_timestamps(&table), Some("created_at"));
    }

    #[test]
    fn test_filter_skips_internal_tables() {
        let tables = vec![
            IntrospectedTable {
                name: "seaql_migrations".into(),
                columns: vec![],
                primary_key_columns: vec!["id".into()],
                foreign_keys: vec![],
            },
            IntrospectedTable {
                name: "_prisma_migrations".into(),
                columns: vec![],
                primary_key_columns: vec!["id".into()],
                foreign_keys: vec![],
            },
        ];
        let result = filter_and_validate_tables(tables, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_skips_no_pk() {
        let tables = vec![IntrospectedTable {
            name: "events".into(),
            columns: vec![],
            primary_key_columns: vec![],
            foreign_keys: vec![],
        }];
        let result = filter_and_validate_tables(tables, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_skips_composite_pk() {
        let tables = vec![IntrospectedTable {
            name: "pivot".into(),
            columns: vec![],
            primary_key_columns: vec!["user_id".into(), "role_id".into()],
            foreign_keys: vec![],
        }];
        let result = filter_and_validate_tables(tables, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_skips_non_id_pk() {
        let tables = vec![IntrospectedTable {
            name: "events".into(),
            columns: vec![IntrospectedColumn {
                name: "event_id".into(),
                col_type: NormalizedType::I32,
                is_nullable: false,
            }],
            primary_key_columns: vec!["event_id".into()],
            foreign_keys: vec![],
        }];
        let result = filter_and_validate_tables(tables, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_skips_uuid_pk() {
        let tables = vec![IntrospectedTable {
            name: "events".into(),
            columns: vec![IntrospectedColumn {
                name: "id".into(),
                col_type: NormalizedType::Uuid,
                is_nullable: false,
            }],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![],
        }];
        let result = filter_and_validate_tables(tables, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_accepts_valid_table() {
        let tables = vec![IntrospectedTable {
            name: "users".into(),
            columns: vec![IntrospectedColumn {
                name: "id".into(),
                col_type: NormalizedType::I32,
                is_nullable: false,
            }],
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![],
        }];
        let result = filter_and_validate_tables(tables, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "users");
    }

    #[test]
    fn test_filter_applies_table_filter() {
        let tables = vec![
            IntrospectedTable {
                name: "users".into(),
                columns: vec![IntrospectedColumn {
                    name: "id".into(),
                    col_type: NormalizedType::I32,
                    is_nullable: false,
                }],
                primary_key_columns: vec!["id".into()],
                foreign_keys: vec![],
            },
            IntrospectedTable {
                name: "posts".into(),
                columns: vec![IntrospectedColumn {
                    name: "id".into(),
                    col_type: NormalizedType::I32,
                    is_nullable: false,
                }],
                primary_key_columns: vec!["id".into()],
                foreign_keys: vec![],
            },
        ];
        let filter = vec!["users".to_string()];
        let result = filter_and_validate_tables(tables, Some(&filter));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "users");
    }

    #[test]
    fn test_resolve_relationships() {
        let tables = vec![
            IntrospectedTable {
                name: "users".into(),
                columns: vec![IntrospectedColumn {
                    name: "id".into(),
                    col_type: NormalizedType::I32,
                    is_nullable: false,
                }],
                primary_key_columns: vec!["id".into()],
                foreign_keys: vec![],
            },
            IntrospectedTable {
                name: "posts".into(),
                columns: vec![
                    IntrospectedColumn {
                        name: "id".into(),
                        col_type: NormalizedType::I32,
                        is_nullable: false,
                    },
                    IntrospectedColumn {
                        name: "user_id".into(),
                        col_type: NormalizedType::I32,
                        is_nullable: false,
                    },
                ],
                primary_key_columns: vec!["id".into()],
                foreign_keys: vec![IntrospectedForeignKey {
                    columns: vec!["user_id".into()],
                    referenced_table: "users".into(),
                    referenced_columns: vec!["id".into()],
                }],
            },
        ];

        let rels = resolve_relationships(&tables);

        // posts should have a BelongsTo User
        let post_rels = rels.get("posts").unwrap();
        assert_eq!(post_rels.len(), 1);
        assert_eq!(post_rels[0].field_name, "user");
        assert_eq!(post_rels[0].related_pascal, "User");
        assert!(matches!(post_rels[0].kind, RelationKind::BelongsTo));

        // users should have a HasMany Post
        let user_rels = rels.get("users").unwrap();
        assert_eq!(user_rels.len(), 1);
        assert_eq!(user_rels[0].field_name, "posts");
        assert_eq!(user_rels[0].related_pascal, "Post");
        assert!(matches!(user_rels[0].kind, RelationKind::HasMany));
    }

    #[cfg(feature = "import-postgres")]
    #[test]
    fn test_map_pg_type_integers() {
        use sea_schema::postgres::def::Type;
        assert_eq!(map_pg_type(&Type::SmallInt), NormalizedType::I32);
        assert_eq!(map_pg_type(&Type::Integer), NormalizedType::I32);
        assert_eq!(map_pg_type(&Type::Serial), NormalizedType::I32);
        assert_eq!(map_pg_type(&Type::BigInt), NormalizedType::I64);
        assert_eq!(map_pg_type(&Type::BigSerial), NormalizedType::I64);
    }

    #[cfg(feature = "import-postgres")]
    #[test]
    fn test_map_pg_type_floats() {
        use sea_schema::postgres::def::Type;
        assert_eq!(map_pg_type(&Type::Real), NormalizedType::F32);
        assert_eq!(map_pg_type(&Type::DoublePrecision), NormalizedType::F64);
    }

    #[cfg(feature = "import-postgres")]
    #[test]
    fn test_map_pg_type_strings() {
        use sea_schema::postgres::def::{StringAttr, Type};
        assert_eq!(
            map_pg_type(&Type::Varchar(StringAttr { length: None })),
            NormalizedType::Str
        );
        assert_eq!(map_pg_type(&Type::Text), NormalizedType::Text);
    }

    #[cfg(feature = "import-postgres")]
    #[test]
    fn test_map_pg_type_special() {
        use sea_schema::postgres::def::Type;
        assert_eq!(map_pg_type(&Type::Boolean), NormalizedType::Bool);
        assert_eq!(map_pg_type(&Type::Uuid), NormalizedType::Uuid);
        assert_eq!(map_pg_type(&Type::Date), NormalizedType::Date);
        assert_eq!(map_pg_type(&Type::Json), NormalizedType::Json);
        assert_eq!(map_pg_type(&Type::JsonBinary), NormalizedType::Json);
    }

    #[cfg(feature = "import-postgres")]
    #[test]
    fn test_map_pg_type_unmappable() {
        use sea_schema::postgres::def::Type;
        assert!(matches!(
            map_pg_type(&Type::Point),
            NormalizedType::Unmappable(_)
        ));
    }
}
