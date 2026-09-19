//! `orrery install <source>` and `orrery remove <name>` — bare top-level verbs.
//!
//! # Why they are top level
//!
//! Decided with the user and written into plan 15's Architecture section:
//! installing an extension is an everyday action, Pi-style, so it gets a verb of
//! its own. `orrery ext list` and `orrery ext test` stay under `ext`, because
//! they are not everyday actions.
//!
//! # `--to`, and why it is not spelled `--workspace`
//!
//! Plan 15 writes the layer flag as `--workspace`. It cannot be, in this
//! binary: `--workspace <PATH>` is already a **global** flag that says which
//! directory the workspace root is, and clap will not let one long name mean
//! two things. So the layer is `--to <user|workspace>`, with `--user` as the
//! bare shorthand the plan also asks for, and the default is still the user
//! layer. The plan is amended in place rather than caveated here.
//!
//! # Where the index comes from
//!
//! A local file: `--index <path>`, or the `index` in the managed layer's
//! `[registry]` table when that names a path. **Fetching a remote index is not
//! in this build** — that is a `net` call, and a `net` call goes through the
//! broker, which means a session. `orrery install <bare name>` with no index
//! configured says exactly that, and never falls through to another host.
//!
//! What it must not say is that it looked. Under a managed layer naming a
//! remote index it answered `buildgraph: not in the registry index
//! https://registry.corp.internal/orrery/index.toml` instantly and with no
//! fetch, which reads as "I looked and it is not there". The index was never
//! opened, so [`orrery_registry::RegistryError::RemoteIndex`] says that instead
//! and names `--index <path>`, which is the thing that does work. `local_index`
//! and the installer decide "is this remote" through one function,
//! [`orrery_registry::is_remote`], because two readings of one URL is how the
//! message came to be about a different code path than the one that ran.
//!
//! # A refused install is written down
//!
//! Section 8 phase 3 is "every decision is logged". A refused **load** reached
//! `orrery ledger` in round 9; a refused *install* exited 4 with a good message
//! and left nothing behind — `ledger`, `ledger --stream load` and
//! `ledger --subject ext:<name>` all answered "nothing recorded". [`install`]
//! now writes the same `ext.load` / `skipped` event a refused load writes, into
//! `<state-dir>/audit/install.jsonl`, so one question has one answer wherever
//! the refusal happened.
//!
//! The index itself is made by [`crate::cmd::registry`]: `orrery registry
//! init|add|sign|verify`. Until round 7 it was made by nothing at all, which
//! made "an admin pins a version set" a sentence about a file nobody here could
//! produce. `registry add` stages from [`mirror_dir`] - the same directory this
//! command's fetcher reads - so the hash an admin pins is the hash an install
//! computes.
//!
//! Implementation plan: `harness/docs/plans/15-registry-supply-chain.md`.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use orrery_proto::{LoadOutcome, SurfaceKind};
use orrery_registry::{
    Decision, GrantDiff, Index, InstallOptions, Installer, Keyring, Layout, ManagedRegistry,
    NoHooks, PublicKey, RegistryError, Source, SystemGit, Target, Timestamp,
};

use crate::args::{Cli, Layer};
use crate::exit::{Exit, fail};

/// `orrery install <source>`.
pub fn install(
    cli: &Cli,
    source: &str,
    to: Option<Layer>,
    link: bool,
    yes: bool,
    force: bool,
    index: Option<&Path>,
) -> ! {
    let source = Source::from_str(source).unwrap_or_else(|e| fail(Exit::Usage, e));
    let layout = layout(cli);
    let managed = managed_registry();
    let keyring = keyring(managed.as_ref());
    let index_path = index
        .map(Path::to_path_buf)
        .or_else(|| managed.as_ref().and_then(|m| local_index(&m.index)));
    let index_doc = index_path.as_deref().map(read_index);
    let index_url = managed
        .as_ref()
        .map(|m| m.index.clone())
        .or_else(|| index_path.as_ref().map(|p| p.display().to_string()))
        .unwrap_or_default();

    let state = state_dir(cli);
    let hooks = NoHooks;
    let git = SystemGit;
    let fetcher = orrery_registry::DirFetcher::new(mirror_dir(&state));

    let installer = Installer {
        layout: layout.clone(),
        index: index_doc.as_ref(),
        index_url,
        keyring: &keyring,
        now: now(),
        managed: managed.as_ref(),
        user_unpinned: None,
        fetcher: &fetcher,
        git: &git,
        hooks: &hooks,
        cache: state.join("registry/cache"),
        quarantine: state.join("registry/quarantine"),
    };

    // The grant diff has to be shown before anything is granted, and the
    // answers have to come from somewhere. On a terminal that is the person; in
    // CI it is the defaults, which deny.
    let options = InstallOptions {
        target: match to.unwrap_or(Layer::User) {
            Layer::User => Target::User,
            Layer::Workspace => Target::Workspace,
        },
        link,
        answers: Vec::new(),
        force,
        allow_all: yes,
    };

    let record = match installer.install(&source, &options) {
        Ok(record) => record,
        // A refusal to install is a **decision**, and section 8 phase 3 is
        // that every decision is logged. It used to exit 4 with a good message
        // and leave no trace at all: `ledger`, `ledger --stream load` and
        // `ledger --subject ext:<name>` all answered "nothing recorded" for an
        // install the managed layer had just refused.
        Err(e @ (RegistryError::Unpinned { .. } | RegistryError::RequiresMismatch { .. })) => {
            record_refusal(&state, &source, &e);
            fail(Exit::Denied, e)
        }
        Err(e) => fail(Exit::Usage, e),
    };

    print_diff(&record.diff, yes);
    println!(
        "installed {} {} into the {} layer at {}",
        record.ext,
        record.version,
        record.target.as_str(),
        record.path.display()
    );
    println!("{}", record.record.line());
    if let Some(warning) = &record.pin.warning {
        eprintln!("orrery: {warning}");
    }
    if let Some(sha) = &record.resolved {
        println!("resolved commit {sha}");
    }

    match &record.outcome {
        LoadOutcome::Degraded { problems, .. } => {
            for problem in problems {
                eprintln!("orrery: {problem}");
            }
            // Degraded is not a failure: the rest of the extension works.
            Exit::Ok.exit()
        }
        _ => Exit::Ok.exit(),
    }
}

