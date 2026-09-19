//! Profiles: a named composition of everything the runtime assembles.
//!
//! This is what makes "configure your own harness" concrete. One binary, two
//! profiles, two measurably different agents: a different model, a different
//! visible tool set, a different answer to "may I write" and a different answer
//! to "is there anybody there to ask".
//!
//! # The permission shorthands
//!
//! Open question 4, decided: **a small, documented, closed set**, because
//! §4.9's own example uses one and a shorthand nobody can enumerate is worse
//! than no shorthand at all. Exactly five keys, each expanding to one rule in
//! the real grammar:
//!
//! | Shorthand | `false` expands to | `true` expands to |
//! |---|---|---|
//! | `read`  | `deny = ["read(./**)"]`    | `allow = ["read(./**)"]` |
//! | `write` | `deny = ["write(./**)"]`   | `allow = ["write(./**)"]` |
//! | `net`   | `deny = ["net(domain: *)"]`| `allow = ["net(domain: *)"]` |
//! | `spawn` | `deny = ["spawn(*)"]`      | `allow = ["spawn(*)"]` |
//! | `creds` | `deny = ["creds(*)"]`      | `allow = ["creds(*)"]` |
//!
//! Anything else is a load error naming the file and the line. Nothing here can
//! carve an exception out of a deny: a shorthand `true` is an `allow`, and the
//! deny list is walked first, from every layer.

use std::collections::BTreeMap;
use std::path::Path;

use orrery_policy::{ConsentMode, PolicyEngine, ResolvedRules, Rule, RuleList, Source};
use orrery_proto::{Aspect, Budget, Layer, Subject};

use crate::error::ConfigError;
use crate::layer::LayerFile;
use crate::provenance::{Origin, Provenanced};

/// The five shorthand keys, and the rule each expands to.
pub const SHORTHANDS: [(&str, Aspect, &str); 5] = [
    ("read", Aspect::Read, "read(./**)"),
    ("write", Aspect::Write, "write(./**)"),
    ("net", Aspect::Net, "net(domain: *)"),
    ("spawn", Aspect::Spawn, "spawn(*)"),
    ("creds", Aspect::Creds, "creds(*)"),
];

/// One `permissions = { .. }` entry in a profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shorthand {
    /// Which of the five.
    pub key: String,
    /// Whether it was set to `true` — an allow — or `false` — a deny.
    pub allowed: bool,
    /// Where it was written.
    pub origin: Origin,
}

impl Shorthand {
    /// The rule it expands to, carrying where it was written.
    ///
    /// The source matters: `permissions explain` names the file and the line
    /// behind a verdict, and a shorthand that dropped its origin reported an
    /// empty file on line 0 — a rule from nowhere. When the origin has no file
    /// of its own the profile is named instead, because a profile is a real
    /// place a person can go and edit.
    #[must_use]
    pub fn rule_in(&self, profile: &str) -> Rule {
        let mut rule = self.rule();
        rule.source = if self.origin.file.as_os_str().is_empty() {
            Source::new(format!("<profile: {profile}>"), 0)
        } else {
            Source::new(&self.origin.file, self.origin.line)
        };
        rule
    }

    /// The rule it expands to.
    #[must_use]
    pub fn rule(&self) -> Rule {
        let (_, aspect, text) = SHORTHANDS
            .iter()
            .find(|(k, _, _)| *k == self.key)
            .copied()
            .expect("a shorthand is only built for a known key");
        let list = if self.allowed {
            RuleList::Allow
        } else {
            RuleList::Deny
        };
        Rule::new(list, self.origin.layer, Subject::Agent, aspect, text)
    }
}

/// One `[agents.<name>]` binding.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentDef {
    /// Its name.
    pub name: String,
    /// The agent it composes, when it composes one.
    pub uses: Option<String>,
    /// Its prompt, when it carries one.
    pub prompt: Option<String>,
    /// Its model.
    pub model: Option<String>,
    /// Its visible tool set.
    pub tools: Vec<String>,
    /// Its parameters, as written.
    pub params: BTreeMap<String, toml::Value>,
    /// What it may spend.
    pub budget: Option<Budget>,
}

