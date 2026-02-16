use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "norn", about = "Manage agent sessions across your machines")]
struct Cli {
    /// Target node (default: localhost)
    #[arg(long, default_value = "localhost:7433")]
    node: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Spawn a new agent session
    Spawn {
        /// Repository path
        #[arg(long)]
        repo: String,

        /// Agent provider (claude, codex, aider)
        #[arg(long, default_value = "claude")]
        provider: String,

        /// Task prompt
        prompt: Vec<String>,
    },

    /// List all sessions
    List,

    /// Show session logs/output
    Logs {
        /// Session name or ID
        name: String,
    },

    /// Kill a session
    Kill {
        /// Session name or ID
        name: String,
    },

    /// Resume a stale session
    Resume {
        /// Session name or ID
        name: String,
    },

    /// List all known nodes
    Nodes,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let base_url = format!("http://{}", cli.node);

    match cli.command {
        Commands::List => {
            let resp = reqwest::get(format!("{base_url}/api/v1/sessions"))
                .await?
                .text()
                .await?;
            println!("{resp}");
        }
        Commands::Nodes => {
            let resp = reqwest::get(format!("{base_url}/api/v1/node"))
                .await?
                .text()
                .await?;
            println!("{resp}");
        }
        _ => {
            println!("Not yet implemented");
        }
    }

    Ok(())
}
