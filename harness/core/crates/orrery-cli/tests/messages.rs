//! Every "run `orrery ...`" in the source names a command that exists.
//!
//! # The defect this closes
//!
//! With `--features anthropic` and no credential, the binary said:
//!
//! ```text
//! orrery: sign in first - no credential for the `anthropic` grant:
//!         run `orrery auth login anthropic`
//! ```
//!
//! and `orrery auth login anthropic` answered `error: unrecognized subcommand
//! 'auth'`. The message was written next to a flow that worked; the verb was
//! never added to the command tree; nothing in the repository compared the two.
//!
//! So this compares them, mechanically, for **every** such message: it reads
//! the verbs out of the built binary's own `--help` — not out of a list a test
//! keeps, which would be the same defect one level up — and then greps the
//! whole harness source tree for `run \`orrery ...\`` and checks each one.
//!
//! A new message naming a verb nobody wrote fails here, whichever crate it is
//! written in.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// What `--help` lists under `Commands:`, for the binary or one of its verbs.
fn verbs(path: &[&str]) -> BTreeSet<String> {
    let mut args: Vec<String> = path.iter().map(|s| (*s).to_owned()).collect();
    args.push("--help".to_owned());
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(&args)
        .env("COLUMNS", "200")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the orrery binary runs");
    let text = String::from_utf8_lossy(&out.stdout);
    let mut found = BTreeSet::new();
    let mut in_commands = false;
    for line in text.lines() {
        if line.trim_end().ends_with("Commands:") {
            in_commands = true;
            continue;
        }
        if in_commands {
            // The block ends at the first blank line, and every entry in it is
            // indented. Anything else is the next section.
            if line.trim().is_empty() {
                break;
            }
            let Some(word) = line.split_whitespace().next() else {
                continue;
            };
            if word.starts_with('-') {
                continue;
            }
            found.insert(word.to_owned());
        }
    }
    // The top level must have commands. A *verb* legitimately has none —
    // `orrery run` takes a prompt — and an empty set is how the caller tells.
    assert!(
        !path.is_empty() || !found.is_empty(),
        "`orrery --help` lists no commands: {text}"
    );
    found
}

/// Every `.rs` file under the harness tree, skipping build output.
fn sources() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the harness root");
    let mut out = Vec::new();
    walk(&root, &mut out);
    assert!(out.len() > 100, "the walk found the tree: {}", out.len());
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == "node_modules" || name.starts_with('.') {
                continue;
            }
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Everything between "run `orrery " and the closing backtick, with the file it
/// was written in.
fn claims() -> BTreeMap<String, Vec<String>> {
    const OPEN: &str = "run `orrery ";
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for file in sources() {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        // This file writes the pattern in its own prose; asserting about its
        // own examples would be circular.
        if file.ends_with("messages.rs") {
            continue;
        }
        let mut rest = text.as_str();
        while let Some(at) = rest.find(OPEN) {
            rest = &rest[at + OPEN.len()..];
            let Some(end) = rest.find('`') else { break };
            let claim = rest[..end].trim().to_owned();
            rest = &rest[end..];
            if claim.is_empty() {
                continue;
            }
            found
                .entry(claim)
                .or_default()
                .push(file.display().to_string());
        }
    }
    found
}

/// Whether a word is something a person types, rather than a placeholder or a
/// flag.
fn is_literal(word: &str) -> bool {
    !word.starts_with('-')
        && !word.starts_with('<')
        && !word.starts_with('[')
        && word != "..."
}

/// **The check.** Every message that tells a person to run something names a
/// verb — and a subverb, where it named one — that the binary has.
#[test]
fn every_message_names_a_command_that_exists() {
    let top = verbs(&[]);
    let claims = claims();
    assert!(
        !claims.is_empty(),
        "the binary does tell people to run things; the grep is broken"
    );

    let mut sub: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (claim, written_in) in &claims {
        let words: Vec<&str> = claim.split_whitespace().collect();
        let Some(verb) = words.first().copied().filter(|w| is_literal(w)) else {
            continue;
        };
        assert!(
            top.contains(verb),
            "`orrery {claim}` names no command this binary has.\n  \
             written in: {}\n  it has: {}",
            written_in.join(", "),
            top.iter().cloned().collect::<Vec<_>>().join(", ")
        );

        let Some(second) = words.get(1).copied().filter(|w| is_literal(w)) else {
            continue;
        };
        let known = sub
            .entry(verb.to_owned())
            .or_insert_with(|| verbs(&[verb]));
        // A verb with no subcommands of its own takes arguments instead, and
        // this claim's second word is one of them.
        if known.is_empty() {
            continue;
        }
        // Likewise: `orrery auth login anthropic`'s second word is the
        // subcommand, but `orrery install ./x`'s is an argument. Only check a
        // second word when the verb actually has subcommands.
        assert!(
            known.contains(second),
            "`orrery {claim}` names no `{verb}` subcommand.\n  \
             written in: {}\n  `{verb}` has: {}",
            written_in.join(", "),
            known.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
}

/// And the one that started it, spelled out, so a regression reads as itself.
#[test]
fn the_sign_in_message_names_a_verb_that_exists() {
    assert!(
        verbs(&[]).contains("auth"),
        "the binary tells people to run `orrery auth login <provider>`"
    );
    assert!(
        verbs(&["auth"]).contains("login"),
        "...and `login` is one of its subcommands"
    );
}
