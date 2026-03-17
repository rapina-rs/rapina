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
        Type::Binary(s) if s.length == Some(16) => NormalizedType::Uuid,
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
        nullable: is_nullable,
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

        // For single PK: must be named "id" and be i32 or Uuid
        // For composite PK: skip for now (schema! limitation)
        if table.primary_key_columns.len() > 1 {
            eprintln!(
                "  {} table {:?} skipped -- composite primary keys are not supported by schema!",
                "warn:".yellow(),
                table.name
            );
            continue;
        }

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
                    NormalizedType::I32 | NormalizedType::Uuid => {}
                    other => {
                        eprintln!(
                            "  {} table {:?} skipped -- PK is {:?} (schema! requires i32 or Uuid)",
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
    force: bool,
) -> Result<(), String> {
    let singular = codegen::singularize(&table.name);
    let plural = &table.name;
    let pascal = codegen::to_pascal_case(&singular);
    let pascal_plural = codegen::to_pascal_case(plural);

    let is_composite_pk = table.primary_key_columns.len() > 1;

    // For composite PK, skip only timestamps. PK columns become regular fields.
    // For single PK, skip id if it's i32 (default) and timestamps.
    // If single PK is NOT i32 (e.g. Uuid), don't skip it, so it can be marked as PK in codegen.
    let is_default_pk = !is_composite_pk
        && table
            .columns
            .iter()
            .any(|c| c.name == "id" && c.col_type == NormalizedType::I32);

    let skip_columns: Vec<&str> = if is_composite_pk || !is_default_pk {
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
    } else if !is_default_pk {
        // Special case: single PK that is NOT the default i32 "id"
        Some(table.primary_key_columns.clone())
    } else {
        None
    };

    let pk_type = if let Some(pk_col) = table.columns.iter().find(|c| c.name == "id") {
        match &pk_col.col_type {
            NormalizedType::Uuid => "Uuid",
            _ => "i32",
        }
    } else {
        "i32"
    };

    codegen::update_entity_file(&pascal, &fields, timestamps, primary_key.as_deref(), force)?;
    codegen::create_migration_file(plural, &pascal_plural, &fields, pk_type)?;
    codegen::create_feature_module(&singular, plural, &pascal, &fields, pk_type, force)?;

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
// Shared introspection
// ---------------------------------------------------------------------------

/// Create a tokio runtime, connect to the database, and return introspected tables.
fn introspect_tables(
    url: &str,
    schema_name: Option<&str>,
) -> Result<Vec<IntrospectedTable>, String> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("Failed to create async runtime: {}", e))?;

    rt.block_on(async {
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
                    .ok_or_else(|| {
                        "Could not determine database name from URL. Use --schema to specify it."
                            .to_string()
                    })?;
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
    })
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn database(
    url: &str,
    table_filter: Option<&[String]>,
    schema_name: Option<&str>,
    force: bool,
) -> Result<(), String> {
    codegen::verify_rapina_project()?;

    println!();
    println!("  {} Connecting to database...", "->".bright_cyan());

    let tables = introspect_tables(url, schema_name)?;

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
        generate_for_table(table, &relationships, force)?;
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
// Schema drift detection
// ---------------------------------------------------------------------------

use super::entity_parser::ParsedEntity;

/// A column that the entity definition expects to exist in the database.
#[derive(Debug)]
struct ExpectedColumn {
    name: String,
    expected_type: NormalizedType,
    nullable: bool,
}

/// Drift detected for a single table.
#[derive(Debug)]
struct TableDrift {
    table_name: String,
    entity_name: String,
    /// Columns in DB but not in entity
    extra_columns: Vec<(String, NormalizedType, bool)>, // (name, type, nullable)
    /// Columns in entity but not in DB
    missing_columns: Vec<String>,
    /// Columns present in both but with type/nullability mismatch
    type_mismatches: Vec<TypeMismatch>,
}

impl TableDrift {
    fn is_empty(&self) -> bool {
        self.extra_columns.is_empty()
            && self.missing_columns.is_empty()
            && self.type_mismatches.is_empty()
    }
}

#[derive(Debug)]
struct TypeMismatch {
    column_name: String,
    entity_type: NormalizedType,
    db_type: NormalizedType,
    entity_nullable: bool,
    db_nullable: bool,
}

/// Full drift report.
#[derive(Debug)]
struct DriftReport {
    drifted_tables: Vec<TableDrift>,
    untracked_tables: Vec<String>,
    missing_tables: Vec<String>,
}

impl DriftReport {
    pub fn has_drift(&self) -> bool {
        !self.drifted_tables.is_empty()
            || !self.untracked_tables.is_empty()
            || !self.missing_tables.is_empty()
    }
}

impl std::fmt::Display for NormalizedType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NormalizedType::Str => write!(f, "String"),
            NormalizedType::Text => write!(f, "Text"),
            NormalizedType::I32 => write!(f, "i32"),
            NormalizedType::I64 => write!(f, "i64"),
            NormalizedType::F32 => write!(f, "f32"),
            NormalizedType::F64 => write!(f, "f64"),
            NormalizedType::Bool => write!(f, "bool"),
            NormalizedType::Uuid => write!(f, "Uuid"),
            NormalizedType::DateTimeUtc => write!(f, "DateTime"),
            NormalizedType::NaiveDateTime => write!(f, "NaiveDateTime"),
            NormalizedType::Date => write!(f, "Date"),
            NormalizedType::Decimal => write!(f, "Decimal"),
            NormalizedType::Json => write!(f, "Json"),
            NormalizedType::Unmappable(s) => write!(f, "{}", s),
        }
    }
}