/// A named composition of everything the runtime assembles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    /// Its name. Empty when no profile was asked for.
    pub name: String,
    /// The model it runs.
    pub model: Option<String>,
    /// The extensions it loads.
    pub extensions: Vec<String>,
    /// The interceptors it installs.
    pub interceptors: Vec<String>,
    /// The sub-agents it may spawn.
    pub subagents: Vec<String>,
    /// The skills it offers.
    pub skills: Vec<String>,
    /// The permission shorthands it sets.
    pub permissions: Vec<Shorthand>,
    /// What an `ask` verdict does.
    pub consent: ConsentMode,
    /// Where the audit stream goes.
    pub audit_sink: Option<String>,
    /// Every agent binding, which is session-wide rather than per profile.
    pub agents: BTreeMap<String, AgentDef>,
    /// Every role binding.
    pub roles: BTreeMap<String, String>,
}

/// A profile, assembled: what the kernel is built from.
///
/// The visible tool set, the model, the rules and the consent mode together are
/// what "two profiles produce measurably different agents from one binary"
/// means, and each one is asserted in `tests/profile.rs`.
#[derive(Debug)]
pub struct Assembled {
    /// The profile it came from.
    pub profile: Profile,
    /// The model to run.
    pub model: Option<String>,
    /// The visible tool set. A tool not named here is not offered at all.
    pub tools: Vec<String>,
    /// What an `ask` does.
    pub consent: ConsentMode,
    /// The rules, compiled, with the profile's shorthands folded in.
    pub engine: PolicyEngine,
}

/// Read the profile a context asked for, plus the session-wide agent and role
/// bindings.
///
/// # Errors
///
/// When the named profile is not defined, or a permission shorthand is not one
/// of the five. Both name the file and the line.
pub fn select(values: &Provenanced, name: Option<&str>) -> Result<Profile, ConfigError> {
    let mut profile = Profile {
        agents: agents(values),
        roles: roles(values),
        ..Profile::default()
    };
    let Some(name) = name else {
        return Ok(profile);
    };
    profile.name = name.to_owned();

    let prefix = format!("profile.{name}");
    if values.keys_under(&prefix).is_empty() {
        return Err(ConfigError::Invalid {
            file: "config.toml".into(),
            line: 0,
            message: format!("no profile named `{name}` is defined in any layer"),
        });
    }

    profile.model = values.str(&format!("{prefix}.model")).map(str::to_owned);
    profile.extensions = values.strings(&format!("{prefix}.extensions"));
    profile.interceptors = values.strings(&format!("{prefix}.interceptors"));
    profile.subagents = values.strings(&format!("{prefix}.subagents"));
    profile.skills = values.strings(&format!("{prefix}.skills"));
    profile.audit_sink = values
        .str(&format!("{prefix}.audit.sink"))
        .map(str::to_owned);
    profile.consent = match values.str(&format!("{prefix}.consent")) {
        Some("never") => ConsentMode::Never,
        _ => ConsentMode::Ask,
    };

    let perms = format!("{prefix}.permissions");
    for key in values.keys_under(&perms) {
        let Some(slot) = values.winner(key) else {
            continue;
        };
        let short = key.rsplit('.').next().unwrap_or(key).to_owned();
        if !SHORTHANDS.iter().any(|(k, _, _)| *k == short) {
            return Err(ConfigError::invalid(
                &slot.origin.file,
                slot.origin.line,
                format!(
                    "`{short}` is not a permission shorthand: expected one of {}, or write a real rule in `[permissions]`",
                    SHORTHANDS.iter().map(|(k, _, _)| *k).collect::<Vec<_>>().join(", ")
                ),
            ));
        }
        let Some(allowed) = slot.value.as_bool() else {
            return Err(ConfigError::invalid(
                &slot.origin.file,
                slot.origin.line,
                format!("`{short}` is a shorthand and takes `true` or `false`"),
            ));
        };
        profile.permissions.push(Shorthand {
            key: short,
            allowed,
            origin: slot.origin.clone(),
        });
    }

    Ok(profile)
}

