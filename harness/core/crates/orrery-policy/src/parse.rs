//! Reading rules: one string, or a whole TOML layer.
//!
//! # The grammar
//!
//! ```text
//! rule      := aspect [ "(" inner ")" ]
//! inner     := term { "," term }              -- commas inside {..} or [..] are literal
//! term      := "re:" REGEX
//!            | "param:" KEY "=" GLOB
//!            | "domain:" GLOB | "url:" GLOB   -- net only
//!            | ID ":" GLOB                    -- tool, mcp and spawn only
//!            | GLOB
//! ```
//!
//! `aspect` is one of `tool`, `mcp`, `skill`, `ext`, `mode`, `read`, `write`,
//! `spawn`, `net`, `creds`, `ui`, `render`, `mem.read`, `mem.write`.
//!
//! A path never splits on `:`, so a Windows `C:\src\**` is one pattern and not
//! a prefix and a glob.

use std::path::{Path, PathBuf};

use orrery_proto::{Aspect, Layer, RuleId, Subject};
use serde::Deserialize;

use crate::error::{ParseError, Warning};
use crate::rule::{Rule, RuleList, Selector, SelectorKind, Source};

/// The aspect an aspect keyword names.
#[must_use]
pub fn aspect_of(word: &str) -> Option<Aspect> {
    Some(match word {
        "tool" => Aspect::Tool,
        "mcp" => Aspect::Mcp,
        "skill" => Aspect::Skill,
        "ext" => Aspect::Ext,
        "mode" => Aspect::Mode,
        "read" => Aspect::Read,
        "write" => Aspect::Write,
        "spawn" => Aspect::Spawn,
        "net" => Aspect::Net,
        "creds" => Aspect::Creds,
        "ui" => Aspect::Ui,
        "render" => Aspect::Render,
        "mem.read" => Aspect::MemRead,
        "mem.write" => Aspect::MemWrite,
        _ => return None,
    })
}

/// The keyword for an aspect, which is what an explanation prints.
#[must_use]
pub fn aspect_word(aspect: Aspect) -> &'static str {
    match aspect {
        Aspect::Tool => "tool",
        Aspect::Mcp => "mcp",
        Aspect::Skill => "skill",
        Aspect::Ext => "ext",
        Aspect::Mode => "mode",
        Aspect::Read => "read",
        Aspect::Write => "write",
        Aspect::Spawn => "spawn",
        Aspect::Net => "net",
        Aspect::Creds => "creds",
        Aspect::Ui => "ui",
        Aspect::Render => "render",
        Aspect::MemRead => "mem.read",
        Aspect::MemWrite => "mem.write",
        _ => "unknown",
    }
}

/// Parse one rule string into its aspect and selector.
///
/// # Errors
///
/// When the aspect is not one of the known keywords, or the parentheses do not
/// balance, or a term is malformed.
pub fn rule(text: &str) -> Result<(Aspect, Selector), ParseError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(ParseError::bare("a rule cannot be empty", text));
    }
    let (word, inner) = match trimmed.split_once('(') {
        None => (trimmed, None),
        Some((word, rest)) => {
            let rest = rest.trim_end();
            let Some(inner) = rest.strip_suffix(')') else {
                return Err(ParseError::bare("the closing `)` is missing", text));
            };
            (word.trim(), Some(inner))
        }
    };

    let Some(aspect) = aspect_of(word.trim()) else {
        return Err(ParseError::bare(
            format!("`{word}` is not an aspect: expected one of tool, mcp, skill, ext, mode, read, write, spawn, net, creds, ui, render, mem.read, mem.write"),
            text,
        ));
    };

    let Some(inner) = inner else {
        return Ok((aspect, Selector::any()));
    };
    let selector = terms(aspect, inner, text)?;
    Ok((aspect, selector))
}

/// The selector half of [`rule`], for a caller that already knows the aspect.
#[must_use]
pub fn selector_of(aspect: Aspect, text: &str) -> Option<Selector> {
    let (parsed, selector) = rule(text).ok()?;
    (parsed == aspect).then_some(selector)
}