/// Map a schema type string (as written in entity.rs) back to a NormalizedType.
fn schema_type_to_normalized(schema_type: &str) -> Option<NormalizedType> {
    match schema_type {
        "String" => Some(NormalizedType::Str),
        "Text" => Some(NormalizedType::Text),
        "i32" => Some(NormalizedType::I32),
        "i64" => Some(NormalizedType::I64),
        "f32" => Some(NormalizedType::F32),
        "f64" => Some(NormalizedType::F64),
        "bool" => Some(NormalizedType::Bool),
        "Uuid" => Some(NormalizedType::Uuid),
        "DateTime" => Some(NormalizedType::DateTimeUtc),
        "NaiveDateTime" => Some(NormalizedType::NaiveDateTime),
        "Date" => Some(NormalizedType::Date),
        "Decimal" => Some(NormalizedType::Decimal),
        "Json" => Some(NormalizedType::Json),
        _ => None,
    }
}

/// Determine the FK column type by looking up the referenced entity's primary key type.
/// Falls back to I32 if the entity isn't found or has a default PK.
fn resolve_fk_type(referenced_entity_name: &str, all_entities: &[ParsedEntity]) -> NormalizedType {
    let referenced = all_entities
        .iter()
        .find(|e| e.name == referenced_entity_name);

    match referenced {
        Some(entity) => match &entity.primary_key {
            None => NormalizedType::I32,
            Some(pk_cols) => {
                if let Some(pk_col) = pk_cols.first() {
                    entity
                        .fields
                        .iter()
                        .find(|f| f.name == *pk_col)
                        .and_then(|f| schema_type_to_normalized(&f.schema_type))
                        .unwrap_or(NormalizedType::I32)
                } else {
                    NormalizedType::I32
                }
            }
        },
        None => NormalizedType::I32,
    }
}

/// Build the list of expected columns for an entity, including auto-generated ones.
fn build_expected_columns(
    entity: &ParsedEntity,
    all_entities: &[ParsedEntity],
) -> Vec<ExpectedColumn> {
    let mut columns = Vec::new();

    // Auto-generated PK column (unless custom primary_key is set)
    if entity.primary_key.is_none() {
        columns.push(ExpectedColumn {
            name: "id".to_string(),
            expected_type: NormalizedType::I32,
            nullable: false,
        });
    }

    // Entity fields
    for field in &entity.fields {
        // has_many fields don't correspond to DB columns
        if field.is_has_many {
            continue;
        }

        if field.is_belongs_to {
            let fk_type = resolve_fk_type(&field.schema_type, all_entities);
            columns.push(ExpectedColumn {
                name: field.column_name.clone(),
                expected_type: fk_type,
                nullable: field.optional,
            });
        } else if let Some(norm) = schema_type_to_normalized(&field.schema_type) {
            columns.push(ExpectedColumn {
                name: field.column_name.clone(),
                expected_type: norm,
                nullable: field.optional,
            });
        }
    }

    // Timestamp columns
    if entity.has_created_at {
        columns.push(ExpectedColumn {
            name: "created_at".to_string(),
            expected_type: NormalizedType::DateTimeUtc,
            nullable: false,
        });
    }
    if entity.has_updated_at {
        columns.push(ExpectedColumn {
            name: "updated_at".to_string(),
            expected_type: NormalizedType::DateTimeUtc,
            nullable: false,
        });
    }

    columns
}

/// Filter tables for diff — removes internal tables but keeps everything else
/// (unlike filter_and_validate_tables which also strips tables without PK).
fn filter_tables_for_diff(
    tables: Vec<IntrospectedTable>,
    table_filter: Option<&[String]>,
) -> Vec<IntrospectedTable> {
    tables
        .into_iter()
        .filter(|table| {
            if INTERNAL_TABLES.contains(&table.name.as_str()) || table.name.starts_with('_') {
                return false;
            }
            if let Some(filter) = table_filter {
                return filter.iter().any(|f| f == &table.name);
            }
            true
        })
        .collect()
}

