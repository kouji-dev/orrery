//! Task 3 · the matcher, and the normalisation that has to happen first.

mod common;

use common::{engine, wide_scope, Workspace};
use orrery_policy::{PendingCall, Verdict};
use orrery_proto::Subject;
use proptest::prelude::*;

fn verdict(toml: &str, ws: &Workspace, call: PendingCall) -> Verdict {
    let (engine, _) = engine(ws, toml);
    engine.check(&call, &Subject::Agent, &wide_scope()).verdict()
}

/// An allow can never carve an exception out of a deny.
#[test]
fn deny_beats_allow() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
deny  = ["write(./**)"]
allow = ["write(./src/**)"]
"#;
    let target = ws.path("src/main.rs").display().to_string();
    assert_eq!(
        verdict(toml, &ws, PendingCall::write(&target)),
        Verdict::Deny,
        "the more specific allow must not win"
    );
}

/// Within one list, the first rule that matches decides — **specificity is
/// deliberately irrelevant**, so the broad rule written first wins over the
/// narrow one written after it.
#[test]
fn first_match_wins_within_a_list() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
allow = ["tool(git.*)", "tool(git.push)"]
"#;
    let (engine, _) = engine(&ws, toml);
    let explained = engine.explain(&PendingCall::tool("git.push"), &Subject::Agent);
    let rule = explained.rule.expect("a rule matched");
    assert_eq!(rule.text, "tool(git.*)", "the first match wins, not the tightest");

    // And the lists run deny, then ask, then allow, whatever order they were
    // written in: an ask on the same name beats an allow on it.
    let toml = r#"
[permissions]
allow = ["tool(git.*)"]
ask   = ["tool(git.push)"]
"#;
    assert_eq!(
        verdict(toml, &ws, PendingCall::tool("git.push")),
        Verdict::Ask
    );
    assert_eq!(
        verdict(toml, &ws, PendingCall::tool("git.status")),
        Verdict::Allow
    );
}

/// The test that stops the rule being defeated by a link.
#[test]
fn paths_normalise() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
deny  = ["read(./secrets/**)"]
allow = ["read(./**)"]
"#;

    let direct = ws.path("secrets/key.txt").display().to_string();
    assert_eq!(verdict(toml, &ws, PendingCall::read(&direct)), Verdict::Deny);

    // A `..` traversal that lands back inside the denied directory.
    let traversal = ws.path("src/../secrets/key.txt").display().to_string();
    assert_eq!(
        verdict(toml, &ws, PendingCall::read(&traversal)),
        Verdict::Deny,
        "`..` must collapse before the pattern runs"
    );

    // A relative form, which is how a model usually writes it.
    assert_eq!(
        verdict(toml, &ws, PendingCall::read("./secrets/key.txt")),
        Verdict::Deny
    );
    assert_eq!(
        verdict(toml, &ws, PendingCall::read("secrets/key.txt")),
        Verdict::Deny
    );

    // A UNC / verbatim form of the same file. `dunce` reduces it.
    #[cfg(windows)]
    {
        let unc = format!(r"\\?\{}", ws.path("secrets/key.txt").display());
        assert_eq!(verdict(toml, &ws, PendingCall::read(&unc)), Verdict::Deny);
    }

    // And something genuinely elsewhere still passes the deny and hits allow.
    let ok = ws.path("src/main.rs").display().to_string();
    assert_eq!(verdict(toml, &ws, PendingCall::read(&ok)), Verdict::Allow);
}

/// A symlink out of the denied directory resolves before it is matched.
///
/// Creating one needs a privilege Windows does not hand out by default, so the
/// test skips rather than lies where it cannot make the link.
#[test]
fn a_symlink_resolves_before_it_matches() {
    let ws = Workspace::new();
    let link = ws.path("src/shortcut.txt");
    let target = ws.path("secrets/key.txt");

    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link).is_ok();
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&target, &link).is_ok();
    #[cfg(not(any(unix, windows)))]
    let made = false;

    if !made {
        eprintln!("skipped: this platform would not create a symlink");
        return;
    }

    let toml = r#"