/// Write a refused install into the load stream, where `orrery ledger` reads.
///
/// The same event a refused **load** writes — `ext.load` with
/// `status: "skipped"` — because they are the same kind of fact and an operator
/// asking "what did this harness decide about `buildgraph`" should not have to
/// know which command was running when it was decided. Best effort: a refusal
/// that cannot be written down is still a refusal, so nothing here can turn
/// exit 4 into exit 1.
fn record_refusal(state: &Path, source: &Source, why: &RegistryError) {
    let Ok(ext) = source.provisional_id().parse::<orrery_proto::ExtId>() else {
        return;
    };
    let dir = crate::session::audit_dir(state);
    // Not named after a session, because there is no session: an install is a
    // command, not a turn. `ledger` scans every `*.jsonl` in this directory.
    let Ok(sink) = orrery_audit::FileSink::open(dir.join("install.jsonl")) else {
        return;
    };
    orrery_audit::AuditSink::append(
        &sink,
        orrery_audit::AuditEvent::ExtensionLoad {
            ext,
            status: "skipped".to_owned(),
            contributions: Vec::new(),
            problems: vec![why.to_string()],
        },
    );
}

/// `orrery remove <name>`.
pub fn remove(cli: &Cli, name: &str, from: Option<Layer>) -> ! {
    let layout = layout(cli);
    let target = from.map(|l| match l {
        Layer::User => Target::User,
        Layer::Workspace => Target::Workspace,
    });
    match orrery_registry::remove(&layout, name, target) {
        Ok(path) => {
            println!("removed {name} from {}", path.display());
            Exit::Ok.exit()
        }
        // Installed at two layers is a question, not a preference: it names
        // both and refuses rather than guessing.
        Err(e) => fail(Exit::Usage, e),
    }
}

/// Print the grant diff the way §4.7 writes it, from the surface itself.
///
/// Rendering the `Surface` rather than the rows keeps one description of the
/// prompt: what a ratatui client draws and what this prints come from the same
/// value.
fn print_diff(diff: &GrantDiff, granted: bool) {
    let surface = diff.surface();
    let SurfaceKind::Stack { children, .. } = &surface.kind else {
        return;
    };
    for child in children {
        match &child.kind {
            SurfaceKind::Text { value, .. } => println!("{value}"),
            SurfaceKind::Question { prompt, .. } => println!("{prompt}"),
            _ => {}
        }
    }
    // `--yes` is `allow_all`: the defaults were not what decided anything, so
    // saying they denied something is a sentence about a code path that did not
    // run. It read "pass --yes to grant them" **after** --yes was passed, which
    // is the kind of line that makes a person doubt the install that just
    // succeeded.
    let denied = if granted {
        0
    } else {
        diff.defaults()
            .into_iter()
            .filter(|(_, d)| *d == Decision::Deny)
            .count()
    };
    if denied > 0 && !std::io::stdin().is_terminal() {
        let _ = std::io::stdout().flush();
        eprintln!(
            "orrery: {denied} capability request(s) were denied by default; pass --yes to grant them"
        );
    }
}

