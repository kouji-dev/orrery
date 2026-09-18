//! `orrery init` and `orrery import`.
//!
//! # One-way, explicit, and never at runtime
//!
//! `import` reads an existing Claude Code or Codex setup **once**, when a
//! person runs the command, and writes an orrery config for them to review.
//! Nothing in this crate reads a foreign config while a session runs: the
//! functions here take text and return text, they are not wired into
//! [`crate::resolve`], and `tests/import.rs::is_explicit_and_one_way` proves a
//! `.claude/settings.json` sitting in a trusted workspace changes nothing.
//!
//! # It is mapping, not reverse engineering
//!
//! The ADE already drives both tools and knows their config shapes — see
//! `ade/src-tauri/src/agents/adapters/claude.rs`, whose tests read
//! `settings.json`'s `permissions.allow` and merge into its `hooks` table, and
//! `codex.rs`, which merges into `~/.codex/config.toml`'s `mcp_servers` and
//! `hooks` tables with `toml_edit`. What is new here is only the translation
//! into our rule grammar.
//!
//! ## The permission mapping
//!
//! | Claude Code | orrery |
//! |---|---|
//! | `Bash(npm run test:*)` | `spawn(npm run test *)` |
//! | `Read(./src/**)` | `read(./src/**)` |
//! | `Edit(p)`, `Write(p)` | `write(p)` |
//! | `WebFetch(domain:docs.rs)` | `net(domain: docs.rs)` |
//! | `mcp__github__get_issue` | `mcp(github.get_issue)` |
//! | `mcp__github` | `mcp(github.*)` |
//! | a bare tool name, `Grep` | `tool(Grep)` |
//!
//! Anything that does not map — hooks, status lines, `defaultMode` — is
//! **reported as a note**, never dropped in silence and never guessed at.

use std::collections::BTreeMap;

use crate::error::ConfigError;
use crate::profile::{Profile, SHORTHANDS};

/// What an import produced: a config to review, and what it could not map.
#[derive(Clone, Debug, Default)]
pub struct Imported {
    /// Rules for the `deny` list.
    pub deny: Vec<String>,
    /// Rules for the `ask` list.
    pub ask: Vec<String>,
    /// Rules for the `allow` list.
    pub allow: Vec<String>,
    /// MCP servers, as written in the source.
    pub mcp_servers: BTreeMap<String, toml::Value>,
    /// The model, when the source names one.
    pub model: Option<String>,
    /// Other top-level values worth carrying, as `key = value`.
    pub extras: BTreeMap<String, toml::Value>,
    /// What could not be mapped, in words a person can act on.
    pub notes: Vec<String>,
}

impl Imported {
    /// The config file to write, for a person to review before using.
    #[must_use]
    pub fn to_toml(&self) -> String {
        let mut doc = toml_edit::DocumentMut::new();
        doc.decor_mut().set_prefix(
            "# Written by `orrery import`. Review it before you use it.\n\
             # Nothing reads a foreign config at runtime; this file is the import.\n\n",
        );
        if let Some(model) = &self.model {
            doc.insert("model", toml_edit::value(model.as_str()));
        }
        for (key, value) in &self.extras {
            if let Some(item) = to_item(value) {
                doc.insert(key, item);
            }
        }

        let mut permissions = toml_edit::Table::new();
        for (name, list) in [
            ("deny", &self.deny),
            ("ask", &self.ask),
            ("allow", &self.allow),
        ] {
            if list.is_empty() {
                continue;
            }
            let mut array = toml_edit::Array::new();
            for rule in list {
                array.push(rule.as_str());
            }
            permissions.insert(name, toml_edit::value(array));
        }
        if !permissions.is_empty() {
            doc.insert("permissions", toml_edit::Item::Table(permissions));
        }

        if !self.mcp_servers.is_empty() {
            let mut servers = toml_edit::Table::new();
            servers.set_implicit(true);
            for (name, value) in &self.mcp_servers {
                if let Some(item) = to_item(value) {
                    let table = match item.into_table() {
                        Ok(mut t) => {
                            t.set_implicit(false);
                            toml_edit::Item::Table(t)
                        }
                        Err(other) => other,
                    };
                    servers.insert(name, table);
                }
            }
            doc.insert("mcp_servers", toml_edit::Item::Table(servers));
        }

        let mut out = doc.to_string();
        if !self.notes.is_empty() {
            out.push_str("\n# Not imported:\n");
            for note in &self.notes {
                out.push_str(&format!("#   {note}\n"));
            }
        }
        out
    }
}

