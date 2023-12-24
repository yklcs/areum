use std::path::PathBuf;

use areum::{server::Server, site::Site};
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Build {
        #[arg(short, long, default_value = "dist")]
        out: PathBuf,
        input: Option<PathBuf>,
    },
    Serve {
        #[arg(short, long, default_value = "0.0.0.0:8000")]
        address: String,
        input: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build { out, input } => {
            let root = input.unwrap_or(std::env::current_dir()?);
            let mut site = Site::new(&root).await?;
            site.read_root()?;
            site.render_to_fs(&out).await?;
        }
        Commands::Serve { address, input } => {
            let root = input.unwrap_or(std::env::current_dir()?);
            let server = Server::new(&root)?;
            server.serve(&address).await?;
        }
    }

    Ok(())
}
