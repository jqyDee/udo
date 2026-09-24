use clap::Parser;
use udo::{cli::Cli, model::tree::Tree};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = async {
        let mut tree = Tree::load().await?;
        cli.execute(&mut tree).await
    }
    .await;

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
