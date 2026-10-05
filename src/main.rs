use clap::Parser;
use udo::{
    Res,
    cli::Cli,
    core::Core,
    dir::{is_same_root, root_dir, root_dir_from},
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let result = async {
        let root = root_dir()?;
        refuse_real_data_in_debug(&root)?;
        let mut core = Core::open(&root).await?;
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
    if cfg!(debug_assertions) && is_same_root(root, &root_dir_from(None)?) {
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
