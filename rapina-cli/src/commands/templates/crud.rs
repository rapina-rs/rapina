use std::fs;
use std::path::Path;

use super::{
    DatabaseType, generate_cargo_toml, generate_env_content, generate_gitignore,
    generate_gitignore_extras, generate_rapina_dep, write_file, write_migrate_bin,
};

pub fn generate(
    name: &str,
    project_path: &Path,
    src_path: &Path,
    db_type: &DatabaseType,
) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let rapina_dep = generate_rapina_dep(version, Some(db_type));

    write_file(
        &project_path.join("Cargo.toml"),
        &generate_cargo_toml(name, &rapina_dep),
        "Cargo.toml",
    )?;
    write_file(
        &src_path.join("main.rs"),
        &generate_main_rs(),
        "src/main.rs",
    )?;
    write_file(
        &src_path.join("items.rs"),
        &generate_items_rs(),
        "src/items.rs",
    )?;
    write_file(
        &project_path.join(".gitignore"),
        &generate_gitignore(&generate_gitignore_extras(Some(db_type))),
        ".gitignore",
    )?;

    let migrations_path = src_path.join("migrations");
    fs::create_dir_all(&migrations_path)
        .map_err(|e| format!("Failed to create src/migrations directory: {}", e))?;
    write_file(
        &migrations_path.join("mod.rs"),
        &generate_migrations_mod_rs(),
        "src/migrations/mod.rs",
    )?;
    write_file(
        &migrations_path.join("m20240101_000001_create_items.rs"),
        &generate_migration_rs(),
        "src/migrations/m20240101_000001_create_items.rs",
    )?;

    // Generate .env file
    write_file(
        &project_path.join(".env"),
        &generate_env_content(Some(db_type), None),
        ".env",
    )?;

    write_migrate_bin(src_path)?;

    Ok(())
}

fn generate_main_rs() -> String {
    r#"mod items;
mod migrations;

use rapina::prelude::*;
use rapina::database::DatabaseConfig;
use rapina::middleware::RequestLogMiddleware;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    load_dotenv();

    let db_config = DatabaseConfig::from_env().expect("Failed to configure database");

    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .with_health_check(true)
        .with_database(db_config)
        .await?
        // NOTE: run_migrations applies pending migrations on every startup.
        // Fine for single-node development; use `rapina migrate up` instead
        // for controlled deployments and multi-replica environments.
        .run_migrations::<migrations::Migrator>()
        .await?
        .router(
            Router::new()
                .get("/items", items::list)
                .get("/items/:id", items::get)
                .post("/items", items::create)
                .put("/items/:id", items::update)
                .delete("/items/:id", items::delete),
        )
        .listen("127.0.0.1:3000")
        .await
}
"#
    .to_string()
}