fn terms(aspect: Aspect, inner: &str, whole: &str) -> Result<Selector, ParseError> {
    let mut selector = Selector::any();
    for term in split_top_level(inner) {
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        if let Some(pattern) = term.strip_prefix("re:") {
            if pattern.trim().is_empty() {
                return Err(ParseError::bare("`re:` needs a pattern", whole));
            }
            selector.regex = Some(pattern.trim().to_owned());
            continue;
        }
        if let Some(rest) = term.strip_prefix("param:") {
            let Some((key, value)) = rest.split_once('=') else {
                return Err(ParseError::bare(
                    "a `param:` term is written `param:key=glob`",
                    whole,
                ));
            };
            if key.trim().is_empty() {
                return Err(ParseError::bare("a `param:` term needs a key", whole));
            }
            selector.params.push((key.trim().to_owned(), value.trim().to_owned()));
            continue;
        }
        if aspect == Aspect::Net {
            if let Some(rest) = term.strip_prefix("domain:").or_else(|| term.strip_prefix("url:")) {
                selector.primary = Some(rest.trim().to_owned());
                continue;
            }
        }
        if matches!(aspect, Aspect::Tool | Aspect::Mcp | Aspect::Spawn) {
            if let Some((head, tail)) = split_id_specifier(term) {
                selector.primary = Some(head.to_owned());
                selector.specifier = Some(tail.to_owned());
                continue;
            }
        }
        if selector.primary.is_some() {
            return Err(ParseError::bare(
                "a rule has one main pattern; write a second rule",
                whole,
            ));
        }
        selector.primary = Some(term.to_owned());
    }
    Ok(selector)
}

/// `shell.exec: npm run *` splits; `C:\src` and `*.corp.internal` do not.
fn split_id_specifier(term: &str) -> Option<(&str, &str)> {
    let (head, tail) = term.split_once(':')?;
    let head = head.trim();
    let tail = tail.trim();
    if head.is_empty() || tail.is_empty() {
        return None;
    }
    let idish = head
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '*'));
    idish.then_some((head, tail))
}

