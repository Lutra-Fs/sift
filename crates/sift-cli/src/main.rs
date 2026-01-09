//! Sift - MCP & Skills Manager
//!
//! Usage:
//!   sift              # Launch TUI (default)
//!   sift install ...  # CLI operations
//!   sift --gui        # Launch GUI

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "sift")]
#[command(about = "MCP & Skills Manager", long_about = None)]
struct Cli {
    /// Launch GUI interface
    #[arg(long, short)]
    gui: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Install an MCP server or skill
    Install {
        /// What to install (mcp or skill)
        kind: String,
        /// Name/ID of the package to install
        name: String,
    },
    /// Uninstall an MCP server or skill
    Uninstall {
        /// What to uninstall (mcp or skill)
        kind: String,
        /// Name/ID of the package to uninstall
        name: String,
    },
    /// List MCP servers or skills
    List {
        /// What to list (mcp or skill)
        kind: Option<String>,
    },
    /// Set configuration scope
    Config {
        /// Configuration scope (global, local, shared)
        scope: String,
    },
}

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sift=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    // Route to appropriate interface
    if cli.gui {
        run_gui();
    } else if cli.command.is_some() {
        run_cli(cli.command.unwrap());
    } else {
        run_tui()?;
    }

    Ok(())
}

fn run_cli(command: Commands) {
    match command {
        Commands::Install { kind, name } => {
            println!("Installing {kind}: {name}");
        }
        Commands::Uninstall { kind, name } => {
            println!("Uninstalling {kind}: {name}");
        }
        Commands::List { kind } => match kind.as_deref() {
            Some("mcp") => println!("Listing MCP servers"),
            Some("skill") => println!("Listing skills"),
            Some(_) | None => println!("Listing all"),
        },
        Commands::Config { scope } => {
            println!("Setting config scope to: {scope}");
        }
    }
}

fn run_tui() -> Result<()> {
    println!("Launching TUI...");
    // The TUI will be implemented in sift-tui crate
    // For now, just stub it
    Ok(())
}

fn run_gui() {
    println!("Launching GUI...");
    // The GUI will be implemented in sift-gui crate
    // For now, just stub it
}