/// Build the kernel's inputs from a profile and the layers in force.
///
/// # Errors
///
/// When a rule does not parse or a pattern does not compile.
pub fn assemble(
    profile: &Profile,
    root: &Path,
    layers: &[LayerFile],
) -> Result<Assembled, ConfigError> {
    let rules = rules(profile, root, layers)?;

    let tools = profile
        .extensions
        .iter()
        .map(|e| format!("{e}.*"))
        .collect();

    Ok(Assembled {
        profile: profile.clone(),
        model: profile.model.clone(),
        tools,
        consent: profile.consent,
        engine: PolicyEngine::new(rules).with_consent(profile.consent),
    })
}

/// The rules in force: every layer, plus the profile's shorthands.
///
/// The one place those two are combined. `permissions explain` prints what this
/// returns and the kernel dispatches through it, so an explanation cannot
/// describe a rule set the run does not have.
///
/// When no layer writes a permission rule the built-in default
/// ([`crate::merge::DEFAULT_PERMISSIONS`]) stands in, and a shorthand is folded
/// on top of it — a profile that says `read = false` narrows the default rather
/// than being the only rule in a workspace where nothing else is allowed.
///
/// # Errors
///
/// When a rule does not parse or a pattern does not compile.
pub fn rules(
    profile: &Profile,
    root: &Path,
    layers: &[LayerFile],
) -> Result<ResolvedRules, ConfigError> {
    let mut builder = if crate::merge::any_rules(root, layers)? {
        crate::merge::builder(root, layers)?
    } else {
        crate::merge::default_builder(root)?
    };
    let shorthands: Vec<Rule> = profile
        .permissions
        .iter()
        .map(|s| s.rule_in(&profile.name))
        .collect();
    if !shorthands.is_empty() {
        // The layer is only where the rules sort; a deny is walked before any
        // allow from any layer, so a shorthand deny is not relaxable either.
        builder = builder.layer_rules(Layer::Project, shorthands);
    }
    Ok(builder.build()?)
}

fn agents(values: &Provenanced) -> BTreeMap<String, AgentDef> {
    let mut out: BTreeMap<String, AgentDef> = BTreeMap::new();
    for key in values.keys_under("agents") {
        let parts: Vec<&str> = key.split('.').collect();
        let (Some(name), Some(field)) = (parts.get(1), parts.get(2)) else {
            continue;
        };
        let Some(slot) = values.winner(key) else {
            continue;
        };
        let def = out.entry((*name).to_owned()).or_insert_with(|| AgentDef {
            name: (*name).to_owned(),
            ..AgentDef::default()
        });
        match *field {
            "use" => def.uses = slot.value.as_str().map(str::to_owned),
            "prompt" => def.prompt = slot.value.as_str().map(str::to_owned),
            "model" => def.model = slot.value.as_str().map(str::to_owned),
            "tools" => {
                def.tools = slot
                    .value
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default();
            }
            "params" => {
                if let Some(param) = parts.get(3) {
                    def.params
                        .insert((*param).to_owned(), slot.value.clone());
                }
            }
            "budget" => {
                let budget = def.budget.get_or_insert_with(Budget::default);
                let n = slot.value.as_integer().unwrap_or_default();
                match parts.get(3).copied() {
                    Some("maxTurns") => budget.max_turns = u32::try_from(n).unwrap_or(u32::MAX),
                    Some("maxTokens") => budget.max_tokens = u64::try_from(n).unwrap_or(u64::MAX),
                    Some("wallClockMs") => {
                        budget.wall_clock_ms = u64::try_from(n).unwrap_or(u64::MAX);
                    }
                    Some("maxMicroUsd") => budget.max_micro_usd = u64::try_from(n).ok(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn roles(values: &Provenanced) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in values.keys_under("roles") {
        if let (Some(role), Some(slot)) = (key.split('.').nth(1), values.winner(key))
            && let Some(bound) = slot.value.as_str()
        {
            out.insert(role.to_owned(), bound.to_owned());
        }
    }
    out
}
