//! Step 4 · load and validate: agent bindings, role bindings, singleton
//! conflicts — all of it naming the file and the line.
//!
//! # Why this is not a serde error
//!
//! `unknown field \`maxIteration\`, expected one of ...` tells a person that
//! something is wrong somewhere in a tree of five merged files. The spans the
//! merge kept are what turns that into `config.toml:14`, and that is the whole
//! reason the merge does not deserialise into one struct.
//!
//! # A role bound to an agent that does not exist fails at `session.start`
//!
//! Not mid-turn, when the router first reaches for it and a run that has
//! already cost money falls over.

use crate::discover::DiscoveryManifest;
use crate::error::ConfigError;
use crate::provenance::{Origin, Provenanced};

/// The keys an `[agents.<name>]` table may set.
pub const AGENT_KEYS: [&str; 9] = [
    "use",
    "prompt",
    "model",
    "tools",
    "params",
    "budget",
    "role",
    "skills",
    "subagents",
];

/// The keys a `budget = { .. }` may set, from `orrery_proto::Budget`.
pub const BUDGET_KEYS: [&str; 4] = ["maxTurns", "maxTokens", "wallClockMs", "maxMicroUsd"];

/// Two layers claiming the same singleton.
///
/// Not an error: the closest layer wins, which is how names resolve. But the
/// loser is **named**, because a router that silently did not take effect is
/// the kind of thing people lose an afternoon to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SingletonConflict {
    /// Which singleton: `router`, `session-store`, and so on.
    pub name: String,
    /// What claimed it and won.
    pub winner: String,
    /// Where that was written.
    pub winner_origin: Origin,
    /// What claimed it and lost.
    pub loser: String,
    /// Where that was written.
    pub loser_origin: Origin,
}

impl std::fmt::Display for SingletonConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "`{}` is claimed twice: `{}` at {} wins by layer precedence; `{}` at {} does not take effect",
            self.name, self.winner, self.winner_origin, self.loser, self.loser_origin
        )
    }
}

/// What validation had to say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validation {
    /// Singletons more than one layer claimed.
    pub conflicts: Vec<SingletonConflict>,
    /// Things that are not errors but somebody should know about.
    pub warnings: Vec<String>,
}

impl Validation {
    /// The conflict over one singleton, if there was one.
    #[must_use]
    pub fn conflict(&self, name: &str) -> Option<&SingletonConflict> {
        self.conflicts.iter().find(|c| c.name == name)
    }
}

/// Validate the effective configuration.
///
/// # Errors
///
/// When an agent binding sets a key nothing knows about, a budget field is
/// misspelled, or a role names an agent that does not exist. Every error names
/// the file and the line it was written on.
pub fn validate(
    values: &Provenanced,
    manifest: &DiscoveryManifest,
) -> Result<Validation, ConfigError> {
    let mut out = Validation::default();
    agents(values)?;
    roles(values, manifest)?;
    out.conflicts = singletons(values);
    Ok(out)
}

