//! `orrery trust` — the verb the trust store never had.
//!
//! DEFECT 10: `orrery init` wrote a workspace config that **nothing could
//! activate**. The workspace layer only loads for a trusted workspace,
//! `TrustStore::record` had exactly one caller inside `orrery_config::resolve`,
//! and there was no CLI verb at all — no `orrery trust`, no `config trust`, no
//! `--trust`. The only way to make the file `init` had just written take effect
//! was to hand-edit `trust.auto = true` into the user layer, which nothing
//! documented.
//!
//! Everything here drives the built binary. **No network request, no key.**

mod common;

use common::{args, home_with, orrery_in, quiet};

/// The whole path, in the order a person walks it: `init` writes the file,
/// `trust grant` makes it load, and `config explain` answers from it.
#[test]
fn init_then_trust_then_the_config_is_in_force() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["init"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Before the grant, the file exists and is not in force.
    let path = ws.path().join(".orrery/config.toml");
    assert!(path.is_file(), "init wrote the workspace layer");
    std::fs::write(
        &path,
        "model = \"from-the-workspace\"\n[permissions]\nallow = [\"read(./**)\"]\n",
    )
    .expect("the workspace config is editable");

    let before = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["config", "explain", "model"]),
    );
    assert!(
        !String::from_utf8_lossy(&before.stdout).contains("from-the-workspace"),
        "an untrusted workspace's own config is not in force: {}",
        String::from_utf8_lossy(&before.stdout)
    );

    let grant = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "grant"]));
    assert!(
        grant.status.success(),
        "{}",
        String::from_utf8_lossy(&grant.stderr)
    );

    let after = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["config", "explain", "model"]),
    );
    assert!(
        String::from_utf8_lossy(&after.stdout).contains("from-the-workspace"),
        "after the grant the workspace layer decides: {}",
        String::from_utf8_lossy(&after.stdout)
    );
}

/// `init` says what has to happen for the file it just wrote to take effect,
/// and the command it names is a command that works.
#[test]
fn init_names_the_command_that_activates_what_it_wrote() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["init"]));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("orrery trust grant"),
        "init tells the user how to activate the file it wrote: {said}"
    );

    let grant = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "grant"]));
    assert!(
        grant.status.success(),
        "the command init names is a command that runs: {}",
        String::from_utf8_lossy(&grant.stderr)
    );
}

/// `trust list` shows what has been answered, and `trust revoke` takes it back.
#[test]
fn list_shows_the_answers_and_revoke_takes_one_back() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let empty = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "list"]));
    assert!(
        empty.status.success(),
        "{}",
        String::from_utf8_lossy(&empty.stderr)
    );

    let _ = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "grant"]));
    let listed = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "list"]));
    let text = String::from_utf8_lossy(&listed.stdout);
    assert!(
        text.contains("trusted"),
        "the grant is listed: {text}"
    );

    let revoked = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "revoke"]));
    assert!(
        revoked.status.success(),
        "{}",
        String::from_utf8_lossy(&revoked.stderr)
    );

    let after = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "list"]));
    let text = String::from_utf8_lossy(&after.stdout);
    assert!(
        !text.contains("trusted"),
        "a revoked answer is forgotten, not stored as `false`: {text}"
    );
}

/// `--json` answers with data, because these commands compose.
#[test]
fn trust_list_speaks_json() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");
    let _ = orrery_in(home.path(), &args(&quiet(ws.path()), &["trust", "grant"]));

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["--json", "trust", "list"]),
    );
    let value: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    let entries = value["entries"]
        .as_array()
        .expect("it carries the answers");
    assert_eq!(entries.len(), 1, "{value}");
    assert_eq!(entries[0]["trusted"], serde_json::Value::Bool(true), "{value}");
}