fn generate_items_rs() -> String {
    r#"use rapina::prelude::*;
use rapina::database::Db;
use rapina::schemars;

#[derive(Serialize, Deserialize, JsonSchema)]
pub struct Item {
    pub id: i64,
    pub name: String,
    pub description: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateItem {
    pub name: String,
    pub description: String,
}

#[get("/items")]
pub async fn list(_db: Db) -> Json<Vec<Item>> {
    // TODO: query database
    Json(vec![])
}

#[get("/items/:id")]
pub async fn get(_db: Db, id: Path<i64>) -> Json<Item> {
    let id = *id;
    // TODO: query database
    Json(Item {
        id,
        name: "Example".to_string(),
        description: "An example item".to_string(),
    })
}

#[post("/items")]
pub async fn create(_db: Db, body: Json<CreateItem>) -> Json<Item> {
    // TODO: insert into database
    Json(Item {
        id: 1,
        name: body.name.clone(),
        description: body.description.clone(),
    })
}

#[put("/items/:id")]
pub async fn update(_db: Db, id: Path<i64>, body: Json<CreateItem>) -> Json<Item> {
    // TODO: update in database
    Json(Item {
        id: *id,
        name: body.name.clone(),
        description: body.description.clone(),
    })
}

#[delete("/items/:id")]
pub async fn delete(_db: Db, id: Path<i64>) -> Json<serde_json::Value> {
    // TODO: delete from database
    Json(serde_json::json!({ "deleted": *id }))
}
"#
    .to_string()
}

fn generate_migrations_mod_rs() -> String {
    r#"mod m20240101_000001_create_items;

rapina::migrations! {
    m20240101_000001_create_items,
}
"#
    .to_string()
}

fn generate_migration_rs() -> String {
    r#"use rapina::sea_orm_migration;
use rapina::migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Items::Table)
                    .col(
                        ColumnDef::new(Items::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Items::Name).string().not_null())
                    .col(ColumnDef::new(Items::Description).string().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Items::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Items {
    Table,
    Id,
    Name,
    Description,
}
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_main_rs_uses_database_config() {
        let content = generate_main_rs();
        assert!(content.contains("load_dotenv()"));
        assert!(content.contains("let db_config ="));
        assert!(content.contains(".with_database(db_config)"));
        assert!(content.contains(".await?"));
        assert!(content.contains(".run_migrations::<migrations::Migrator>()"));
        assert!(!content.contains("rapina::database::connect"));
    }

    #[test]
    fn test_generate_main_rs_has_crud_routes() {
        let content = generate_main_rs();
        assert!(content.contains(".get(\"/items\", items::list)"));
        assert!(content.contains(".get(\"/items/:id\", items::get)"));
        assert!(content.contains(".post(\"/items\", items::create)"));
        assert!(content.contains(".put(\"/items/:id\", items::update)"));
        assert!(content.contains(".delete(\"/items/:id\", items::delete)"));
    }

    #[test]
    fn test_generate_main_rs_postgres_uses_from_env() {
        let content = generate_main_rs();
        assert!(content.contains("DatabaseConfig::from_env()"));
    }

    #[test]
    fn test_generate_items_rs_has_all_handlers() {
        let content = generate_items_rs();
        assert!(content.contains("pub async fn list("));
        assert!(content.contains("pub async fn get("));
        assert!(content.contains("pub async fn create("));
        assert!(content.contains("pub async fn update("));
        assert!(content.contains("pub async fn delete("));
        assert!(content.contains("pub struct Item"));
        assert!(content.contains("pub struct CreateItem"));
        assert!(content.contains("_db: Db"));
    }

    #[test]
    fn test_generate_migrations_mod_rs() {
        let content = generate_migrations_mod_rs();
        assert!(content.contains("rapina::migrations!"));
        assert!(content.contains("m20240101_000001_create_items"));
    }

    #[test]
    fn test_generate_migration_rs_uses_seaorm_pattern() {
        let content = generate_migration_rs();
        assert!(content.contains("use rapina::migration::prelude::*;"));
        assert!(content.contains("#[derive(DeriveMigrationName)]"));
        assert!(content.contains("impl MigrationTrait for Migration"));
        assert!(content.contains("Items::Table"));
        assert!(content.contains("Items::Name"));
        assert!(content.contains("Items::Description"));
        assert!(content.contains("drop_table"));
        assert!(!content.contains("CREATE TABLE"));
    }

    #[test]
    fn test_gitignore_includes_db_files_for_sqlite() {
        let content = generate_gitignore(&["*.db"]);
        assert!(content.contains("/target"));
        assert!(content.contains("Cargo.lock"));
        assert!(content.contains("*.db"));
    }

    #[test]
    fn test_generate_env_file_sqlite() {
        let content = generate_env_content(Some(&DatabaseType::Sqlite), None);
        assert!(content.contains("DATABASE_URL=sqlite://"));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_generate_env_file_postgres() {
        let content = generate_env_content(Some(&DatabaseType::Postgres), None);
        assert!(content.contains("DATABASE_URL=postgres://"));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_generate_env_file_mysql() {
        let content = generate_env_content(Some(&DatabaseType::Mysql), None);
        assert!(content.contains("DATABASE_URL=mysql://"));
        assert!(content.contains("Replace"));
    }

    #[test]
    fn test_crud_generate_creates_migrate_bin() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        generate("myapp", dir.path(), &src, &DatabaseType::Sqlite).unwrap();
        assert!(src.join("bin").join("rapina_migrate.rs").exists());
    }
}
