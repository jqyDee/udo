use clap::Parser;
use udo::{cli::Cli, dir::root_dir, model::tree::Tree, storage::Storage};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = async {
        let root_dir = root_dir()?;
        let mut tree = Tree::load_from(&root_dir).await?;
        let storage = Storage::open_db(&root_dir)?;
        cli.execute(&mut tree, &storage).await
    }
    .await;

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
