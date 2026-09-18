//! `orrery init` - write a starter configuration into the workspace.
//!
//! A thin wrapper, on purpose: [`orrery_config::import::init`] expands the
//! profile's shorthands into real rules, so the file a person ends up reviewing
//! says what it actually does. All this adds is where the file goes and the
//! refusal to write over one that is already there.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md` task 8,
//! surfaced by `harness/docs/plans/17-cli.md` task 9.

use orrery_config::{CONFIG_DIR, CONFIG_FILE};

use crate::args::Cli;
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Scaffold configuration for a profile.
pub fn dispatch(cli: &Cli, profile: Option<&str>) -> ! {
    // `--profile` is global and `init --profile` is the same question asked
    // twice; the subcommand's own flag wins when both are given.
    let resolved = layers::resolve_with(cli, profile.or(cli.profile.as_deref()));

    let dir = resolved.root.join(CONFIG_DIR);
    let path = dir.join(CONFIG_FILE);
    if path.exists() {
        fail(
            Exit::Usage,
            format!(
                "{} already exists. Edit it, or `orrery config explain <key>` to see what is in force",
                path.display()
            ),
        );
    }

    let text = orrery_config::import::init(&resolved.profile);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        fail(Exit::Kernel, format!("{}: {e}", dir.display()));
    }
    if let Err(e) = std::fs::write(&path, text) {
        fail(Exit::Kernel, format!("{}: {e}", path.display()));
    }

    eprintln!("orrery: review it before you use it");
    // stdout is data: the path, so `orrery init` composes with an editor.
    println!("{}", path.display());
    Exit::Ok.exit()
}
