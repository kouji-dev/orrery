//! One module per subcommand, as `17-cli.md` lays them out. Each lands with
//! its own plan; until then it exits 2 naming that plan.

pub mod attach;
pub mod config;
pub mod eval;
pub mod ext;
pub mod import;
pub mod init;
pub mod ledger;
pub mod permissions;
pub mod replay;
pub mod run;
pub mod serve;
pub mod session;
