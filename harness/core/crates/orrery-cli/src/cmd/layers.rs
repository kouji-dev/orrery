//! Resolving the five configuration layers, for the commands that answer from
//! them rather than from a running kernel.
//!
//! `permissions explain`, `config explain` and `init` all need the same thing —
//! the layers on disk, merged, with provenance — and none of them needs a
//! model, a session or a provider. That is the whole reason they are here and
//! not on [`Session`](crate::session::Session): explaining a rule must work in
//! a repository with no API key in sight, which is also why every test for them
//! runs with a sandboxed `HOME` and no `--provider`.

use orrery_config::{ConfigPaths, ResolvedConfig, StartupCtx};
use orrery_policy::ResolvedRules;

use crate::args::Cli;
use crate::exit::{Exit, fail};

/// Resolve the layers for the workspace the flags name.
///
/// Exits 2 when a layer will not parse or a named profile does not exist —
/// both are things the person typed or wrote, not failures of the harness. The
/// error already names the file and the line.
pub fn resolve(cli: &Cli) -> ResolvedConfig {
    resolve_with(cli, cli.profile.as_deref())
}

/// The same, for a command with a `--profile` of its own — `init` — where the
/// subcommand's flag wins over the global one.
pub fn resolve_with(cli: &Cli, profile: Option<&str>) -> ResolvedConfig {
    let workspace = cli.workspace.clone().unwrap_or_else(|| {
        std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, format!("no workspace: {e}")))
    });
    let mut ctx = StartupCtx::new(ConfigPaths::for_workspace(&workspace));
    if let Some(profile) = profile {
        ctx = ctx.with_profile(profile.to_owned());
    }
    let resolved = orrery_config::resolve(&ctx).unwrap_or_else(|e| fail(Exit::Usage, e));

    // Narration, on stderr: a workspace that is not trusted answers from the
    // user layer alone, and somebody staring at an answer that ignores the file
    // in front of them deserves to be told why.
    if !resolved.trust.is_trusted() {
        eprintln!(
            "orrery: {} is not trusted, so its own configuration is not in force ({})",
            workspace.display(),
            resolved.trust_why.why
        );
    }
    resolved
}

/// The permission rules in force for these flags.
///
/// **The one place the rules are chosen.** `permissions explain` prints what
/// this returns and `cmd::setup` hands the very same set to the kernel, so the
/// verdict a person is shown and the decision a turn makes cannot disagree.
/// They did, for every round before this one: the explanation read the layers
/// and the kernel ran on a hardcoded default, which is a permission system that
/// reports a denial it does not enforce.
///
/// `--profile` folds that profile's permission shorthands in, on both sides.
#[must_use]
pub fn rules(cli: &Cli, resolved: &ResolvedConfig) -> ResolvedRules {
    let rules = if cli.profile.is_some() {
        resolved.rules_in_force()
    } else {
        resolved.policy()
    };
    rules.unwrap_or_else(|e| fail(Exit::Usage, e))
}

/// Whether the person asked for machine-readable output.
#[must_use]
pub fn wants_json(cli: &Cli) -> bool {
    cli.json || matches!(cli.ui, Some(crate::ui::Ui::Json))
}
