//! Rapina CLI - Command line tool for the Rapina web framework.

mod colors;
mod commands;
mod common;

use clap::{Parser, Subcommand};
use colored::Colorize;

#[derive(Parser)]
#[command(name = "rapina")]
#[command(author, version, about = "CLI tool for the Rapina web framework", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Display version information
    Version,
    /// Create a new Rapina project
    New {
        /// Name of the project to create
        name: String,
        /// Starter template (crud, auth). Defaults to a REST API scaffold when omitted.
        #[arg(long)]
        template: Option<String>,
        /// Skip generating AI assistant config files (AGENT.md, .claude/, .cursor/)
        #[arg(long)]
        no_ai: bool,
    },
    /// Add a resource to an existing Rapina project
    Add {
        #[command(subcommand)]
        command: AddCommands,
    },
    /// Start development server with hot reload
    Dev {
        /// Port to listen on
        #[arg(short, long, env = "RAPINA_PORT", default_value = "3000")]
        port: u16,
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Disable hot reload
        #[arg(long)]
        no_reload: bool,
    },
    /// OpenAPI specification tools
    Openapi {
        #[command(subcommand)]
        command: OpenapiCommands,
    },
    /// List all registered routes
    Routes {
        /// Port to listen on
        #[arg(short, long, env = "RAPINA_PORT", default_value = "3000")]
        port: u16,
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Database seeding tools
    Seed {
        #[command(subcommand)]
        command: SeedCommands,
    },
    /// Database migration tools
    Migrate {
        #[command(subcommand)]
        command: MigrateCommands,
    },
    /// Background jobs management
    Jobs {
        #[command(subcommand)]
        command: JobsCommands,
    },
    /// Run health checks on your API
    Doctor {
        /// Port to listen on
        #[arg(short, long, env = "RAPINA_PORT", default_value = "3000")]
        port: u16,
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Import from external sources (OpenAPI specs, databases, etc.)
    Import {
        #[command(subcommand)]
        command: ImportCommands,
    },
    /// Run tests with pretty output
    Test {
        /// Generate coverage report (requires cargo-llvm-cov)
        #[arg(long)]
        coverage: bool,
        /// Watch for changes and re-run tests
        #[arg(short, long)]
        watch: bool,
        /// Update snapshot files (golden-file testing)
        #[arg(long)]
        bless: bool,
        /// Filter tests by name
        filter: Option<String>,
    },
}

#[derive(Subcommand)]
enum SeedCommands {
    /// Load seed data from JSON files into the database
    Load {
        /// Load specific entity only
        #[arg(long)]
        entity: Option<String>,
        /// Wipe database before loading (truncate + load)
        #[arg(long)]
        fresh: bool,
    },
    /// Dump database data to JSON seed files
    Dump {
        /// Dump specific entity only
        #[arg(long)]
        entity: Option<String>,
    },
    /// Generate fake seed data based on schema types
    Generate {
        /// Number of records per entity
        #[arg(long, default_value = "10")]
        count: u32,
        /// Generate for specific entity only
        #[arg(long)]
        entity: Option<String>,
    },
}

#[derive(Subcommand)]
enum AddCommands {
    /// Scaffold a new CRUD resource (handlers, DTO, error type, migration)
    Resource {
        /// Name of the resource in snake_case (e.g., post, blog_post)
        name: String,
        /// Fields in name:type format (e.g., title:string body:text published:bool)
        fields: Vec<String>,
    },
}

#[derive(Subcommand)]
enum MigrateCommands {
    /// Generate a new migration file
    New {
        /// Name of the migration (e.g., create_users)
        name: String,
    },
}

#[derive(Subcommand)]
enum JobsCommands {
    /// Set up the background jobs migration in your project
    Init,
}

#[derive(Subcommand)]
enum ImportCommands {
    /// Import schema from a live database
    Database {
        /// Database connection URL (e.g., postgres://user:pass@host/db)
        #[arg(long, env = "DATABASE_URL")]
        url: String,
        /// Only import specific tables (comma-separated)
        #[arg(long, value_delimiter = ',')]
        tables: Option<Vec<String>>,
        /// Database schema name (default: "public" for Postgres)
        #[arg(long)]
        schema: Option<String>,
        /// Overwrite existing files (useful for re-importing after schema changes)
        #[arg(long)]
        force: bool,
    },
    /// Import handlers, DTOs, and module structure from an OpenAPI 3.0 spec
    #[cfg(feature = "import-openapi")]
    Openapi {
        /// Path to OpenAPI spec file (JSON or YAML)
        file: String,
        /// Preview generated files without writing
        #[arg(long)]
        dry_run: bool,
        /// Only import endpoints with these tags (comma-separated)
        #[arg(long, value_delimiter = ',')]
        tags: Option<Vec<String>>,
    },
}

#[derive(Subcommand)]
enum OpenapiCommands {
    /// Export OpenAPI spec to stdout or file
    Export {
        /// Output file path (stdout if not specified)
        #[arg(short, long)]
        output: Option<String>,
        /// Port to connect to
        #[arg(short, long, env = "SERVER_PORT", default_value = "3000")]
        port: u16,
        /// Host to connect to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Check if openapi.json matches the current code
    Check {
        /// Path to openapi.json file
        #[arg(default_value = "openapi.json")]
        file: String,
        /// Port to connect to
        #[arg(short, long, env = "SERVER_PORT", default_value = "3000")]
        port: u16,
        /// Host to connect to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
    /// Compare spec with another branch and detect breaking changes
    Diff {
        /// Base branch to compare against
        #[arg(short, long)]
        base: String,
        /// Path to openapi.json file
        #[arg(default_value = "openapi.json")]
        file: String,
        /// Port to connect to
        #[arg(short, long, env = "SERVER_PORT", default_value = "3000")]
        port: u16,
        /// Host to connect to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Version) => {
            print_version();
        }
        Some(Commands::New {
            name,
            template,
            no_ai,
        }) => {
            if let Err(e) = commands::new::execute(&name, template.as_deref(), no_ai) {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Add { command }) => {
            let result = match command {
                AddCommands::Resource { name, fields } => commands::add::resource(&name, &fields),
            };
            if let Err(e) = result {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Dev {
            port,
            host,
            no_reload,
        }) => {
            let config = commands::dev::DevConfig {
                host,
                port,
                reload: !no_reload,
            };
            if let Err(e) = commands::dev::execute(config) {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Seed { command }) => {
            #[cfg(feature = "seed")]
            {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                let result = rt.block_on(async {
                    match command {
                        SeedCommands::Load { entity, fresh } => {
                            commands::seed::load(entity, fresh).await
                        }
                        SeedCommands::Dump { entity } => commands::seed::dump(entity).await,
                        SeedCommands::Generate { count, entity } => {
                            commands::seed::generate(count, entity)
                        }
                    }
                });
                if let Err(e) = result {
                    eprintln!("{} {}", "Error:".red().bold(), e);
                    std::process::exit(1);
                }
            }
            #[cfg(not(feature = "seed"))]
            {
                let _ = command;
                let msg = "The seed command requires a database feature. \
                           Reinstall with: cargo install rapina-cli --features seed-postgres";
                eprintln!("{} {}", "Error:".red().bold(), msg);
                std::process::exit(1);
            }
        }
        Some(Commands::Migrate { command }) => {
            let result = match command {
                MigrateCommands::New { name } => commands::migrate::new_migration(&name),
            };
            if let Err(e) = result {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Openapi { command }) => {
            let result = match command {
                OpenapiCommands::Export { output, host, port } => {
                    commands::openapi::export(output, &host, port)
                }
                OpenapiCommands::Check { file, host, port } => {
                    commands::openapi::check(&file, &host, port)
                }
                OpenapiCommands::Diff {
                    base,
                    file,
                    host,
                    port,
                } => commands::openapi::diff(&base, &file, &host, port),
            };
            if let Err(e) = result {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Routes { host, port }) => {
            let config = commands::routes::RoutesConfig { host, port };
            if let Err(e) = commands::routes::execute(config) {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Jobs { command }) => {
            let result = match command {
                JobsCommands::Init => commands::jobs::init(),
            };
            if let Err(e) = result {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Import { command }) => {
            #[allow(unreachable_patterns)]
            let result: Result<(), String> = match command {
                ImportCommands::Database {
                    url,
                    tables,
                    schema,
                    force,
                } => {
                    #[cfg(feature = "import")]
                    {
                        commands::import::database(
                            &url,
                            tables.as_deref(),
                            schema.as_deref(),
                            force,
                        )
                    }
                    #[cfg(not(feature = "import"))]
                    {
                        let _ = (url, tables, schema, force);
                        Err("The import command requires the import feature. \
                             Reinstall with: cargo install rapina-cli --features import-postgres"
                            .to_string())
                    }
                }
                #[cfg(feature = "import-openapi")]
                ImportCommands::Openapi {
                    file,
                    dry_run,
                    tags,
                } => commands::import_openapi::openapi(&file, tags.as_deref(), dry_run),
                _ => Err(
                    "No import subcommands available. Enable features like 'import-openapi'."
                        .to_string(),
                ),
            };
            if let Err(e) = result {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Doctor { host, port }) => {
            let config = commands::doctor::DoctorConfig { host, port };
            if let Err(e) = commands::doctor::execute(config) {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Test {
            coverage,
            watch,
            bless,
            filter,
        }) => {
            let config = commands::test::TestConfig {
                coverage,
                watch,
                bless,
                filter,
            };
            if let Err(e) = commands::test::execute(config) {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        None => {
            print_banner();
            println!();
            println!("Run {} for usage information.", "rapina --help".cyan());
        }
    }
}

fn print_banner() {
    println!();
    println!(
        "{}",
        "  ╭─────────────────────────────────────╮".bright_magenta()
    );
    println!(
        "{}",
        "  │                                     │".bright_magenta()
    );
    println!(
        "{}{}{}",
        "  │".bright_magenta(),
        "          🦀 Rapina CLI 🦀           ".bold(),
        "│".bright_magenta()
    );
    println!(
        "{}",
        "  │                                     │".bright_magenta()
    );
    println!(
        "{}",
        "  ╰─────────────────────────────────────╯".bright_magenta()
    );
}

fn print_version() {
    println!("rapina-cli {}", env!("CARGO_PKG_VERSION"));
}
