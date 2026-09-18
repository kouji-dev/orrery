//! One module per subcommand, as `17-cli.md` lays them out. Each lands with
//! its own plan; until then it exits 2 naming that plan.

pub mod attach;
pub mod config;
pub mod eval;
pub mod ext;
pub mod import;
pub mod interactive;
pub mod init;
pub mod layers;
pub mod ledger;
pub mod permissions;
pub mod replay;
pub mod run;
pub mod serve;
pub mod session;

use std::path::PathBuf;

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
    Setup {
        workspace,
        state_dir,
        profile: cli.profile.clone().unwrap_or_else(|| "default".to_owned()),
        fixtures,
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
