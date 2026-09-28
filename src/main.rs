use clap::Parser;
use udo::{cli::Cli, core::Core, dir::root_dir};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = async {
        let mut core = Core::open(&root_dir()?).await?;
        cli.execute(&mut core).await
    }
    .await;

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
