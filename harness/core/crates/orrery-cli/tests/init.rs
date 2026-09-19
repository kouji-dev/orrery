//! `orrery init` and `orrery import`, the two commands that write or print a
//! configuration and start nothing.
//!
//! Neither reads a foreign config at runtime — `import` is a command a person
//! runs once — and neither needs a provider, so there is no `--provider` in
//! this file and no request can leave the machine.

mod common;

use common::{args, home_with, orrery_in, quiet};

/// Plan 17 task 9: `init` scaffolds a workspace config from a profile.
#[test]
fn init_writes_a_workspace_config() {
    let home = home_with(
        "[profile.ci]\n\
         model = \"fixture\"\n\
         consent = \"never\"\n\
         [profile.ci.permissions]\n\
         read = true\n",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["--profile", "ci", "init"]),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let written = ws.path().join(".orrery/config.toml");
    assert!(written.is_file(), "the file is where the layer expects it");
    let text = std::fs::read_to_string(&written).expect("it is readable");
    assert!(text.contains("profile = \"ci\""), "{text}");
    assert!(text.contains("model = \"fixture\""), "{text}");
    assert!(
        text.contains("read(./**)"),
        "the shorthand is expanded, so the file says what it does: {text}"
    );

    // stdout is data: the path, and nothing else.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(stdout.trim().ends_with("config.toml"), "{stdout}");
}

/// DEFECT 4: `orrery init` with no `--profile` wrote 49 bytes of nothing.
///
/// The whole file was
/// ``# Written by `orrery init` from the `` profile.`` — an empty profile name
/// interpolated into a comment — and then it said "review it before you use
/// it". This is the first command a new user runs.
#[test]
fn init_with_no_profile_writes_a_starter_config() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["init"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = std::fs::read_to_string(ws.path().join(".orrery/config.toml"))
        .expect("the file is where the layer expects it");
    assert!(
        !text.contains("the `` profile"),
        "an empty profile name is not a document: {text}"
    );
    assert!(
        text.parse::<toml::Value>().is_ok(),
        "it is a configuration, not a comment: {text}"
    );

    // Real rules, in the grammar, that a person can read and edit.
    assert!(
        text.contains("[permissions]") && text.contains("read(./**)"),
        "{text}"
    );
    // And it says what the file is and what to do next.
    assert!(
        text.contains("orrery config explain"),
        "it documents how to ask what is in force: {text}"
    );
    assert!(
        text.contains("[profile."),
        "…and how to write a profile, since there was none to scaffold: {text}"
    );

    // stdout stays data: the path, and nothing else.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
}

/// …and when profiles **do** exist and none was named, it says so, because
/// `--profile` is the flag that would have scaffolded one of them.
#[test]
fn init_with_no_profile_names_the_profiles_there_are() {
    let home = home_with(
        "[profile.ci]
model = \"fixture\"
[profile.review]
model = \"fixture\"
",
    );
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["init"]));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("ci") && err.contains("review"),
        "it names the profiles `--profile` could have used: {err}"
    );
}

/// History and configuration are not the same thing, but neither is silently
/// replaceable: a second `init` refuses rather than overwriting.
#[test]
fn init_refuses_to_overwrite() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir_all(ws.path().join(".orrery")).expect("the config directory");
    std::fs::write(ws.path().join(".orrery/config.toml"), "model = \"mine\"\n").expect("written");

    let out = orrery_in(home.path(), &args(&quiet(ws.path()), &["init"]));
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        std::fs::read_to_string(ws.path().join(".orrery/config.toml")).expect("still there"),
        "model = \"mine\"\n",
        "the file a person wrote is untouched"
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("already"), "{err}");
}

/// Plan 17 task 9: `import` maps a Claude Code settings file into our grammar.
#[test]
fn import_claude_code_prints_a_config_to_review() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir_all(ws.path().join(".claude")).expect("the claude directory");
    std::fs::write(
        ws.path().join(".claude/settings.json"),
        r#"{
  "model": "claude-sonnet-4-5",
  "permissions": {
    "allow": ["Read(./src/**)", "Bash(npm run test:*)"],
    "deny": ["WebFetch(domain:evil.example)"]
  },
  "hooks": { "PreToolUse": [] }
}"#,
    )
    .expect("the settings file");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["import", "--from", "claude-code"]),
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("read(./src/**)"), "{text}");
    assert!(text.contains("spawn(npm run test *)"), "{text}");
    assert!(text.contains("net(domain: evil.example)"), "{text}");
    assert!(
        text.parse::<toml::Value>().is_ok(),
        "stdout is the config itself, so `orrery import > config.toml` works: {text}"
    );

    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("hooks"),
        "what could not be mapped is narrated on stderr, never dropped: {err}"
    );
}

/// Nothing to import is a usage error naming where it looked, not an empty file.
#[test]
fn import_says_where_it_looked() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    let out = orrery_in(
        home.path(),
        &args(&quiet(ws.path()), &["import", "--from", "codex"]),
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(".codex"), "{err}");
}
