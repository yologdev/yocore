//! Yocore CLI - standalone server for AI coding session management

use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use yocore::{Config, Core};

#[derive(Parser, Debug)]
#[command(name = "yocore")]
#[command(author = "Yolog Team")]
#[command(version)]
#[command(about = "Yocore - headless service for AI coding sessions", long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "~/.yolog/config.toml")]
    config: PathBuf,

    /// Run in MCP server mode (stdio)
    #[arg(long)]
    mcp: bool,

    /// Override server port
    #[arg(short, long)]
    port: Option<u16>,

    /// Override server host
    #[arg(long)]
    host: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Initialize a new config file with defaults
    #[arg(long)]
    init: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("yocore={},tower_http=debug", log_level).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Handle --init flag
    if args.init {
        let config_path = expand_path(&args.config);
        if config_path.exists() {
            tracing::warn!("Config file already exists: {}", config_path.display());
            return Ok(());
        }
        Config::create_default(&config_path)?;
        tracing::info!("Created default config at: {}", config_path.display());
        return Ok(());
    }

    // Load configuration
    let config_path = expand_path(&args.config);
    let mut config = if config_path.exists() {
        Config::from_file(&config_path)?
    } else {
        tracing::warn!(
            "Config file not found at {}, using defaults",
            config_path.display()
        );
        Config::default()
    };

    // Apply CLI overrides
    if let Some(port) = args.port {
        config.server.port = port;
    }
    if let Some(host) = args.host {
        config.server.host = host;
    }

    // Create core instance
    let core = Core::new(config, config_path)?;

    if args.mcp {
        // MCP server mode - communicate over stdio
        tracing::info!("Starting MCP server mode");
        yocore::mcp::run_mcp_server(core).await?;
    } else {
        // HTTP server mode
        tracing::info!("Starting HTTP server mode");

        // Start file watcher if watch paths are configured
        if !core.config.watch.is_empty() {
            tracing::info!(
                "Starting file watcher for {} watch paths",
                core.config.watch.len()
            );
            core.start_watching().await?;
        }

        // Start periodic background tasks (ranking, duplicate cleanup, embedding refresh)
        core.start_periodic_tasks();

        // Recover pending AI tasks (title, memory, skills) from previous sessions
        core.recover_pending_ai_tasks().await;

        // Start API server (blocks until shutdown)
        core.start_api_server().await?;
    }

    Ok(())
}

/// Expand ~ to home directory
fn expand_path(path: &PathBuf) -> PathBuf {
    if path.starts_with("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(path.strip_prefix("~").unwrap());
        }
    }
    path.clone()
}
