use std::path::PathBuf;

use areum::{
    server::{Command, Server},
    builder::Builder,
};
use clap::{Parser, Subcommand};
use notify::{event::ModifyKind, Event, EventKind, RecursiveMode, Watcher};

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
            let mut site = Builder::new(&root).await?;
            site.build(&out).await?;
        }
        Commands::Serve { address, input } => {
            let root = input.unwrap_or(std::env::current_dir()?);
            let server = Server::new(&root)?;
            let tx = server.tx_cmd.clone();

            let mut watcher =
                notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
                    Ok(event) => match event.kind {
                        EventKind::Create(_)
                        | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_))
                        | EventKind::Remove(_) => {
                            tx.blocking_send(Command::Restart);
                        }
                        _ => {}
                    },
                    Err(e) => println!("watch error: {:?}", e),
                })?;
            watcher.watch(&root, RecursiveMode::Recursive)?;

            server.serve(&address).await?;
        }
    }

    Ok(())
}
