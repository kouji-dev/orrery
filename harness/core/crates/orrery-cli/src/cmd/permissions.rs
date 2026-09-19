//! `orrery permissions explain` - why a call would be allowed or denied.
//!
//! The answer comes from [`PolicyEngine::explain`], which is the same engine a
//! session checks against: the CLI parses a call, hands it over and prints what
//! comes back. Nothing about the verdict is computed here, because an
//! explanation a client works out for itself is an explanation that can
//! disagree with the decision.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md` task 10,
//! surfaced by `harness/docs/plans/17-cli.md` task 6.

use orrery_policy::{Explanation, PendingCall, PolicyEngine, RuleMatch, Verdict};
use orrery_proto::{Aspect, Subject};

use crate::args::{Cli, PermissionsCommand};
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Dispatch a `permissions` subcommand.
pub fn dispatch(cli: &Cli, command: &PermissionsCommand) -> ! {
    let PermissionsCommand::Explain { call } = command;
    let pending = parse_call(call).unwrap_or_else(|e| fail(Exit::Usage, e));

    let resolved = layers::resolve(cli);
    // `layers::rules` is what a run is built from too — a profile folds its
    // shorthands in on both sides — so this explanation is of the rule set that
    // will actually decide, not of a second one that resembles it.
    let engine = PolicyEngine::new(layers::rules(cli, &resolved));

    let explanation = engine.explain(&pending, &Subject::Agent);
    if layers::wants_json(cli) {
        println!("{}", json(&explanation));
    } else {
        print!("{explanation}");
    }
    Exit::Ok.exit()
}

/// The rule grammar, read as a *call*: `write(./src/main.rs)`,
/// `net(domain: docs.rs)`, `spawn(cargo: cargo test)`.
///
/// The same spelling `PendingCall::match_text` prints and an audit record
/// carries, so what a person types to ask about a call is what they read back
/// afterwards.
fn parse_call(text: &str) -> Result<PendingCall, String> {
    const ASPECTS: &str = "tool, mcp, skill, ext, mode, read, write, spawn, net, creds, ui, \
                           render, mem.read, mem.write";

    let trimmed = text.trim();
    let (word, inner) = match trimmed.split_once('(') {
        Some((word, rest)) => {
            let rest = rest.trim_end();
            let inner = rest
                .strip_suffix(')')
                .ok_or_else(|| format!("`{text}`: the closing `)` is missing"))?;
            (word.trim(), inner.trim())
        }
        None => {
            return Err(format!(
                "`{text}` is not a call: write one in the rule grammar, \
                 for example `read(./src/main.rs)` or `net(domain: docs.rs)`. \
                 The aspects are {ASPECTS}"
            ));
        }
    };

    let aspect = orrery_policy::parse::aspect_of(word)
        .ok_or_else(|| format!("`{word}` is not an aspect: expected one of {ASPECTS}"))?;
    if inner.is_empty() {
        return Err(format!(
            "`{text}` names no target: a call is one thing, not a pattern"
        ));
    }

    // `net(domain: x)` and `net(url: x)` are how the grammar writes a host.
    if aspect == Aspect::Net {
        if let Some(rest) = inner
            .strip_prefix("domain:")
            .or_else(|| inner.strip_prefix("url:"))
        {
            return Ok(PendingCall::new(aspect, rest.trim()));
        }
    }
    // `spawn(cargo: cargo test)`, `tool(git.status: …)` — an id and the tool's
    // own specifier.
    if matches!(aspect, Aspect::Tool | Aspect::Mcp | Aspect::Spawn) {
        if let Some((head, tail)) = split_specifier(inner) {
            return Ok(PendingCall::new(aspect, head).with_specifier(tail));
        }
    }
    Ok(PendingCall::new(aspect, inner))
}

/// `shell.exec: npm run build` splits; `C:\src\main.rs` does not.
fn split_specifier(term: &str) -> Option<(&str, &str)> {
    let (head, tail) = term.split_once(':')?;
    let (head, tail) = (head.trim(), tail.trim());
    if head.is_empty() || tail.is_empty() {
        return None;
    }
    head.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .then_some((head, tail))
}

fn verdict_word(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Allow => "allow",
        Verdict::Ask => "ask",
        Verdict::Deny => "deny",
        _ => "unknown",
    }
}

fn rule_json(rule: &RuleMatch) -> serde_json::Value {
    serde_json::json!({
        "id": rule.id.to_string(),
        "text": rule.text,
        "list": rule.list.keyword(),
        "layer": format!("{:?}", rule.layer).to_lowercase(),
        "file": rule.file.display().to_string(),
        "line": rule.line,
        "matched": rule.matched,
    })
}

/// The same answer as the printed one, as one JSON object on one line.
fn json(explanation: &Explanation) -> serde_json::Value {
    serde_json::json!({
        "subject": explanation.subject.to_string(),
        "request": explanation.request,
        "verdict": verdict_word(explanation.verdict),
        "reason": explanation.reason,
        "rule": explanation.rule.as_ref().map(rule_json),
        "considered": explanation.considered.iter().map(rule_json).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_is_read_in_the_rule_grammar() {
        let call = parse_call("read(./src/main.rs)").expect("a path call");
        assert_eq!(call.aspect, Aspect::Read);
        assert_eq!(call.target, "./src/main.rs");
        assert_eq!(call.match_text(), "read(./src/main.rs)");

        let net = parse_call("net(domain: docs.rs)").expect("a host call");
        assert_eq!(net.target, "docs.rs");

        let spawn = parse_call("spawn(cargo: cargo test)").expect("a command call");
        assert_eq!(spawn.target, "cargo");
        assert_eq!(spawn.specifier.as_deref(), Some("cargo test"));
    }

    #[test]
    fn a_windows_path_is_not_a_specifier() {
        let call = parse_call(r"read(C:\src\main.rs)").expect("a path call");
        assert_eq!(call.target, r"C:\src\main.rs");
        assert!(call.specifier.is_none(), "the drive letter is not an id");
    }

    #[test]
    fn what_is_not_a_call_says_what_one_looks_like() {
        let e = parse_call("rm -rf /").expect_err("not a call");
        assert!(e.contains("read(./src/main.rs)"), "{e}");
        let e = parse_call("fly(./x)").expect_err("not an aspect");
        assert!(e.contains("not an aspect"), "{e}");
    }
}
