//! Every exit code in `17-cli.md`'s table, reached by a real command.
//!
//! A CI script's response to 3, 4 and 5 is different, so each one is asserted
//! against a run rather than against a `match` arm. The two that no
//! non-interactive command can produce in this build — 1 from a grader, and 5
//! from a provider that needs a credential — are covered where they are decided,
//! in `exit::tests`; 1 is reached here through `ext test` instead.

mod common;

use common::{args, base, orrery, stream};

/// 0 — the turn completed.
#[test]
fn success_is_zero() {
    let dir = common::workspace();
    let base = base(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);
    let out = orrery(&args(&base, &["run", "-p", "hello"]));
    assert_eq!(out.status.code(), Some(0));
}

/// 2 — usage. An unknown flag, and a command that needs a model with none named.
#[test]
fn usage_is_two() {
    let out = orrery(&["--no-such-flag".to_owned()]);
    assert_eq!(out.status.code(), Some(2), "an unknown flag");

    let dir = common::workspace();
    let out = orrery(&args(&base(dir.path(), &[]), &["run", "-p", "hello"]));
    assert_eq!(out.status.code(), Some(2), "no `--provider`");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("--provider"),
        "and it says what to pass"
    );
}

/// 3 — a ceiling. The last stream repeats, so a fixture that only ever asks for
/// a tool never stops asking, and the turn budget is what ends it.
#[test]
fn a_ceiling_is_three() {
    let dir = common::workspace();
    let base = base(dir.path(), &["tool-call.jsonl"]);
    let out = orrery(&args(&base, &["run", "-p", "loop forever"]));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(3), "stderr was: {stderr}");
    assert!(
        stderr.contains("ceiling"),
        "and it says what stopped it: {stderr}"
    );
}

/// 4 — denied by policy, with no fallback. The default rules allow the
/// workspace and nothing else, so a read outside it is refused by the broker;
/// the model then answers in prose and the turn *completes* with a refusal as
/// its last act.
#[test]
fn a_denial_with_no_fallback_is_four() {
    let dir = common::workspace();
    // Forward slashes on both platforms: the rules match on a resolved path,
    // and a backslash inside a JSON fragment inside a JSON line is three
    // levels of escaping for nothing.
    let outside = if cfg!(windows) {
        "C:/Windows/System32/drivers/etc/hosts"
    } else {
        "/etc/hosts"
    };
    // Hand-written rather than committed: the path has to differ by platform,
    // and a stream is `ModelEvent`s, so writing one here is writing the type.
    let refused = dir.path().join("refused.jsonl");
    std::fs::write(
        &refused,
        format!(
            concat!(
                "{{\"t\":\"started\",\"id\":\"msg_denied\"}}\n",
                "{{\"t\":\"text-delta\",\"text\":\"Let me look at the hosts file.\"}}\n",
                "{{\"t\":\"tool-use-start\",\"call\":\"0192f3a0-0000-7000-8000-0000000000d1\",\"name\":\"builtin.read\"}}\n",
                "{{\"t\":\"tool-use-delta\",\"call\":\"0192f3a0-0000-7000-8000-0000000000d1\",\"json_fragment\":\"{{\\\"path\\\":\\\"{outside}\\\"}}\"}}\n",
                "{{\"t\":\"tool-use-end\",\"call\":\"0192f3a0-0000-7000-8000-0000000000d1\"}}\n",
                "{{\"t\":\"done\",\"stop\":\"tool-use\"}}\n",
            ),
            outside = outside
        ),
    )
    .expect("the fixture is writable");

    let mut base = base(dir.path(), &[]);
    base.push("--provider".to_owned());
    base.push(format!("fixture:{}", refused.display()));
    base.push("--provider".to_owned());
    base.push(format!("fixture:{}", stream("text-turn.jsonl").display()));

    let out = orrery(&args(&base, &["run", "-p", "read the hosts file"]));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(4),
        "a turn whose last call was refused; stderr was: {stderr}"
    );
}

/// 6 — the harness broke. A fixture that is not there is discovered at build,
/// naming the file, rather than in the middle of a turn.
#[test]
fn a_broken_harness_is_six() {
    let dir = common::workspace();
    let missing = dir.path().join("no-such-stream.jsonl");
    let base = vec![
        "--workspace".to_owned(),
        dir.path().display().to_string(),
        "--state-dir".to_owned(),
        dir.path().join(".orrery").display().to_string(),
        "--provider".to_owned(),
        format!("fixture:{}", missing.display()),
    ];
    let out = orrery(&args(&base, &["run", "-p", "hello"]));
    assert_eq!(out.status.code(), Some(6));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no-such-stream"),
        "and it names the file"
    );
}
