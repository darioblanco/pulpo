#[cfg(not(coverage))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use clap::Parser;

    let cli = pulpo_cli::Cli::parse();
    let output = pulpo_cli::execute(&cli).await?;
    if pulpo_cli::should_print_output(&output) {
        println!("{output}");
    }
    Ok(())
}

// Dummy main for coverage builds — the real logic is tested via lib.rs
#[cfg(coverage)]
fn main() {}
