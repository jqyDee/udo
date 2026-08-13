use clap::Parser;
use udo::{cli::Cli, config::AppConfig};

#[tokio::main]
async fn main() {
    let mut app_config = AppConfig::load_or_create()
        .await
        .expect("Error loading App Config");
    let cli = Cli::parse();
    cli.execute(&mut app_config).await;
}
