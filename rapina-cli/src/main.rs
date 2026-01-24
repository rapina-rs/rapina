//! Rapina CLI - Command line tool for the Rapina web framework.

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
        Some(Commands::New { name }) => {
            if let Err(e) = commands::new::execute(&name) {
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
