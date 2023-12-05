use std::path::PathBuf;

use areum::site::Site;
use clap::Parser;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, default_value = "dist")]
    out: PathBuf,

    input: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let args = Args::parse();

    let root = args.input.unwrap_or(std::env::current_dir()?);
    let mut site = Site::new_with_root(&root)?;
    site.read_root()?;
    site.render_to_fs(&args.out).await?;

    Ok(())
}
