use std::fs;
use std::path::Path;

use super::{generate_cargo_toml, generate_gitignore, write_file};

pub fn generate(name: &str, project_path: &Path, src_path: &Path) -> Result<(), String> {
    let version = env!("CARGO_PKG_VERSION");
    let rapina_dep = format!("{{ version = \"{version}\", features = [\"sqlite\"] }}");

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
        &generate_gitignore(&["*.db"]),
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
    Rapina::new()
        .with_tracing(TracingConfig::new())
        .middleware(RequestLogMiddleware::new())
        .with_database(DatabaseConfig::new("sqlite://app.db?mode=rwc"))
        .await?
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
        assert!(content.contains("DatabaseConfig::new("));
        assert!(content.contains(".with_database("));
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
    fn test_gitignore_includes_db_files() {
        let content = generate_gitignore(&["*.db"]);
        assert!(content.contains("/target"));
        assert!(content.contains("Cargo.lock"));
        assert!(content.contains("*.db"));
    }
}
