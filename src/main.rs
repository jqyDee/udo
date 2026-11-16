use clap::Parser;
use udo::{
    Res,
    cli::Cli,
    core::Core,
    dir::{default_root, dirs, is_same_root},
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = async {
        let dirs = dirs()?;
        refuse_real_data_in_debug(&dirs.root)?;
        let mut core = Core::open(&dirs).await?;
        cli.execute(&mut core, &std::env::current_dir()?).await
    }
    .await;

    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// A debug build never opens the real data (`~/.config/udo`), however it
/// was started or spelled (`is_same_root`: also a symlink, `..`): `cargo
/// run` sets `UDO_ROOT` (`.cargo/config.toml`), a `target/debug/udo`
/// started by hand or by a hook must set it too. Release builds (the
/// installed udo) skip the check.
fn refuse_real_data_in_debug(root: &std::path::Path) -> Res<()> {
    if cfg!(debug_assertions) && is_same_root(root, &default_root()?) {
        return Err(format!(
            "a debug build refuses the real data at {}: point {} at a test root \
             (scripts/seed-testdata.sh makes /tmp/udo-test)",
            root.display(),
            udo::ROOT_ENV
        )
        .into());
    }
    Ok(())
}