/// Compare parsed entities against introspected DB tables and produce a drift report.
fn compute_drift(entities: &[ParsedEntity], db_tables: &[IntrospectedTable]) -> DriftReport {
    let entity_map: HashMap<&str, &ParsedEntity> = entities
        .iter()
        .map(|e| (e.table_name.as_str(), e))
        .collect();
    let db_map: HashMap<&str, &IntrospectedTable> =
        db_tables.iter().map(|t| (t.name.as_str(), t)).collect();

    let mut drifted_tables = Vec::new();
    let mut untracked_tables = Vec::new();
    let mut missing_tables = Vec::new();

    // Check each DB table
    for table in db_tables {
        if !entity_map.contains_key(table.name.as_str()) {
            untracked_tables.push(table.name.clone());
        }
    }

    // Check each entity
    for entity in entities {
        match db_map.get(entity.table_name.as_str()) {
            None => {
                missing_tables.push(entity.table_name.clone());
            }
            Some(db_table) => {
                let drift = compare_table(entity, db_table, entities);
                if !drift.is_empty() {
                    drifted_tables.push(drift);
                }
            }
        }
    }

    DriftReport {
        drifted_tables,
        untracked_tables,
        missing_tables,
    }
}

/// Compare a single entity against its DB table.
fn compare_table(
    entity: &ParsedEntity,
    db_table: &IntrospectedTable,
    all_entities: &[ParsedEntity],
) -> TableDrift {
    let expected = build_expected_columns(entity, all_entities);
    let expected_map: HashMap<&str, &ExpectedColumn> =
        expected.iter().map(|c| (c.name.as_str(), c)).collect();
    let db_col_map: HashMap<&str, &IntrospectedColumn> = db_table
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c))
        .collect();

    let mut extra_columns = Vec::new();
    let mut missing_columns = Vec::new();
    let mut type_mismatches = Vec::new();

    // Columns in DB but not in entity
    for db_col in &db_table.columns {
        if !expected_map.contains_key(db_col.name.as_str()) {
            extra_columns.push((
                db_col.name.clone(),
                db_col.col_type.clone(),
                db_col.is_nullable,
            ));
        }
    }

    // Columns in entity but not in DB, or type mismatches
    for exp_col in &expected {
        match db_col_map.get(exp_col.name.as_str()) {
            None => {
                missing_columns.push(exp_col.name.clone());
            }
            Some(db_col) => {
                let type_differs = exp_col.expected_type != db_col.col_type
                    // Don't flag unmappable types as mismatches
                    && !matches!(db_col.col_type, NormalizedType::Unmappable(_));
                let null_differs = exp_col.nullable != db_col.is_nullable;

                if type_differs || null_differs {
                    type_mismatches.push(TypeMismatch {
                        column_name: exp_col.name.clone(),
                        entity_type: exp_col.expected_type.clone(),
                        db_type: db_col.col_type.clone(),
                        entity_nullable: exp_col.nullable,
                        db_nullable: db_col.is_nullable,
                    });
                }
            }
        }
    }

    TableDrift {
        table_name: db_table.name.clone(),
        entity_name: entity.name.clone(),
        extra_columns,
        missing_columns,
        type_mismatches,
    }
}