[permissions]
deny  = ["read(./secrets/**)"]
allow = ["read(./**)"]
"#;
    assert_eq!(
        verdict(toml, &ws, PendingCall::read(link.display().to_string())),
        Verdict::Deny,
        "a link out of the workspace rule must not defeat it"
    );
}

/// Windows and macOS fold case; Linux does not, and the rule must agree with
/// whichever filesystem it is running on.
#[test]
fn case_folding_where_the_fs_is_insensitive() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
deny  = ["read(./Secrets/**)"]
allow = ["read(./**)"]
"#;
    let shouted = ws.path("SECRETS/key.txt").display().to_string();
    let expected = if cfg!(any(windows, target_os = "macos")) {
        Verdict::Deny
    } else {
        Verdict::Allow
    };
    assert_eq!(verdict(toml, &ws, PendingCall::read(&shouted)), expected);
}

/// A named-input match, for the calls whose target is not the whole story.
#[test]
fn a_param_term_matches_one_named_input() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
deny  = ["tool(jira.create, param:project=SECRET)"]
allow = ["tool(jira.*)"]
"#;
    assert_eq!(
        verdict(
            toml,
            &ws,
            PendingCall::tool("jira.create").with_param("project", "SECRET")
        ),
        Verdict::Deny
    );
    assert_eq!(
        verdict(
            toml,
            &ws,
            PendingCall::tool("jira.create").with_param("project", "ORR")
        ),
        Verdict::Allow
    );
}

/// A tool's own specifier, which is the second half of `tool(id: spec)`.
#[test]
fn a_specifier_matches_the_tools_own_argument() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
deny  = ["tool(shell.exec: rm *)"]
allow = ["tool(shell.exec: npm run *)"]
"#;
    let call = |s: &str| PendingCall::tool("shell.exec").with_specifier(s);
    assert_eq!(verdict(toml, &ws, call("rm -rf /")), Verdict::Deny);
    assert_eq!(verdict(toml, &ws, call("npm run build")), Verdict::Allow);
    // Nothing matched at all, so the default refuses.
    assert_eq!(verdict(toml, &ws, call("cargo build")), Verdict::Deny);
}

proptest! {
    /// Arbitrary selectors against arbitrary inputs: it may refuse, it may
    /// allow, it may fail to parse. It may not panic.
    #[test]
    fn never_panics(
        aspect in prop::sample::select(vec![
            "tool", "mcp", "skill", "ext", "mode", "read", "write", "spawn",
            "net", "creds", "mem.read", "mem.write", "nonsense", "",
        ]),
        pattern in "[a-zA-Z0-9_*./:{},\\\\-]{0,24}",
        target in "[a-zA-Z0-9_*./:\\\\-]{0,24}",
    ) {
        let text = if pattern.is_empty() {
            aspect.to_string()
        } else {
            format!("{aspect}({pattern})")
        };
        let ws = Workspace::new();
        let toml = format!(
            "[permissions]\nallow = [{}]\n",
            serde_json::to_string(&text).unwrap()
        );
        // Either it does not load — which is a value — or it decides. Neither
        // of those is a panic, and that is the whole property.
        if let Ok(builder) = orrery_policy::PolicyBuilder::new(ws.root())
            .layer_toml(&toml, "p.toml", orrery_proto::Layer::Project, false)
        {
            if let Ok(rules) = builder.build() {
                let engine = orrery_policy::PolicyEngine::new(rules);
                for asp in [
                    orrery_proto::Aspect::Tool,
                    orrery_proto::Aspect::Read,
                    orrery_proto::Aspect::Net,
                ] {
                    let call = PendingCall::new(asp, target.clone());
                    let _ = engine.check(&call, &Subject::Agent, &wide_scope());
                }
            }
        }
    }
}
