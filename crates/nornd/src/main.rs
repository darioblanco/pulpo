use anyhow::Result;
use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

mod api;
mod backend;
mod config;
mod peers;
mod platform;
mod session;
mod store;

#[derive(Parser)]
#[command(name = "nornd", about = "Norn daemon — agent session orchestrator")]
struct Cli {
    /// Config file path
    #[arg(long, default_value = "~/.norn/config.toml")]
    config: String,

    /// Port to listen on (overrides config)
    #[arg(short, long)]
    port: Option<u16>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("nornd=info".parse()?))
        .init();

    let cli = Cli::parse();
    info!("Starting nornd...");

    let config = config::load(&cli.config)?;
    let port = cli.port.unwrap_or(config.node.port);

    let store = store::Store::new(&config.data_dir()).await?;
    store.migrate().await?;

    let state = api::AppState::new(config, store);
    let app = api::router(state);

    let addr = format!("0.0.0.0:{port}");
    info!("Listening on {addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