/// Print a colored drift report to stdout.
fn print_drift_report(report: &DriftReport) {
    println!();

    if !report.has_drift() {
        println!("  {} No schema drift detected", "✓".green());
        return;
    }

    println!("  {}:", "Drift report".bright_yellow());
    println!();

    for drift in &report.drifted_tables {
        println!(
            "  {} Table {:?} ({}) has drift:",
            "✗".red(),
            drift.table_name,
            drift.entity_name.bright_cyan()
        );
        for (name, col_type, nullable) in &drift.extra_columns {
            let null_str = if *nullable { "nullable" } else { "not null" };
            println!(
                "    {} column {:?} ({}, {}) exists in DB but not in entity",
                "+".green(),
                name,
                col_type,
                null_str
            );
        }
        for name in &drift.missing_columns {
            println!(
                "    {} column {:?} exists in entity but not in DB",
                "-".red(),
                name
            );
        }
        for m in &drift.type_mismatches {
            if m.entity_type != m.db_type {
                println!(
                    "    {} column {:?} type mismatch: entity has {}, DB has {}",
                    "~".yellow(),
                    m.column_name,
                    m.entity_type,
                    m.db_type
                );
            }
            if m.entity_nullable != m.db_nullable {
                let entity_null = if m.entity_nullable {
                    "NULL"
                } else {
                    "NOT NULL"
                };
                let db_null = if m.db_nullable { "NULL" } else { "NOT NULL" };
                println!(
                    "    {} column {:?} nullability mismatch: entity has {}, DB has {}",
                    "~".yellow(),
                    m.column_name,
                    entity_null,
                    db_null
                );
            }
        }
        println!();
    }

    if !report.untracked_tables.is_empty() {
        println!("  {} Untracked tables (in DB, no entity):", "⚠".yellow());
        for t in &report.untracked_tables {
            println!("    {} {}", "•".yellow(), t);
        }
        println!();
    }

    if !report.missing_tables.is_empty() {
        println!("  {} Missing tables (in entity, not in DB):", "⚠".yellow());
        for t in &report.missing_tables {
            println!("    {} {}", "•".yellow(), t);
        }
        println!();
    }

    let mut parts = Vec::new();
    if !report.drifted_tables.is_empty() {
        parts.push(format!(
            "{} table(s) with drift",
            report.drifted_tables.len()
        ));
    }
    if !report.untracked_tables.is_empty() {
        parts.push(format!("{} untracked", report.untracked_tables.len()));
    }
    if !report.missing_tables.is_empty() {
        parts.push(format!("{} missing", report.missing_tables.len()));
    }
    println!("  {}: {}", "Summary".bright_yellow(), parts.join(", "));
    println!();
}

// ---------------------------------------------------------------------------
// Diff entry point
// ---------------------------------------------------------------------------

