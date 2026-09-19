//! `orrery init` - write a starter configuration into the workspace.
//!
//! A thin wrapper where there is a profile to scaffold, on purpose:
//! [`orrery_config::import::init`] expands that profile's shorthands into real
//! rules, so the file a person ends up reviewing says what it actually does.
//! All this adds is where the file goes and the refusal to write over one that
//! is already there.
//!
//! # With no `--profile`, added 2026-09-19
//!
//! There is nothing to scaffold *from*, and what this wrote was a 49-byte file
//! whose entire content was
//! ``# Written by `orrery init` from the `` profile.`` - an empty profile name
//! interpolated into a comment - followed by "review it before you use it".
//! This is the first command a new user runs.
//!
//! It now writes [`starter`]: a documented configuration in the real grammar,
//! with the permission aspects spelled out and a commented profile to copy.
//! **The rules in it are not written out here.** They come from
//! [`orrery_config::profile::SHORTHANDS`] - the same table `import::init`
//! expands a profile's shorthands through - so a starter config cannot come to
//! contain a rule the parser no longer accepts.
//!
//! A `--profile` that names nothing is still an error, and so is a second
//! `init`: nothing here overwrites a file a person wrote.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md` task 8,
//! surfaced by `harness/docs/plans/17-cli.md` task 9.

use std::collections::BTreeSet;

use orrery_config::profile::SHORTHANDS;
use orrery_config::{CONFIG_DIR, CONFIG_FILE, ResolvedConfig};

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

    // With a profile, the file says what that profile does. Without one there
    // is nothing to expand, so the file has to document itself instead.
    let text = if resolved.profile.name.is_empty() {
        let defined = profiles(&resolved);
        if defined.is_empty() {
            eprintln!(
                "orrery: no profile named, and none is defined yet - writing a starter configuration"
            );
        } else {
            eprintln!(
                "orrery: no profile named, so this is a starter configuration; `orrery --profile <name> init` scaffolds one of: {}",
                defined.into_iter().collect::<Vec<_>>().join(", ")
            );
        }
        starter()
    } else {
        orrery_config::import::init(&resolved.profile)
    };
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

/// Every profile the layers in force define, whatever layer defined it.
///
/// Read out of the resolved values rather than by a second walk of the files:
/// the names `--profile` would accept are exactly the ones
/// `orrery_config::resolve` already merged.
fn profiles(resolved: &ResolvedConfig) -> BTreeSet<String> {
    resolved
        .values
        .keys_under("profile")
        .into_iter()
        .filter_map(|key| key.strip_prefix("profile."))
        .filter_map(|rest| rest.split('.').next())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// The configuration a workspace with no profile starts from.
///
/// Documented, in the real grammar, and **conservative**: the only thing
/// allowed outright is reading the workspace. Everything that leaves a mark -
/// writing, spawning, the network - is an `ask`, and credentials are denied,
/// because a file nobody has read yet should not be the file that granted
/// something.
///
/// The rule texts come from [`SHORTHANDS`], so this cannot drift from the
/// grammar the parser accepts.
fn starter() -> String {
    let rule = |key: &str| {
        SHORTHANDS
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, _, text)| *text)
            .unwrap_or_else(|| {
                fail(
                    Exit::Kernel,
                    format!("`{key}` is no longer a permission shorthand"),
                )
            })
    };
    let list = |keys: &[&str]| {
        keys.iter()
            .map(|key| format!("\"{}\"", rule(key)))
            .collect::<Vec<_>>()
            .join(", ")
    };

    format!(
        r##"# Written by `orrery init`. Review it before you use it.
#
# This is the **workspace layer**. Four more layers are in force around it -
# built-in defaults, your user layer (~/.orrery/config.toml), the managed layer
# an administrator controls, and the flags you type - and
# `orrery config explain <key>` says which one won, and on which line.
#
# Nothing below is required. An empty file is a valid configuration.

# Which model a turn runs. Without one, the provider's own default.
# model = "claude-sonnet-4-5"

# What the agent may do, in the rule grammar. `orrery permissions explain` takes
# one call - `orrery permissions explain "write(./src/main.rs)"` - and answers
# with the rule, the layer, the file and the line that decided it.
[permissions]
# Walked first, from every layer. Nothing below carves an exception out of a
# deny, and a managed deny cannot be relaxed here at all.
deny = [{deny}]
# Asked once, and the answer is recorded in the ledger.
ask = [{ask}]
# Allowed without asking.
allow = [{allow}]

# A profile is a named composition: a model, a tool set, an extension list and
# its own permissions. Two profiles out of one binary are two measurably
# different agents. Uncomment, rename, and run it with
# `orrery --profile review run -p "..."`.
#
# [profile.review]
# model = "claude-sonnet-4-5"
# extensions = ["git"]
# consent = "ask"
# [profile.review.permissions]
# read = true
# write = false
"##,
        deny = list(&["creds"]),
        ask = list(&["write", "spawn", "net"]),
        allow = list(&["read"]),
    )
}

#[cfg(test)]
mod tests {
    use super::starter;

    /// The starter config is a configuration, not a comment: it parses, and it
    /// carries the aspects a person came here to set.
    #[test]
    fn the_starter_is_real_toml_with_real_rules() {
        let text = starter();
        let doc: toml::Value = text.parse().expect("the starter config parses");
        let permissions = doc
            .get("permissions")
            .and_then(toml::Value::as_table)
            .expect("it has a permissions table");
        for (list, expected) in [
            ("deny", "creds(*)"),
            ("ask", "write(./**)"),
            ("allow", "read(./**)"),
        ] {
            let rules = permissions[list]
                .as_array()
                .unwrap_or_else(|| panic!("`{list}` is a list"));
            assert!(
                rules.iter().any(|r| r.as_str() == Some(expected)),
                "`{list}` is missing `{expected}`: {text}"
            );
        }
    }
}