/// Import a Claude Code `settings.json`.
///
/// # Errors
///
/// When the text is not JSON.
pub fn claude_code(text: &str) -> Result<Imported, ConfigError> {
    let root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| ConfigError::Syntax {
            file: "settings.json".into(),
            line: u32::try_from(e.line()).unwrap_or(0),
            message: e.to_string(),
        })?;

    let mut out = Imported::default();
    if let Some(model) = root.get("model").and_then(serde_json::Value::as_str) {
        out.model = Some(model.to_owned());
    }

    if let Some(perms) = root.get("permissions") {
        for (name, list) in [
            ("deny", &mut out.deny),
            ("ask", &mut out.ask),
            ("allow", &mut out.allow),
        ] {
            let Some(items) = perms.get(name).and_then(serde_json::Value::as_array) else {
                continue;
            };
            for item in items.iter().filter_map(serde_json::Value::as_str) {
                match claude_rule(item) {
                    Some(rule) => list.push(rule),
                    None => out
                        .notes
                        .push(format!("`{item}` has no equivalent rule and was skipped")),
                }
            }
        }
        if perms.get("defaultMode").is_some() {
            out.notes.push(
                "`permissions.defaultMode` is Claude Code's own prompting mode; write `consent` in a profile instead".to_owned(),
            );
        }
    }

    if let Some(servers) = root.get("mcpServers").and_then(serde_json::Value::as_object) {
        for (name, value) in servers {
            if let Some(v) = json_to_toml(value) {
                out.mcp_servers.insert(name.clone(), v);
            }
        }
    }

    if root.get("hooks").is_some() {
        out.notes.push(
            "`hooks` are Claude Code's; orrery's equivalent is an interceptor, which is code rather than config".to_owned(),
        );
    }

    Ok(out)
}

/// One Claude Code permission string, as an orrery rule.
#[must_use]
pub fn claude_rule(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix("mcp__") {
        let mut parts = rest.splitn(2, "__");
        let server = parts.next()?;
        return Some(match parts.next() {
            Some(tool) => format!("mcp({server}.{tool})"),
            None => format!("mcp({server}.*)"),
        });
    }

    let Some((tool, inner)) = split_call(text) else {
        // A bare tool name: `Grep`, `Task`.
        return Some(format!("tool({text})"));
    };

    Some(match tool {
        "Bash" => format!("spawn({})", bash_pattern(inner)),
        "Read" | "Glob" | "Grep" | "NotebookRead" => format!("read({inner})"),
        "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => format!("write({inner})"),
        "WebFetch" | "WebSearch" => {
            let domain = inner.strip_prefix("domain:").unwrap_or(inner).trim();
            format!("net(domain: {domain})")
        }
        _ => format!("tool({tool}: {inner})"),
    })
}

/// `npm run test:*` is Claude Code's "commands starting with npm run test".
fn bash_pattern(inner: &str) -> String {
    match inner.rsplit_once(':') {
        Some((prefix, rest)) => format!("{} {}", prefix.trim(), rest.trim()),
        None => inner.trim().to_owned(),
    }
}

fn split_call(text: &str) -> Option<(&str, &str)> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close < open {
        return None;
    }
    Some((&text[..open], &text[open + 1..close]))
}

