//! One module per subcommand, as `17-cli.md` lays them out. Each lands with
//! its own plan; until then it exits 2 naming that plan.

pub mod attach;
pub mod config;
pub mod eval;
pub mod ext;
pub mod import;
pub mod init;
pub mod install;
pub mod interactive;
pub mod layers;
pub mod ledger;
pub mod mcp;
pub mod permissions;
pub mod replay;
pub mod run;
pub mod serve;
pub mod session;
pub mod skills;

use std::path::PathBuf;

use orrery_kernel::KernelConfig;

use crate::args::Cli;
use crate::exit::{Exit, fail};
use crate::session::Setup;

/// Turn the global flags into something [`Session::build`] can use.
///
/// [`Session::build`]: crate::session::Session::build
pub fn setup(cli: &Cli) -> Setup {
    let workspace = cli.workspace.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, format!("no workspace: {e}")))
    });
    let fixtures: Vec<PathBuf> = cli
        .provider
        .iter()
        .map(|spec| match spec.strip_prefix("fixture:") {
            Some(path) if !path.is_empty() => PathBuf::from(path),
            _ => fail(
                Exit::Usage,
                format!(
                    "`{spec}` is not a provider: this build understands `fixture:<path-to.jsonl>`"
                ),
            ),
        })
        .collect();
    let state_dir = cli
        .state_dir
        .clone()
        .unwrap_or_else(|| workspace.join(".orrery"));
    // The layers on disk, folded into the shape the kernel runs on. This is the
    // one path from a config file to a running turn: without it `maxUsd` is
    // inert, the retry policy is whatever was compiled in, and `--profile` picks
    // a name nothing reads. `layers::resolve` exits 2 on a file that will not
    // parse, which is the right answer for something a person wrote.
    let resolved = layers::resolve(cli);
    let kernel = orrery_harness::kernel_config(
        &resolved.values,
        &resolved.profile.name,
        KernelConfig {
            model: "fixture".to_owned(),
            ..KernelConfig::default()
        },
    );

    Setup {
        workspace,
        state_dir,
        profile: cli.profile.clone().unwrap_or_else(|| "default".to_owned()),
        fixtures,
        kernel,
        // What `orrery ext list` prints and what a turn can actually call have
        // to be the same set, or installing an extension is theatre.
        extensions: orrery_harness::extension_sources(&resolved),
    }
}

/// Build a session, or exit with the code the failure deserves.
pub fn session(cli: &Cli) -> crate::session::Session {
    match crate::session::Session::build(&setup(cli)) {
        Ok(session) => session,
        Err(e @ crate::session::SetupError::NoProvider) => fail(Exit::Usage, e),
        Err(e) => fail(Exit::Kernel, e),
    }
}
