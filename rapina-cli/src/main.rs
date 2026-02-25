//! Rapina CLI - Command line tool for the Rapina web framework.

mod colors;
mod commands;

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
        /// Skip AI assistant config files (AGENT.md, .claude/, .cursor/)
        #[arg(long)]
        no_ai: bool,
    },
    /// Start development server with hot reload
    Dev {
        /// Port to listen on
        #[arg(short, long, default_value = "3000")]
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
    Routes,
    /// Database migration tools
    Migrate {
        #[command(subcommand)]
        command: MigrateCommands,
    },
    /// Run health checks on your API
    Doctor,
    /// Add components to your Rapina project
    Add {
        #[command(subcommand)]
        command: AddCommands,
    },
    /// Run tests with pretty output
    Test {
        /// Generate coverage report (requires cargo-llvm-cov)
        #[arg(long)]
        coverage: bool,
        /// Watch for changes and re-run tests
        #[arg(short, long)]
        watch: bool,
        /// Filter tests by name
        filter: Option<String>,
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
enum AddCommands {
    /// Generate a new CRUD resource (handlers, DTOs, error type, entity, migration)
    Resource {
        /// Name of the resource (lowercase, e.g., user, blog_post)
        name: String,
        /// Fields in name:type format (e.g., title:string active:bool)
        fields: Vec<String>,
    },
}

#[derive(Subcommand)]
enum OpenapiCommands {
    /// Export OpenAPI spec to stdout or file
    Export {
        /// Output file path (stdout if not specified)
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Check if openapi.json matches the current code
    Check {
        /// Path to openapi.json file
        #[arg(default_value = "openapi.json")]
        file: String,
    },
    /// Compare spec with another branch and detect breaking changes
    Diff {
        /// Base branch to compare against
        #[arg(short, long)]
        base: String,
        /// Path to openapi.json file
        #[arg(default_value = "openapi.json")]
        file: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Version) => {
            print_version();
        }
        Some(Commands::New { name, no_ai }) => {
            if let Err(e) = commands::new::execute(&name, no_ai) {
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
        Some(Commands::Migrate { command }) => {
            let result = match command {
                MigrateCommands::New { name } => commands::migrate::new_migration(&name),
            };
            if let Err(e) = result {
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
        Some(Commands::Openapi { command }) => {
            let result = match command {
                OpenapiCommands::Export { output } => commands::openapi::export(output),
                OpenapiCommands::Check { file } => commands::openapi::check(&file),
                OpenapiCommands::Diff { base, file } => commands::openapi::diff(&base, &file),
            };
            if let Err(e) = result {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Routes) => {
            if let Err(e) = commands::routes::execute() {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Doctor) => {
            if let Err(e) = commands::doctor::execute() {
                eprintln!("{} {}", "Error:".red().bold(), e);
                std::process::exit(1);
            }
        }
        Some(Commands::Test {
            coverage,
            watch,
            filter,
        }) => {
            let config = commands::test::TestConfig {
                coverage,
                watch,
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