/// Import a Codex `config.toml`.
///
/// # Errors
///
/// When the text is not TOML.
pub fn codex(text: &str) -> Result<Imported, ConfigError> {
    let root: toml::Value = text.parse().map_err(|e: toml::de::Error| ConfigError::Syntax {
        file: "config.toml".into(),
        line: 0,
        message: e.to_string(),
    })?;

    let mut out = Imported::default();
    if let Some(model) = root.get("model").and_then(toml::Value::as_str) {
        out.model = Some(model.to_owned());
    }
    if let Some(servers) = root.get("mcp_servers").and_then(toml::Value::as_table) {
        for (name, value) in servers {
            out.mcp_servers.insert(name.clone(), value.clone());
        }
    }
    // Codex's approval policy is the nearest thing it has to our consent mode.
    match root.get("approval_policy").and_then(toml::Value::as_str) {
        Some("never") => {
            out.extras
                .insert("consent".to_owned(), toml::Value::String("never".to_owned()));
        }
        Some(other) => out.notes.push(format!(
            "`approval_policy = \"{other}\"` maps to asking, which is the default; set `consent` in a profile to change it"
        )),
        None => {}
    }
    if let Some(effort) = root
        .get("model_reasoning_effort")
        .and_then(toml::Value::as_str)
    {
        out.notes.push(format!(
            "`model_reasoning_effort = \"{effort}\"` is a provider parameter; set it on the agent that uses the model"
        ));
    }
    if root.get("sandbox_mode").is_some() {
        out.notes.push(
            "`sandbox_mode` is Codex's sandbox; orrery enforces in the broker, from the rules above".to_owned(),
        );
    }
    if root.get("hooks").is_some() {
        out.notes
            .push("`hooks` are Codex's; orrery's equivalent is an interceptor".to_owned());
    }
    Ok(out)
}

/// Write a workspace config from a chosen profile.
///
/// The shorthands are expanded into real rules on the way out, so the file a
/// person ends up reviewing says what it actually does.
#[must_use]
pub fn init(profile: &Profile) -> String {
    let mut doc = toml_edit::DocumentMut::new();
    doc.decor_mut().set_prefix(format!(
        "# Written by `orrery init` from the `{}` profile.\n\n",
        profile.name
    ));
    if !profile.name.is_empty() {
        doc.insert("profile", toml_edit::value(profile.name.as_str()));
    }
    if let Some(model) = &profile.model {
        doc.insert("model", toml_edit::value(model.as_str()));
    }
    for (key, list) in [
        ("extensions", &profile.extensions),
        ("interceptors", &profile.interceptors),
        ("subagents", &profile.subagents),
        ("skills", &profile.skills),
    ] {
        if list.is_empty() {
            continue;
        }
        let mut array = toml_edit::Array::new();
        for item in list {
            array.push(item.as_str());
        }
        doc.insert(key, toml_edit::value(array));
    }

    let mut deny = toml_edit::Array::new();
    let mut allow = toml_edit::Array::new();
    for short in &profile.permissions {
        let Some((_, _, text)) = SHORTHANDS.iter().find(|(k, _, _)| *k == short.key) else {
            continue;
        };
        if short.allowed {
            allow.push(*text);
        } else {
            deny.push(*text);
        }
    }
    let mut permissions = toml_edit::Table::new();
    if !deny.is_empty() {
        permissions.insert("deny", toml_edit::value(deny));
    }
    if !allow.is_empty() {
        permissions.insert("allow", toml_edit::value(allow));
    }
    if !permissions.is_empty() {
        doc.insert("permissions", toml_edit::Item::Table(permissions));
    }
    doc.to_string()
}

fn json_to_toml(value: &serde_json::Value) -> Option<toml::Value> {
    Some(match value {
        serde_json::Value::Null => return None,
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(toml::Value::Integer)
            .or_else(|| n.as_f64().map(toml::Value::Float))?,
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Array(items) => {
            toml::Value::Array(items.iter().filter_map(json_to_toml).collect())
        }
        serde_json::Value::Object(map) => {
            let mut table = toml::value::Table::new();
            for (k, v) in map {
                if let Some(v) = json_to_toml(v) {
                    table.insert(k.clone(), v);
                }
            }
            toml::Value::Table(table)
        }
    })
}

fn to_item(value: &toml::Value) -> Option<toml_edit::Item> {
    let text = toml::to_string(&SingleKey { value: value.clone() }).ok()?;
    let doc: toml_edit::DocumentMut = text.parse().ok()?;
    doc.get("value").cloned()
}

#[derive(serde::Serialize)]
struct SingleKey {
    value: toml::Value,
}