/// Split on commas that are not inside `{}` or `[]`, so a `{a,b}` glob survives.
fn split_top_level(inner: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in inner.char_indices() {
        match c {
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ',' if depth <= 0 => {
                out.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&inner[start..]);
    out
}

/// The `[permissions]` table of one config file.
#[derive(Debug, Default, Deserialize)]
pub struct ConfigDoc {
    /// The permissions block, if there is one.
    #[serde(default)]
    pub permissions: PermissionsDoc,
}

/// One `[permissions]` table: the agent's own lists, plus a table per subject.
#[derive(Debug, Default, Deserialize)]
pub struct PermissionsDoc {
    /// The agent's own allow list.
    #[serde(default)]
    pub allow: Vec<String>,
    /// The agent's own ask list.
    #[serde(default)]
    pub ask: Vec<String>,
    /// The agent's own deny list.
    #[serde(default)]
    pub deny: Vec<String>,
    /// One table per subject: `"ext:buildgraph"`, `"agent:critic"`.
    #[serde(flatten)]
    pub subjects: indexmap::IndexMap<String, RuleLists>,
}

/// The three lists for one subject.
#[derive(Debug, Default, Deserialize)]
pub struct RuleLists {
    /// Allowed without asking.
    #[serde(default)]
    pub allow: Vec<String>,
    /// The user is asked.
    #[serde(default)]
    pub ask: Vec<String>,
    /// Refused.
    #[serde(default)]
    pub deny: Vec<String>,
}

/// Every rule one config file contributes, with the warnings it raised.
#[derive(Debug)]
pub struct LoadedLayer {
    /// The layer.
    pub layer: Layer,
    /// Its rules, in the order they were written.
    pub rules: Vec<Rule>,
    /// What a person should be told about at load.
    pub warnings: Vec<Warning>,
}

/// Read one TOML config into a layer's rules.
///
/// `regex_allowed` is the `re:` escape hatch. Off by default: a `re:` rule under
/// it is a load **error**, not a silent ignore, because a rule that does not
/// apply is worse than one that does not parse. On, it parses and raises a
/// [`Warning`].
///
/// # Errors
///
/// When the TOML is malformed, a subject key is not a [`Subject`], a rule does
/// not parse, or a `re:` rule appears while the escape hatch is off. Every error
/// names the file and the line.
pub fn load_toml(
    text: &str,
    file: impl AsRef<Path>,
    layer: Layer,
    regex_allowed: bool,
) -> Result<LoadedLayer, ParseError> {
    let file: PathBuf = file.as_ref().to_path_buf();
    let doc: ConfigDoc = toml::from_str(text)
        .map_err(|e| ParseError::at(&file, line_of_toml(text, &e), e.to_string(), ""))?;

    let mut out = LoadedLayer {
        layer,
        rules: Vec::new(),
        warnings: Vec::new(),
    };

    let own = RuleLists {
        allow: doc.permissions.allow,
        ask: doc.permissions.ask,
        deny: doc.permissions.deny,
    };
    push_subject(&mut out, text, &file, layer, Subject::Agent, &own, regex_allowed)?;

    for (key, lists) in &doc.permissions.subjects {
        let subject: Subject = key.parse().map_err(|_| {
            ParseError::at(
                &file,
                line_of(text, key),
                format!("`{key}` is not a subject: expected `agent`, `ext:<id>` or `agent:<name>`"),
                key,
            )
        })?;
        push_subject(&mut out, text, &file, layer, subject, lists, regex_allowed)?;
    }

    Ok(out)
}

fn push_subject(
    out: &mut LoadedLayer,
    text: &str,
    file: &Path,
    layer: Layer,
    subject: Subject,
    lists: &RuleLists,
    regex_allowed: bool,
) -> Result<(), ParseError> {
    for (list, written) in [
        (RuleList::Deny, &lists.deny),
        (RuleList::Ask, &lists.ask),
        (RuleList::Allow, &lists.allow),
    ] {
        for raw in written {
            let line = line_of(text, raw);
            let (aspect, selector) = rule(raw)
                .map_err(|e| ParseError::at(file, line, e.message().to_owned(), raw))?;
            if selector.regex.is_some() {
                if !regex_allowed {
                    return Err(ParseError::at(
                        file,
                        line,
                        "`re:` rules are disabled; turn the escape hatch on deliberately, or write a glob",
                        raw,
                    ));
                }
                out.warnings.push(Warning {
                    file: file.to_path_buf(),
                    line,
                    rule: raw.clone(),
                    message: "`re:` is an escape hatch: a permission rule nobody can read at a glance is a governance problem".to_owned(),
                });
            }
            out.rules.push(Rule {
                id: RuleId::new(),
                list,
                layer,
                subject: subject.clone(),
                aspect,
                kind: SelectorKind::of(aspect),
                selector,
                source: Source::new(file, line),
                text: raw.clone(),
            });
        }
    }
    Ok(())
}

/// The 1-based line a literal first appears on, or 0 when it does not.
fn line_of(text: &str, needle: &str) -> u32 {
    if needle.is_empty() {
        return 0;
    }
    if let Some(i) = text.lines().position(|l| l.contains(needle)) {
        return u32::try_from(i + 1).unwrap_or(0);
    }
    // The rule as parsed is unescaped; the file still holds `\\` where the rule
    // holds `\`. Compare with the backslashes taken out of both sides rather
    // than trying to re-escape.
    let bare = needle.replace('\\', "");
    text.lines()
        .position(|l| l.replace('\\', "").contains(&bare))
        .map_or(0, |i| u32::try_from(i + 1).unwrap_or(0))
}

fn line_of_toml(text: &str, error: &toml::de::Error) -> u32 {
    error.span().map_or(0, |span| {
        u32::try_from(text[..span.start.min(text.len())].lines().count().max(1)).unwrap_or(0)
    })
}