/// Every agent binding's keys, and its budget's.
fn agents(values: &Provenanced) -> Result<(), ConfigError> {
    for key in values.keys_under("agents") {
        let Some(slot) = values.winner(key) else {
            continue;
        };
        let parts: Vec<&str> = key.split('.').collect();
        // agents.<name>.<field>[.<sub>...]
        let (Some(field), Some(agent)) = (parts.get(2), parts.get(1)) else {
            continue;
        };
        if !AGENT_KEYS.contains(field) {
            return Err(ConfigError::invalid(
                &slot.origin.file,
                slot.origin.line,
                format!(
                    "`{field}` is not something an agent binding sets on `{agent}`: expected one of {}",
                    AGENT_KEYS.join(", ")
                ),
            ));
        }
        if *field == "budget"
            && let Some(sub) = parts.get(3)
            && !BUDGET_KEYS.contains(sub)
        {
            return Err(ConfigError::invalid(
                &slot.origin.file,
                slot.origin.line,
                format!(
                    "`{sub}` is not a budget field: expected one of {}",
                    BUDGET_KEYS.join(", ")
                ),
            ));
        }
    }

    // A `params` table is checked against the parameters the agent it `use`s
    // declares, when that agent is defined here. A typo'd parameter is the
    // commonest of these and the one the plan names.
    for agent in agent_names(values) {
        let Some(target) = values.str(&format!("agents.{agent}.use")) else {
            continue;
        };
        let target = target.rsplit('.').next().unwrap_or(target);
        let declared: Vec<String> = param_names(values, target);
        if declared.is_empty() {
            continue;
        }
        for (name, origin) in params_of(values, &agent) {
            if !declared.contains(&name) {
                return Err(ConfigError::invalid(
                    &origin.file,
                    origin.line,
                    format!(
                        "`{name}` is not a parameter of `{target}`: expected one of {}",
                        declared.join(", ")
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn agent_names(values: &Provenanced) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in values.keys_under("agents") {
        if let Some(name) = key.split('.').nth(1) {
            if !out.iter().any(|n| n == name) {
                out.push(name.to_owned());
            }
        }
    }
    out
}

fn param_names(values: &Provenanced, agent: &str) -> Vec<String> {
    values
        .keys_under(&format!("agents.{agent}.params"))
        .into_iter()
        .filter_map(|k| k.split('.').nth(3).map(str::to_owned))
        .collect()
}

fn params_of(values: &Provenanced, agent: &str) -> Vec<(String, Origin)> {
    values
        .keys_under(&format!("agents.{agent}.params"))
        .into_iter()
        .filter_map(|k| {
            let name = k.split('.').nth(3)?.to_owned();
            let origin = values.winner(k)?.origin.clone();
            Some((name, origin))
        })
        .collect()
}

/// Every `[roles]` binding names an agent that exists.
fn roles(values: &Provenanced, manifest: &DiscoveryManifest) -> Result<(), ConfigError> {
    let defined = agent_names(values);
    for key in values.keys_under("roles") {
        let Some(slot) = values.winner(key) else {
            continue;
        };
        let Some(bound) = slot.value.as_str() else {
            continue;
        };
        let role = key.split('.').nth(1).unwrap_or(key);
        let short = bound.rsplit('.').next().unwrap_or(bound);
        let known = defined.iter().any(|a| a == bound || a == short)
            || manifest
                .extensions
                .iter()
                .any(|e| e.name == bound || bound.starts_with(&format!("{}.", e.name)));
        if !known {
            return Err(ConfigError::invalid(
                &slot.origin.file,
                slot.origin.line,
                format!(
                    "the `{role}` role is bound to `{bound}`, and no agent by that name is defined"
                ),
            ));
        }
    }
    Ok(())
}

/// Every singleton more than one layer claimed.
fn singletons(values: &Provenanced) -> Vec<SingletonConflict> {
    let mut out = Vec::new();
    for key in values.keys_under("singleton") {
        let Some(winner) = values.winner(key) else {
            continue;
        };
        let name = key.split('.').nth(1).unwrap_or(key).to_owned();
        for loser in values.shadowed(key) {
            out.push(SingletonConflict {
                name: name.clone(),
                winner: winner.value.as_str().unwrap_or_default().to_owned(),
                winner_origin: winner.origin.clone(),
                loser: loser.value.as_str().unwrap_or_default().to_owned(),
                loser_origin: loser.origin.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use orrery_proto::Layer;

    use crate::layer::LayerFile;

    fn values(files: &[LayerFile]) -> Provenanced {
        crate::merge::merge(files).expect("the fixture parses").values
    }

    /// A file whose interesting line is line 14.
    const TYPO: &str = r#"# 1
# 2
# 3
model = "claude-sonnet-5"
# 5
# 6
# 7
# 8
# 9
# 10
[agents.planner]
use = "myteam.architect"
model = "claude-sonnet-5"
paramz = { maxIterations = 2 }
"#;

    #[test]
    fn unknown_param_names_the_file_and_line() {
        let vals = values(&[LayerFile::new(Layer::Workspace, "config.toml", TYPO)]);
        let err = validate(&vals, &DiscoveryManifest::default()).expect_err("it fails at load");
        assert_eq!(err.file().unwrap().to_string_lossy(), "config.toml");
        assert_eq!(err.line(), Some(14), "{err}");
        assert!(err.to_string().starts_with("config.toml:14:"), "{err}");
        assert!(err.to_string().contains("paramz"), "{err}");
    }

    #[test]
    fn unknown_budget_field_names_the_file_and_line() {
        let text = "[agents.reviewer]\nprompt = \"review\"\nbudget = { maxTurns = 4, maxTokns = 60000 }\n";
        let vals = values(&[LayerFile::new(Layer::Workspace, "config.toml", text)]);
        let err = validate(&vals, &DiscoveryManifest::default()).expect_err("it fails at load");
        assert_eq!(err.line(), Some(3), "{err}");
        assert!(err.to_string().contains("maxTokns"), "{err}");
    }

    #[test]
    fn a_typod_parameter_of_a_used_agent_is_caught() {
        let text = concat!(
            "[agents.architect]\n",
            "prompt = \"plan\"\n",
            "params = { maxIterations = 1, depth = 1 }\n",
            "\n",
            "[agents.planner]\n",
            "use = \"architect\"\n",
            "params = { maxIteration = 2 }\n",
        );
        let vals = values(&[LayerFile::new(Layer::Workspace, "config.toml", text)]);
        let err = validate(&vals, &DiscoveryManifest::default()).expect_err("it fails at load");
        assert_eq!(err.line(), Some(7), "{err}");
        assert!(err.to_string().contains("maxIteration"), "{err}");
        assert!(err.to_string().contains("maxIterations"), "it says what was expected: {err}");
    }

    #[test]
    fn missing_role_binding_fails_at_session_start() {
        let text = "[agents.planner]\nprompt = \"plan\"\n\n[roles]\nplanner = \"planner\"\nrouter = \"chooser\"\n";
        let vals = values(&[LayerFile::new(Layer::Workspace, "config.toml", text)]);
        let err = validate(&vals, &DiscoveryManifest::default()).expect_err("it fails at load");
        assert_eq!(err.line(), Some(6), "{err}");
        assert!(err.to_string().contains("chooser"), "{err}");

        // And the same file with the agent defined passes.
        let ok = "[agents.planner]\nprompt = \"plan\"\n[agents.chooser]\nprompt = \"route\"\n[roles]\nrouter = \"chooser\"\n";
        let vals = values(&[LayerFile::new(Layer::Workspace, "config.toml", ok)]);
        validate(&vals, &DiscoveryManifest::default()).expect("it loads");
    }

    #[test]
    fn singleton_conflict_is_reported() {
        let vals = values(&[
            LayerFile::new(
                Layer::User,
                "user.toml",
                "[singleton]\nrouter = \"cheapest\"\n",
            ),
            LayerFile::new(
                Layer::Project,
                "project.toml",
                "[singleton]\nrouter = \"round-robin\"\n",
            ),
        ]);
        let report = validate(&vals, &DiscoveryManifest::default()).expect("it loads");
        let conflict = report.conflict("router").expect("reported");
        assert_eq!(conflict.winner, "round-robin", "the closest layer wins");
        assert_eq!(conflict.winner_origin.layer, Layer::Project);
        assert_eq!(conflict.loser, "cheapest", "and the loser is named");
        assert_eq!(conflict.loser_origin.layer, Layer::User);
        assert!(conflict.to_string().contains("does not take effect"));
    }
}
