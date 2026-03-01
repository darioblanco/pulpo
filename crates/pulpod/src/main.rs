#[cfg(not(coverage))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;
    use tracing::info;

    let cli = pulpod::Cli::parse();

    match &cli.command {
        Some(pulpod::CliCommand::Mcp) => {
            // MCP mode: no tracing to stdout (would corrupt STDIO protocol)
            let server = pulpod::build_mcp_server(&cli).await?;
            pulpod::mcp::run_stdio(server).await?;
        }
        None => {
            // HTTP daemon mode
            pulpod::init_tracing()?;
            let (app, addr, shutdown_handle) = pulpod::build_app(&cli).await?;
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal(shutdown_handle))
                .await?;
            info!("pulpod shut down cleanly");
        }
    }

    Ok(())
}

/// Wait for SIGTERM or Ctrl+C, then signal all background loops to stop.
#[cfg(not(coverage))]
async fn shutdown_signal(handle: pulpod::ShutdownHandle) {
    use tracing::info;

    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    info!("Shutdown signal received, stopping background tasks...");
    handle.shutdown();
}

// Dummy main for coverage builds — the real logic is tested via lib.rs
#[cfg(coverage)]
fn main() {}
