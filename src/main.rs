use std::path::PathBuf;

use anyhow::anyhow;
use areum::{
    builder::Builder,
    server::{Command, Server},
};
use clap::{Parser, Subcommand};
use notify::{event::ModifyKind, Event, EventKind, RecursiveMode, Watcher};
use tokio::signal;

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
            let (server, tx) = Server::new(&root)?;

            let tx_ = tx.clone();
            let mut watcher =
                notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
                    Ok(event) => match event.kind {
                        EventKind::Create(_)
                        | EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_))
                        | EventKind::Remove(_) => {
                            tx_.send(Command::Restart).or(Err("")).unwrap();
                        }
                        _ => {}
                    },
                    Err(e) => println!("watch error: {:?}", e),
                })?;
            watcher.watch(&root, RecursiveMode::Recursive)?;

            tokio::spawn(async move {
                signal::ctrl_c()
                    .await
                    .map_err(|err| anyhow!(err))
                    .and_then(|_| {
                        tx.send(Command::Stop)
                            .unwrap_or_else(|_| panic!("error sending to channel"));
                        Ok(())
                    })
            });

            server.serve(&address).await?;
        }
    }

    Ok(())
}
