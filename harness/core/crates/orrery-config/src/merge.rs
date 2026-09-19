//! The merge, over `toml_edit`, with every span kept.
//!
//! It cannot use `serde`'s deserialize-into-one-struct: the moment the layers
//! collapse into a struct the spans are gone and `config explain` has nothing
//! to say. So the merge walks each layer's parsed document, records every leaf
//! with its layer, file and line, and deserialises **once, at the end**.
//!
//! Two directions, deliberately:
//!
//! - **Names** resolve closest-layer-first. A project value shadows a user one.
//! - **Deny** is a union: every layer's denies stay in force, and a managed
//!   deny cannot be relaxed by a closer `allow`.

use std::collections::BTreeSet;
use std::path::Path;

use orrery_policy::{PolicyBuilder, ResolvedRules, Rule, RuleList};
use orrery_proto::{Aspect, Layer, Subject};

use crate::error::ConfigError;
use crate::layer::{LayerFile, line_at};
use crate::provenance::{Fold, Origin, Provenanced, Slot};

/// A closer layer trying to allow something a managed layer denies.
///
/// It does not work — the deny list is walked first, from every layer — but it
/// is a thing somebody did on purpose, so it is recorded rather than dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relaxation {
    /// The rule text that was written.
    pub rule: String,
    /// Which list it was written in: `allow` or `ask`.
    pub list: &'static str,
    /// Where the attempt was written.
    pub attempted: Origin,
    /// Where the managed deny is written.
    pub managed: Origin,
    /// The managed rule text, which may be broader than the attempt.
    pub managed_rule: String,
}

impl std::fmt::Display for Relaxation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: `{}` in the {} list cannot relax `{}` denied at {}",
            self.attempted, self.rule, self.list, self.managed_rule, self.managed
        )
    }
}

/// A local layer claiming trust for itself. Never merged, always reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoredClaim {
    /// The dotted key that was written.
    pub key: String,
    /// Where it was written.
    pub origin: Origin,
}

impl std::fmt::Display for IgnoredClaim {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: `{}` is ignored — a project does not get a vote on whether it is trusted",
            self.origin, self.key
        )
    }
}

/// What the merge produced.
#[derive(Clone, Debug, Default)]
pub struct MergeReport {
    /// Every value, with its origin and what it shadowed.
    pub values: Provenanced,
    /// Attempts to relax a managed deny. Logged, never honoured.
    pub relaxations: Vec<Relaxation>,
    /// Trust claims a workspace or project layer made. Dropped, then reported.
    pub ignored_trust_claims: Vec<IgnoredClaim>,
}

/// Merge layer files into one provenanced set of values.
///
/// The files are taken in the order given, farthest layer first, so the last
/// one to set a key is the one in force. Nested project files carry a `depth`
/// and are expected to arrive farthest-first too.
///
/// # Errors
///
/// When a file does not parse as TOML. The error names the file and the line.
pub fn merge(files: &[LayerFile]) -> Result<MergeReport, ConfigError> {
    let mut values = Provenanced::new();
    let mut ignored_trust_claims = Vec::new();
    for file in files {
        let local = matches!(file.layer, Layer::Workspace | Layer::Project);
        let doc = file.document()?;
        let mut path = Vec::new();
        walk(
            doc.as_table(),
            &mut path,
            &mut |leaf_path: &[String], value: toml::Value, offset: usize| {
                let line = line_at(&file.text, offset);
                let key = crate::provenance::join(leaf_path);
                // A local layer never gets a vote on its own trust: the claim
                // is dropped here, before anything can read it, and reported.
                if local && crate::trust::is_trust_claim(&key) {
                    ignored_trust_claims.push(IgnoredClaim {
                        key,
                        origin: Origin::new(file.layer, &file.path, line),
                    });
                    return;
                }
                let fold = fold_of(leaf_path);
                values.record(
                    leaf_path.to_vec(),
                    fold,
                    Slot {
                        value,
                        origin: Origin::new(file.layer, &file.path, line),
                    },
                );
            },
        );
    }
    let relaxations = relaxations(&values);
    Ok(MergeReport {
        values,
        relaxations,
        ignored_trust_claims,
    })
}