fn layout(cli: &Cli) -> Layout {
    let workspace = cli
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, e)));
    let user_dir = orrery_config::layer::user_dir().unwrap_or_else(|| {
        fail(
            Exit::Usage,
            "no home directory, so there is no user layer to install into; pass --to workspace",
        )
    });
    Layout::new(user_dir, workspace)
}

/// The mirror a package's bytes are staged from.
///
/// **One answer**, asked by the installer and by `orrery registry add`. An
/// admin pinning a hash computed from a directory the installer never reads
/// would be pinning nothing, and that is exactly the shape of defect this
/// round is about.
pub fn mirror_dir(state: &Path) -> PathBuf {
    state.join("registry/mirror")
}

pub fn state_dir(cli: &Cli) -> PathBuf {
    cli.state_dir.clone().unwrap_or_else(|| {
        orrery_config::layer::user_dir().unwrap_or_else(|| PathBuf::from(".orrery"))
    })
}

/// The keys the managed layer distributes, for a command with no managed
/// registry of its own in hand. `orrery registry verify` checks against the
/// very keyring an install would.
pub fn managed_keyring() -> Keyring {
    keyring(managed_registry().as_ref())
}

fn managed_registry() -> Option<ManagedRegistry> {
    let path = orrery_config::layer::managed_path();
    match ManagedRegistry::read(&path) {
        Ok(found) => found,
        Err(e) => fail(Exit::Usage, e),
    }
}

/// The keys the managed layer distributes.
///
/// One key, hex, with no window of its own: the index's `expires` is what
/// bounds it. Two keys with an overlap is what plan 15 open question 1 settles
/// on, and lands with a managed schema that can express one.
fn keyring(managed: Option<&ManagedRegistry>) -> Keyring {
    let Some(key) = managed.and_then(|m| m.key.as_deref()) else {
        return Keyring::default();
    };
    match PublicKey::new(
        "managed",
        key,
        "1970-01-01T00:00:00Z",
        "9999-12-31T23:59:59Z",
    ) {
        Ok(key) => Keyring::new(vec![key]),
        Err(e) => fail(Exit::Usage, e),
    }
}

/// The index URL, when it names a file this build can actually read.
///
/// "Remote" is [`orrery_registry::is_remote`] and not a second `contains`:
/// this function decides whether the installer is handed a document, and the
/// installer decides what to *say* when it was not. Two readings of the same
/// URL is how it came to say `not in the registry index <url>` for an index it
/// had never opened.
fn local_index(url: &str) -> Option<PathBuf> {
    if url.is_empty() || orrery_registry::is_remote(url) {
        return None;
    }
    Some(PathBuf::from(url))
}

fn read_index(path: &Path) -> Index {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| fail(Exit::Usage, format!("{}: {e}", path.display())));
    let index =
        Index::parse(&text, path.display().to_string()).unwrap_or_else(|e| fail(Exit::Usage, e));
    // The document signature is checked when one is next to it. An index with
    // no `.sig` is usable only for what it pins per entry, and the entry
    // signatures are checked regardless.
    let sig_path = path.with_extension("toml.sig");
    if let Ok(sig) = std::fs::read_to_string(&sig_path) {
        let keyring = keyring(managed_registry().as_ref());
        if let Err(e) = orrery_registry::verify::verify_index(
            &index,
            &text,
            sig.trim(),
            &keyring,
            &now(),
            &path.display().to_string(),
            &path.display().to_string(),
        ) {
            fail(Exit::Denied, e);
        }
    }
    index
}

/// The instant every expiry is checked against.
///
/// The one place in this crate that reads a clock: everything under
/// `orrery-registry` takes the instant as an argument, which is what lets its
/// tests pin an expiry with a literal.
pub fn now() -> Timestamp {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Timestamp::parse("now", &rfc3339(secs)).unwrap_or_else(|e| fail(Exit::Kernel, e))
}

/// Seconds since the epoch, as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Written out rather than taken from a date crate, because one conversion in
/// one direction is not worth a dependency, and `orrery-registry` deliberately
/// has no clock of its own to borrow.
pub fn rfc3339(secs: u64) -> String {
    let days = secs / 86_400;
    let rest = secs % 86_400;
    let (h, m, s) = (rest / 3600, (rest % 3600) / 60, rest % 60);

    // Civil-from-days, the standard algorithm, with the era shifted so the
    // arithmetic is unsigned for every date this program can see.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// That same instant, a number of days on. What `orrery registry init` gives a
/// fresh index for its validity window, through the one clock this crate has.
pub fn plus_days(days: u64) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    rfc3339(secs.saturating_add(days.saturating_mul(86_400)))
}

#[cfg(test)]
mod tests {
    use super::rfc3339;

    #[test]
    fn the_epoch_and_a_known_instant() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        // 2026-09-18T12:00:00Z
        assert_eq!(rfc3339(1_789_732_800), "2026-09-18T12:00:00Z");
    }
}