pub fn database_diff(
    url: &str,
    table_filter: Option<&[String]>,
    schema_name: Option<&str>,
) -> Result<(), String> {
    codegen::verify_rapina_project()?;

    println!();
    println!("  {} Parsing entity definitions...", "→".bright_cyan());

    let entities =
        super::entity_parser::parse_entity_file_at(std::path::Path::new("src/entity.rs"))?;
    println!(
        "  {} Parsed {} entity/entities from {}",
        "✓".green(),
        entities.len(),
        "src/entity.rs".cyan()
    );

    println!("  {} Connecting to database...", "→".bright_cyan());

    let tables = introspect_tables(url, schema_name)?;

    let total = tables.len();
    let db_tables = filter_tables_for_diff(tables, table_filter);
    println!("  {} Discovered {} table(s)", "✓".green(), total);

    let report = compute_drift(&entities, &db_tables);
    print_drift_report(&report);

    if report.has_drift() {
        Err("Schema drift detected".to_string())
    } else {
        Ok(())
    }
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
    fn test_filter_accepts_uuid_pk() {
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
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "events");
    }

    #[test]
    fn test_filter_skips_invalid_pk_type() {
        let tables = vec![IntrospectedTable {
            name: "events".into(),
            columns: vec![IntrospectedColumn {
                name: "id".into(),
                col_type: NormalizedType::Str, // Not i32 or Uuid
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

    #[cfg(feature = "import-sqlite")]
    #[test]
    fn test_map_sqlite_type_special() {
        use sea_schema::sea_query::ColumnType;
        assert_eq!(map_sqlite_type(&ColumnType::Uuid), NormalizedType::Uuid);
        assert_eq!(map_sqlite_type(&ColumnType::Integer), NormalizedType::I32);
    }

    #[cfg(feature = "import-mysql")]
    #[test]
    fn test_map_mysql_type_special() {
        use sea_schema::mysql::def::{NumericAttr, StringAttr, Type};
        assert_eq!(
            map_mysql_type(&Type::Binary(StringAttr {
                length: Some(16),
                ..Default::default()
            })),
            NormalizedType::Uuid
        );
        assert_eq!(
            map_mysql_type(&Type::Int(NumericAttr::default())),
            NormalizedType::I32
        );
    }

    // -----------------------------------------------------------------------
    // schema_type_to_normalized
    // -----------------------------------------------------------------------

    #[test]
    fn test_schema_type_to_normalized_all_types() {
        assert_eq!(
            schema_type_to_normalized("String"),
            Some(NormalizedType::Str)
        );
        assert_eq!(
            schema_type_to_normalized("Text"),
            Some(NormalizedType::Text)
        );
        assert_eq!(schema_type_to_normalized("i32"), Some(NormalizedType::I32));
        assert_eq!(schema_type_to_normalized("i64"), Some(NormalizedType::I64));
        assert_eq!(schema_type_to_normalized("f32"), Some(NormalizedType::F32));
        assert_eq!(schema_type_to_normalized("f64"), Some(NormalizedType::F64));
        assert_eq!(
            schema_type_to_normalized("bool"),
            Some(NormalizedType::Bool)
        );
        assert_eq!(
            schema_type_to_normalized("Uuid"),
            Some(NormalizedType::Uuid)
        );
        assert_eq!(
            schema_type_to_normalized("DateTime"),
            Some(NormalizedType::DateTimeUtc)
        );
        assert_eq!(
            schema_type_to_normalized("NaiveDateTime"),
            Some(NormalizedType::NaiveDateTime)
        );
        assert_eq!(
            schema_type_to_normalized("Date"),
            Some(NormalizedType::Date)
        );
        assert_eq!(
            schema_type_to_normalized("Decimal"),
            Some(NormalizedType::Decimal)
        );
        assert_eq!(
            schema_type_to_normalized("Json"),
            Some(NormalizedType::Json)
        );
    }

    #[test]
    fn test_schema_type_to_normalized_unknown() {
        assert_eq!(schema_type_to_normalized("User"), None);
        assert_eq!(schema_type_to_normalized("Post"), None);
        assert_eq!(schema_type_to_normalized("unknown"), None);
    }

    // -----------------------------------------------------------------------
    // NormalizedType Display
    // -----------------------------------------------------------------------

    #[test]
    fn test_normalized_type_display() {
        assert_eq!(format!("{}", NormalizedType::Str), "String");
        assert_eq!(format!("{}", NormalizedType::I32), "i32");
        assert_eq!(format!("{}", NormalizedType::DateTimeUtc), "DateTime");
        assert_eq!(
            format!("{}", NormalizedType::Unmappable("geometry".into())),
            "geometry"
        );
    }

    // -----------------------------------------------------------------------
    // build_expected_columns
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_expected_columns_simple() {
        use super::super::entity_parser::{ParsedEntity, ParsedField};

        let entity = ParsedEntity {
            name: "User".into(),
            table_name: "users".into(),
            fields: vec![
                ParsedField {
                    name: "email".into(),
                    column_name: "email".into(),
                    schema_type: "String".into(),
                    optional: false,
                    is_belongs_to: false,
                    is_has_many: false,
                },
                ParsedField {
                    name: "bio".into(),
                    column_name: "bio".into(),
                    schema_type: "Text".into(),
                    optional: true,
                    is_belongs_to: false,
                    is_has_many: false,
                },
            ],
            has_created_at: true,
            has_updated_at: true,
            primary_key: None,
        };

        let cols = build_expected_columns(&entity, &[]);
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        // id (auto) + email + bio + created_at + updated_at
        assert_eq!(
            names,
            vec!["id", "email", "bio", "created_at", "updated_at"]
        );

        // Check id is i32, not null
        assert_eq!(cols[0].expected_type, NormalizedType::I32);
        assert!(!cols[0].nullable);

        // Check bio is Text, nullable
        assert_eq!(cols[2].expected_type, NormalizedType::Text);
        assert!(cols[2].nullable);
    }

    #[test]
    fn test_build_expected_columns_no_timestamps() {
        use super::super::entity_parser::{ParsedEntity, ParsedField};

        let entity = ParsedEntity {
            name: "Token".into(),
            table_name: "tokens".into(),
            fields: vec![ParsedField {
                name: "value".into(),
                column_name: "value".into(),
                schema_type: "String".into(),
                optional: false,
                is_belongs_to: false,
                is_has_many: false,
            }],
            has_created_at: false,
            has_updated_at: false,
            primary_key: None,
        };

        let cols = build_expected_columns(&entity, &[]);
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        // id + value, no timestamps
        assert_eq!(names, vec!["id", "value"]);
    }

    #[test]
    fn test_build_expected_columns_belongs_to() {
        use super::super::entity_parser::{ParsedEntity, ParsedField};

        let entity = ParsedEntity {
            name: "Post".into(),
            table_name: "posts".into(),
            fields: vec![
                ParsedField {
                    name: "title".into(),
                    column_name: "title".into(),
                    schema_type: "String".into(),
                    optional: false,
                    is_belongs_to: false,
                    is_has_many: false,
                },
                ParsedField {
                    name: "author".into(),
                    column_name: "author_id".into(),
                    schema_type: "User".into(),
                    optional: false,
                    is_belongs_to: true,
                    is_has_many: false,
                },
            ],
            has_created_at: true,
            has_updated_at: true,
            primary_key: None,
        };

        let cols = build_expected_columns(&entity, &[]);
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"author_id"));

        let fk = cols.iter().find(|c| c.name == "author_id").unwrap();
        assert_eq!(fk.expected_type, NormalizedType::I32);
        assert!(!fk.nullable);
    }

    #[test]
    fn test_build_expected_columns_has_many_ignored() {
        use super::super::entity_parser::{ParsedEntity, ParsedField};

        let entity = ParsedEntity {
            name: "User".into(),
            table_name: "users".into(),
            fields: vec![
                ParsedField {
                    name: "name".into(),
                    column_name: "name".into(),
                    schema_type: "String".into(),
                    optional: false,
                    is_belongs_to: false,
                    is_has_many: false,
                },
                ParsedField {
                    name: "posts".into(),
                    column_name: "posts".into(),
                    schema_type: "Post".into(),
                    optional: false,
                    is_belongs_to: false,
                    is_has_many: true,
                },
            ],
            has_created_at: false,
            has_updated_at: false,
            primary_key: None,
        };

        let cols = build_expected_columns(&entity, &[]);
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        // has_many "posts" should NOT produce a column
        assert_eq!(names, vec!["id", "name"]);
    }

    #[test]
    fn test_build_expected_columns_custom_pk() {
        use super::super::entity_parser::{ParsedEntity, ParsedField};

        let entity = ParsedEntity {
            name: "Event".into(),
            table_name: "events".into(),
            fields: vec![
                ParsedField {
                    name: "id".into(),
                    column_name: "id".into(),
                    schema_type: "Uuid".into(),
                    optional: false,
                    is_belongs_to: false,
                    is_has_many: false,
                },
                ParsedField {
                    name: "name".into(),
                    column_name: "name".into(),
                    schema_type: "String".into(),
                    optional: false,
                    is_belongs_to: false,
                    is_has_many: false,
                },
            ],
            has_created_at: true,
            has_updated_at: true,
            primary_key: Some(vec!["id".to_string()]),
        };

        let cols = build_expected_columns(&entity, &[]);
        let names: Vec<&str> = cols.iter().map(|c| c.name.as_str()).collect();
        // No auto-generated id because primary_key is Some
        assert_eq!(names, vec!["id", "name", "created_at", "updated_at"]);
        // id should be Uuid, not i32
        let id_col = cols.iter().find(|c| c.name == "id").unwrap();
        assert_eq!(id_col.expected_type, NormalizedType::Uuid);
    }

    // -----------------------------------------------------------------------
    // compute_drift
    // -----------------------------------------------------------------------

    fn make_entity(name: &str, table: &str, fields: Vec<(&str, &str, bool)>) -> ParsedEntity {
        use super::super::entity_parser::{ParsedEntity, ParsedField};

        ParsedEntity {
            name: name.into(),
            table_name: table.into(),
            fields: fields
                .into_iter()
                .map(|(n, t, opt)| ParsedField {
                    name: n.into(),
                    column_name: n.into(),
                    schema_type: t.into(),
                    optional: opt,
                    is_belongs_to: false,
                    is_has_many: false,
                })
                .collect(),
            has_created_at: false,
            has_updated_at: false,
            primary_key: None,
        }
    }

    fn make_table(name: &str, columns: Vec<(&str, NormalizedType, bool)>) -> IntrospectedTable {
        IntrospectedTable {
            name: name.into(),
            columns: columns
                .into_iter()
                .map(|(n, t, nullable)| IntrospectedColumn {
                    name: n.into(),
                    col_type: t,
                    is_nullable: nullable,
                })
                .collect(),
            primary_key_columns: vec!["id".into()],
            foreign_keys: vec![],
        }
    }

    #[test]
    fn test_drift_no_changes() {
        let entities = vec![make_entity(
            "User",
            "users",
            vec![("email", "String", false)],
        )];
        let tables = vec![make_table(
            "users",
            vec![
                ("id", NormalizedType::I32, false),
                ("email", NormalizedType::Str, false),
            ],
        )];

        let report = compute_drift(&entities, &tables);
        assert!(!report.has_drift());
        assert!(report.drifted_tables.is_empty());
        assert!(report.untracked_tables.is_empty());
        assert!(report.missing_tables.is_empty());
    }

    #[test]
    fn test_drift_extra_db_column() {
        let entities = vec![make_entity(
            "User",
            "users",
            vec![("email", "String", false)],
        )];
        let tables = vec![make_table(
            "users",
            vec![
                ("id", NormalizedType::I32, false),
                ("email", NormalizedType::Str, false),
                ("phone", NormalizedType::Str, true),
            ],
        )];

        let report = compute_drift(&entities, &tables);
        assert!(report.has_drift());
        assert_eq!(report.drifted_tables.len(), 1);
        assert_eq!(report.drifted_tables[0].extra_columns.len(), 1);
        assert_eq!(report.drifted_tables[0].extra_columns[0].0, "phone");
    }

    #[test]
    fn test_drift_missing_db_column() {
        let entities = vec![make_entity(
            "User",
            "users",
            vec![("email", "String", false), ("phone", "String", false)],
        )];
        let tables = vec![make_table(
            "users",
            vec![
                ("id", NormalizedType::I32, false),
                ("email", NormalizedType::Str, false),
            ],
        )];

        let report = compute_drift(&entities, &tables);
        assert!(report.has_drift());
        assert_eq!(report.drifted_tables[0].missing_columns, vec!["phone"]);
    }

    #[test]
    fn test_drift_type_mismatch() {
        let entities = vec![make_entity(
            "User",
            "users",
            vec![("email", "String", false)],
        )];
        let tables = vec![make_table(
            "users",
            vec![
                ("id", NormalizedType::I32, false),
                ("email", NormalizedType::Text, false), // Text vs String
            ],
        )];

        let report = compute_drift(&entities, &tables);
        assert!(report.has_drift());
        assert_eq!(report.drifted_tables[0].type_mismatches.len(), 1);
        let m = &report.drifted_tables[0].type_mismatches[0];
        assert_eq!(m.column_name, "email");
        assert_eq!(m.entity_type, NormalizedType::Str);
        assert_eq!(m.db_type, NormalizedType::Text);
    }

    #[test]
    fn test_drift_nullability_mismatch() {
        let entities = vec![make_entity(
            "User",
            "users",
            vec![("email", "String", false)], // NOT NULL
        )];
        let tables = vec![make_table(
            "users",
            vec![
                ("id", NormalizedType::I32, false),
                ("email", NormalizedType::Str, true), // NULL
            ],
        )];

        let report = compute_drift(&entities, &tables);
        assert!(report.has_drift());
        assert_eq!(report.drifted_tables[0].type_mismatches.len(), 1);
        let m = &report.drifted_tables[0].type_mismatches[0];
        assert_eq!(m.column_name, "email");
        assert!(!m.entity_nullable);
        assert!(m.db_nullable);
    }

    #[test]
    fn test_drift_untracked_table() {
        let entities = vec![make_entity(
            "User",
            "users",
            vec![("email", "String", false)],
        )];
        let tables = vec![
            make_table(
                "users",
                vec![
                    ("id", NormalizedType::I32, false),
                    ("email", NormalizedType::Str, false),
                ],
            ),
            make_table("analytics_events", vec![("id", NormalizedType::I32, false)]),
        ];

        let report = compute_drift(&entities, &tables);
        assert!(report.has_drift());
        assert_eq!(report.untracked_tables, vec!["analytics_events"]);
    }

    #[test]
    fn test_drift_missing_table() {
        let entities = vec![
            make_entity("User", "users", vec![("email", "String", false)]),
            make_entity(
                "Notification",
                "notifications",
                vec![("body", "String", false)],
            ),
        ];
        let tables = vec![make_table(
            "users",
            vec![
                ("id", NormalizedType::I32, false),
                ("email", NormalizedType::Str, false),
            ],
        )];

        let report = compute_drift(&entities, &tables);
        assert!(report.has_drift());
        assert_eq!(report.missing_tables, vec!["notifications"]);
    }

    #[test]
    fn test_drift_unmappable_db_type_is_extra_column() {
        let entities = vec![make_entity(
            "Place",
            "places",
            vec![("name", "String", false)],
        )];
        let tables = vec![make_table(
            "places",
            vec![
                ("id", NormalizedType::I32, false),
                ("name", NormalizedType::Str, false),
                ("geom", NormalizedType::Unmappable("geometry".into()), false),
            ],
        )];

        let report = compute_drift(&entities, &tables);
        assert!(report.has_drift());
        assert_eq!(report.drifted_tables[0].extra_columns.len(), 1);
        assert_eq!(report.drifted_tables[0].extra_columns[0].0, "geom");
        // Should NOT show as type mismatch
        assert!(report.drifted_tables[0].type_mismatches.is_empty());
    }

    #[test]
    fn test_drift_has_drift_returns_false_when_empty() {
        let report = DriftReport {
            drifted_tables: vec![],
            untracked_tables: vec![],
            missing_tables: vec![],
        };
        assert!(!report.has_drift());
    }

    #[test]
    fn test_drift_mixed_scenario() {
        let entities = vec![
            make_entity(
                "User",
                "users",
                vec![
                    ("email", "String", false),
                    ("age", "i32", false), // missing in DB
                ],
            ),
            make_entity(
                "Notification",
                "notifications",
                vec![("body", "String", false)],
            ), // missing table
        ];
        let tables = vec![
            make_table(
                "users",
                vec![
                    ("id", NormalizedType::I32, false),
                    ("email", NormalizedType::Text, false), // type mismatch
                    ("phone", NormalizedType::Str, true),   // extra
                ],
            ),
            make_table("temp_imports", vec![("id", NormalizedType::I32, false)]), // untracked
        ];

        let report = compute_drift(&entities, &tables);
        assert!(report.has_drift());

        // Users table should have drift
        assert_eq!(report.drifted_tables.len(), 1);
        let drift = &report.drifted_tables[0];
        assert_eq!(drift.table_name, "users");
        assert_eq!(drift.extra_columns.len(), 1); // phone
        assert_eq!(drift.missing_columns, vec!["age"]);
        assert_eq!(drift.type_mismatches.len(), 1); // email String vs Text

        assert_eq!(report.untracked_tables, vec!["temp_imports"]);
        assert_eq!(report.missing_tables, vec!["notifications"]);
    }

    // -----------------------------------------------------------------------
    // filter_tables_for_diff
    // -----------------------------------------------------------------------

    #[test]
    fn test_filter_tables_for_diff_removes_internal() {
        let tables = vec![
            make_table("users", vec![("id", NormalizedType::I32, false)]),
            make_table("seaql_migrations", vec![("id", NormalizedType::I32, false)]),
            make_table(
                "_prisma_migrations",
                vec![("id", NormalizedType::I32, false)],
            ),
        ];

        let result = filter_tables_for_diff(tables, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "users");
    }

    #[test]
    fn test_filter_tables_for_diff_keeps_tables_without_pk() {
        // Unlike filter_and_validate_tables, diff filter should keep tables without PK
        let tables = vec![IntrospectedTable {
            name: "events".into(),
            columns: vec![IntrospectedColumn {
                name: "data".into(),
                col_type: NormalizedType::Json,
                is_nullable: false,
            }],
            primary_key_columns: vec![], // no PK
            foreign_keys: vec![],
        }];

        let result = filter_tables_for_diff(tables, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "events");
    }

    #[test]
    fn test_filter_tables_for_diff_applies_filter() {
        let tables = vec![
            make_table("users", vec![("id", NormalizedType::I32, false)]),
            make_table("posts", vec![("id", NormalizedType::I32, false)]),
        ];

        let filter = vec!["users".to_string()];
        let result = filter_tables_for_diff(tables, Some(&filter));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "users");
    }

    // -----------------------------------------------------------------------
    // resolve_fk_type
    // -----------------------------------------------------------------------

    #[test]
    fn test_resolve_fk_type_default_pk() {
        assert_eq!(resolve_fk_type("Unknown", &[]), NormalizedType::I32);
    }

    #[test]
    fn test_resolve_fk_type_uuid_pk() {
        use super::super::entity_parser::{ParsedEntity, ParsedField};

        let event = ParsedEntity {
            name: "Event".to_string(),
            table_name: "events".to_string(),
            fields: vec![ParsedField {
                name: "id".to_string(),
                column_name: "id".to_string(),
                schema_type: "Uuid".to_string(),
                optional: false,
                is_belongs_to: false,
                is_has_many: false,
            }],
            has_created_at: false,
            has_updated_at: false,
            primary_key: Some(vec!["id".to_string()]),
        };

        assert_eq!(resolve_fk_type("Event", &[event]), NormalizedType::Uuid);
    }

    #[test]
    fn test_drift_belongs_to_uuid_pk() {
        use super::super::entity_parser::{ParsedEntity, ParsedField};

        let event = ParsedEntity {
            name: "Event".to_string(),
            table_name: "events".to_string(),
            fields: vec![ParsedField {
                name: "id".to_string(),
                column_name: "id".to_string(),
                schema_type: "Uuid".to_string(),
                optional: false,
                is_belongs_to: false,
                is_has_many: false,
            }],
            has_created_at: false,
            has_updated_at: false,
            primary_key: Some(vec!["id".to_string()]),
        };
        let ticket = ParsedEntity {
            name: "Ticket".to_string(),
            table_name: "tickets".to_string(),
            fields: vec![ParsedField {
                name: "event".to_string(),
                column_name: "event_id".to_string(),
                schema_type: "Event".to_string(),
                optional: false,
                is_belongs_to: true,
                is_has_many: false,
            }],
            has_created_at: false,
            has_updated_at: false,
            primary_key: None,
        };
        let entities = vec![event, ticket];

        let db_table = IntrospectedTable {
            name: "tickets".to_string(),
            columns: vec![
                IntrospectedColumn {
                    name: "id".to_string(),
                    col_type: NormalizedType::I32,
                    is_nullable: false,
                },
                IntrospectedColumn {
                    name: "event_id".to_string(),
                    col_type: NormalizedType::Uuid,
                    is_nullable: false,
                },
            ],
            primary_key_columns: vec!["id".to_string()],
            foreign_keys: vec![],
        };

        let drift = compare_table(&entities[1], &db_table, &entities);
        assert!(
            drift.type_mismatches.is_empty(),
            "UUID FK should not produce a type mismatch"
        );
    }
}