/// The rules those same layers make, compiled against a workspace root.
///
/// Rule resolution is `orrery-policy`'s and is not re-implemented here: the
/// merge hands it every layer, and the builder is what makes deny a union and a
/// managed deny final.
///
/// # Errors
///
/// When a rule does not parse or a pattern does not compile.
pub fn policy(root: impl AsRef<Path>, files: &[LayerFile]) -> Result<ResolvedRules, ConfigError> {
    Ok(builder(root.as_ref(), files)?.build()?)
}

/// What [`DEFAULT_PERMISSIONS`]'s rules name as the file they were written in.
///
/// Not a path. `permissions explain` promises the rule, the layer, the file and
/// the line, and for as long as the default was attributed to
/// `<workspace>/orrery.toml` it named a file that is not there — sending anyone
/// who went to look for the rule to a file that does not exist. The line number
/// stays true: it is the line within this text.
pub const DEFAULT_PERMISSIONS_SOURCE: &str = "<built-in default>";

/// The permission **floor**: what the agent may do that no layer has spoken
/// about.
///
/// Read, write and spawn **inside the workspace**, and every tool that is
/// registered. Deliberately not "everything": a path outside the root does not
/// match `./**`, so the first thing a misbehaving tool tries is the first thing
/// that is refused.
///
/// It lives here, beside the layering, rather than in the harness, because the
/// default has to be the *same* default on both sides: the rules a run
/// dispatches through and the rules `permissions explain` prints are one
/// answer, and a default only one of them knew about is how they came to
/// disagree.
///
/// # Why a floor and not a fallback — decided 2026-09-19
///
/// For several rounds this stood in only when **no layer declared any
/// permission rule at all**. Declaring one replaced the whole set, so a user
/// config whose entire content was
///
/// ```toml
/// [permissions]
/// allow = ["read(./**)"]
/// ```
///
/// left nothing matching `tool(builtin.read)`, the offered tool list came out
/// **empty**, and the run exited 4 — while `permissions explain
/// 'read(./Cargo.toml)'` answered Allow. The file `orrery init` itself writes
/// did exactly this. The user was told their configuration was fine while the
/// model silently had no tools.
///
/// The decision is the one the rest of the system already states — *a rule file
/// narrows, it never widens* — applied **per aspect**:
///
/// - A layer's `allow` or `ask` for an aspect says what is *permitted* for that
///   aspect, so it **replaces** that aspect's floor entirely. Naming one tool
///   offers one tool.
/// - A layer's `deny` says what is *refused*, not what is permitted, so it
///   narrows the floor and leaves the rest of it standing. Otherwise a managed
///   `deny = ["tool(shell.*)"]` would be a workspace with no tools at all.
/// - An aspect no layer allows or asks about keeps its floor.
///
/// "Deny them all" stays expressible: `deny = ["tool(*)"]` is walked before any
/// allow from any layer, floor included.
///
/// See `harness/docs/plans/10-config-layers.md` and
/// `harness/docs/plans/07-policy-broker-audit.md`.
pub const DEFAULT_PERMISSIONS: &str = "[permissions]
allow = [\"tool(*)\", \"read(./**)\", \"write(./**)\", \"spawn(*)\"]
";

/// A builder loaded with every layer's rules **and the floor under them**.
///
/// The one place rules are compiled from layers. [`policy`] is this plus
/// `build()`, and [`crate::profile::rules`] is this plus the profile's
/// shorthands — so there is no way to assemble a rule set that is missing the
/// floor, which is how the defect above kept coming back: a second construction
/// path knew a different default.
///
/// Separate from [`policy`] because a caller that folds more rules in — a
/// profile's shorthands — needs the layers *before* they are compiled.
///
/// # Errors
///
/// When a rule does not parse or a pattern does not compile.
pub fn builder(root: impl AsRef<Path>, files: &[LayerFile]) -> Result<PolicyBuilder, ConfigError> {
    let mut builder = PolicyBuilder::new(root.as_ref());
    for file in files {
        builder = builder.layer_toml(&file.text, &file.path, file.layer, false)?;
    }
    if let Some(text) = floor_for(files)? {
        builder = builder.layer_toml(&text, DEFAULT_PERMISSIONS_SOURCE, Layer::Project, true)?;
    }
    Ok(builder)
}

/// The part of [`DEFAULT_PERMISSIONS`] these layers have **not** spoken for, as
/// a config text of its own, or `None` when they have spoken for all of it.
///
/// Written out as TOML rather than built as [`orrery_policy::Rule`] values on
/// purpose: a rule built in code carries no source, and `permissions explain`
/// would then attribute the floor to an empty file on line 0. Parsed from text,
/// every floor rule names [`DEFAULT_PERMISSIONS_SOURCE`] and its own line
/// within it.
///
/// # Errors
///
/// When a layer's rules do not parse. The floor itself is this crate's own
/// text; a failure to parse it is a bug here.
fn floor_for(files: &[LayerFile]) -> Result<Option<String>, ConfigError> {
    let permitted = permitted_aspects(files)?;
    let mut kept = Vec::new();
    for rule in parse_default()? {
        if !permitted.contains(&rule.aspect) {
            kept.push(format!("\"{}\"", rule.text));
        }
    }
    if kept.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "[permissions]\nallow = [{}]\n",
        kept.join(", ")
    )))
}

