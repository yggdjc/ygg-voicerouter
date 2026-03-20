//! voicerouter CLI entry point.

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "voicerouter", version, about = "Voice router for Linux")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Enable verbose logging.
    #[arg(short, long)]
    verbose: bool,

    /// Path to config file.
    #[arg(short, long)]
    config: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run first-time setup.
    Setup,
    /// Get or set a config value.
    Config {
        key: Option<String>,
        value: Option<String>,
    },
    /// Control the background service.
    Service { action: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let log_level = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    let config = voicerouter::config::Config::load(cli.config.as_deref())?;
    log::debug!("loaded config: {config:?}");

    match cli.command {
        None => {
            log::info!("No subcommand given. Use --help for usage.");
        }
        Some(Commands::Setup) => {
            log::info!("Setup not yet implemented.");
        }
        Some(Commands::Config { key, value }) => {
            log::info!("Config key={key:?} value={value:?} — not yet implemented.");
        }
        Some(Commands::Service { action }) => {
            log::info!("Service action={action} — not yet implemented.");
        }
    }

    Ok(())
}