/// Every aspect some layer has said is **permitted** — written in an `allow` or
/// an `ask` list, for the agent itself.
///
/// `deny` is deliberately not here: see [`DEFAULT_PERMISSIONS`]. Subjects other
/// than the agent are deliberately not here either — a rule file written about
/// one sub-agent does not decide what the agent it was spawned from may do.
fn permitted_aspects(files: &[LayerFile]) -> Result<BTreeSet<Aspect>, ConfigError> {
    let mut out = BTreeSet::new();
    for file in files {
        let loaded = orrery_policy::parse::load_toml(&file.text, &file.path, file.layer, true)
            .map_err(orrery_policy::PolicyError::from)?;
        for rule in loaded.rules {
            if rule.subject == Subject::Agent && rule.list != RuleList::Deny {
                out.insert(rule.aspect);
            }
        }
    }
    Ok(out)
}

/// [`DEFAULT_PERMISSIONS`], parsed, so the floor's aspects come from the text
/// rather than from a second list that could drift from it.
///
/// # Errors
///
/// Only if this crate's own constant stops parsing, which is a bug here.
fn parse_default() -> Result<Vec<Rule>, ConfigError> {
    let loaded = orrery_policy::parse::load_toml(
        DEFAULT_PERMISSIONS,
        DEFAULT_PERMISSIONS_SOURCE,
        Layer::Project,
        true,
    )
    .map_err(orrery_policy::PolicyError::from)?;
    Ok(loaded.rules)
}

/// How a key folds across layers.
///
/// Only `deny` unions. Allow and ask resolve by layer precedence, which is what
/// lets a user narrow their own allow list without a project widening it.
#[must_use]
pub fn fold_of(path: &[String]) -> Fold {
    let is_permissions = path.first().is_some_and(|s| s == "permissions");
    let is_deny = path.last().is_some_and(|s| s == "deny");
    if is_permissions && is_deny {
        Fold::Union
    } else {
        Fold::Override
    }
}

/// Every closer-layer allow or ask that names exactly what a managed layer
/// denies.
fn relaxations(values: &Provenanced) -> Vec<Relaxation> {
    let mut managed: Vec<(String, String, &Origin)> = Vec::new();
    for key in deny_keys(values) {
        for (text, origin) in values.union_strings(&key) {
            if origin.layer == Layer::Managed {
                managed.push((normalise(&text), text, origin));
            }
        }
    }
    if managed.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    for key in values.keys().map(str::to_owned).collect::<Vec<_>>() {
        let list = match key.rsplit('.').next() {
            Some("allow") => "allow",
            Some("ask") => "ask",
            _ => continue,
        };
        if !key.starts_with("permissions") {
            continue;
        }
        for slot in values.all(&key) {
            if slot.origin.layer == Layer::Managed {
                continue;
            }
            let written = match &slot.value {
                toml::Value::Array(items) => items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect::<Vec<_>>(),
                toml::Value::String(s) => vec![s.clone()],
                _ => continue,
            };
            for text in written {
                let norm = normalise(&text);
                if let Some((_, managed_rule, managed_origin)) =
                    managed.iter().find(|(m, _, _)| *m == norm)
                {
                    out.push(Relaxation {
                        rule: text,
                        list,
                        attempted: slot.origin.clone(),
                        managed: (*managed_origin).clone(),
                        managed_rule: managed_rule.clone(),
                    });
                }
            }
        }
    }
    out
}

fn deny_keys(values: &Provenanced) -> Vec<String> {
    values
        .keys()
        .filter(|k| k.starts_with("permissions") && k.ends_with("deny"))
        .map(str::to_owned)
        .collect()
}

/// Rule text with its incidental whitespace taken out, so `net(domain: *)` and
/// `net(domain:*)` are the same rule.
fn normalise(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// Walk a parsed table, calling `sink` for every leaf with its dotted path and
/// the 1-based line of the key that names it.
fn walk<F>(table: &toml_edit::Table, path: &mut Vec<String>, sink: &mut F)
where
    F: FnMut(&[String], toml::Value, usize),
{
    for (name, item) in table.iter() {
        let line = table
            .key(name)
            .and_then(|k| k.span())
            .map_or(0, |s| s.start);
        path.push(name.to_owned());
        walk_item(item, path, line, sink);
        path.pop();
    }
}

fn walk_item<F>(item: &toml_edit::Item, path: &mut Vec<String>, key_start: usize, sink: &mut F)
where
    F: FnMut(&[String], toml::Value, usize),
{
    match item {
        toml_edit::Item::Table(t) => walk(t, path, sink),
        toml_edit::Item::ArrayOfTables(arr) => {
            for (i, t) in arr.iter().enumerate() {
                path.push(i.to_string());
                walk(t, path, sink);
                path.pop();
            }
        }
        toml_edit::Item::Value(toml_edit::Value::InlineTable(t)) => {
            for (name, value) in t.iter() {
                let start = t.key(name).and_then(|k| k.span()).map_or(key_start, |s| s.start);
                path.push(name.to_owned());
                walk_value(value, path, start, sink);
                path.pop();
            }
        }
        toml_edit::Item::Value(v) => walk_value(v, path, key_start, sink),
        toml_edit::Item::None => {}
    }
}

fn walk_value<F>(value: &toml_edit::Value, path: &mut Vec<String>, key_start: usize, sink: &mut F)
where
    F: FnMut(&[String], toml::Value, usize),
{
    if let toml_edit::Value::InlineTable(t) = value {
        for (name, inner) in t.iter() {
            let start = t.key(name).and_then(|k| k.span()).map_or(key_start, |s| s.start);
            path.push(name.to_owned());
            walk_value(inner, path, start, sink);
            path.pop();
        }
        return;
    }
    let start = value.span().map_or(key_start, |s| s.start).min(key_start);
    sink(path, convert(value), start);
}

/// Convert a `toml_edit` value into a plain `toml` one. Arrays are leaves.
fn convert(value: &toml_edit::Value) -> toml::Value {
    match value {
        toml_edit::Value::String(s) => toml::Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => toml::Value::Integer(*i.value()),
        toml_edit::Value::Float(f) => toml::Value::Float(*f.value()),
        toml_edit::Value::Boolean(b) => toml::Value::Boolean(*b.value()),
        toml_edit::Value::Datetime(d) => toml::Value::String(d.value().to_string()),
        toml_edit::Value::Array(a) => toml::Value::Array(a.iter().map(convert).collect()),
        toml_edit::Value::InlineTable(t) => {
            let mut table = toml::value::Table::new();
            for (k, v) in t.iter() {
                table.insert(k.to_owned(), convert(v));
            }
            toml::Value::Table(table)
        }
    }
}
