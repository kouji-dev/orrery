//! A skill's `scripts/`, run under the grant configuration declared for it.
//!
//! # The counterfactual, because it is the whole point
//!
//! In every other runtime, a skill's bundled `scripts/` are executed by the
//! agent's shell tool. They inherit the user's privileges entirely: the whole
//! home directory, every credential on the box, the network. A skill is a
//! Markdown file with a folder next to it, so it reads as documentation and is
//! reviewed like documentation — which makes it a **better attack vector than
//! an extension**, because an extension at least announces itself as code.
//!
//! Here a script runs the way a tool call runs:
//!
//! 1. **No grant, no script.** [`SkillError::NoGrant`] before anything starts.
//! 2. The process is created by `orrery-broker`, against a single-use
//!    [`CapabilityToken`](orrery_policy::CapabilityToken) the policy engine
//!    minted — so what may run is decided where the process is created, not by
//!    the string that matched a rule.
//! 3. It is **budgeted**: wall clock, output bytes and memory, enforced while
//!    the bytes are produced. A script that runs forever is stopped.
//! 4. Its effects go through the broker too, and one outside the grant is
//!    **denied and audited** rather than performed.
//!
//! # The effect channel, and what it does and does not prove
//!
//! A skill script has no ambient filesystem: it asks. One JSON object per line
//! on stdout — `{"effect":"write","path":…,"contents":…}` — is a request the
//! host brokers; every other line is plain output. Each request is checked
//! against the skill's grant and either carried out or refused with a rule name.
//!
//! This confines a script that uses the channel, which is the interface a skill
//! is written against. It does not, on its own, stop a **hostile binary** from
//! calling `open(2)` directly; that is
//! [`orrery_broker::contain`]'s job — a job object on Windows, a cgroup and a
//! seccomp filter elsewhere — and it is enforced on the same spawn this module
//! makes. The two are layers of one answer, and neither is claimed to be the
//! other.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orrery_audit::Audit;
use orrery_broker::{Broker, SpawnSpec};
use orrery_policy::{Decision, PendingCall, PolicyEngine};
use orrery_proto::{AgentScope, CallId, Grant, RuleId, Subject};
use orrery_tools::ToolBudget;
use serde::{Deserialize, Serialize};
use crate::error::SkillError;
use crate::scope::SkillRef;

/// The subject a skill's scripts act as.
///
/// A sub-agent rather than an extension: [`ExtId`](orrery_proto::ExtId) admits
/// one dotted form and it is reserved for `mcp.<server>`, and a skill is not an
/// extension anyway. `agent:skill:<name>` sits under the main agent, so a skill
/// is narrowed by the agent that loaded it and can never be wider than it.
#[must_use]
pub fn subject_of(skill: &str) -> Subject {
    Subject::SubAgent(format!("skill:{skill}"))
}

/// One thing a script asked the host to do.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "effect", rename_all = "kebab-case")]
pub enum Effect {
    /// Write a file.
    Write {
        /// Where.
        path: String,
        /// What.
        #[serde(default)]
        contents: String,
    },
}

/// What happened to a requested effect.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectOutcome {
    /// The grant covered it and it was carried out.
    Applied,
    /// The grant did not cover it. **A denial is a value**, exactly as it is
    /// for a tool call.
    Denied {
        /// Which rule refused, when a rule did.
        rule: RuleId,
        /// Why, in words a person can act on.
        reason: String,
    },
    /// The grant covered it and it still went wrong.
    Failed {
        /// What the broker said.
        message: String,
    },
}

impl EffectOutcome {
    /// Whether this effect was refused.
    #[must_use]
    pub fn is_denied(&self) -> bool {
        matches!(self, EffectOutcome::Denied { .. })
    }
}

/// One requested effect and what became of it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectRecord {
    /// What was asked for.
    pub effect: Effect,
    /// What happened.
    pub outcome: EffectOutcome,
}

/// What one run of a script produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptRun {
    /// Everything the script printed that was not an effect request.
    pub output: String,
    /// What it printed on stderr, up to the budget.
    pub stderr: String,
    /// Its exit code, when it had one.
    pub code: Option<i32>,
    /// Every effect it asked for, in order.
    pub effects: Vec<EffectRecord>,
    /// Whether the wall-clock budget had to stop it.
    pub timed_out: bool,
    /// Whether the output ceiling cut it short.
    pub truncated: bool,
}

impl ScriptRun {
    /// Whether anything the script asked for was refused.
    #[must_use]
    pub fn any_denied(&self) -> bool {
        self.effects.iter().any(|e| e.outcome.is_denied())
    }
}

/// Runs a skill's scripts: policy, then the broker, then the effects.
///
/// Holds the engine and the broker rather than taking them per call, because a
/// call site that can pass a different policy is a call site that will.
pub struct ScriptRunner {
    engine: Arc<PolicyEngine>,
    broker: Arc<dyn Broker>,
    audit: Audit,
}

impl std::fmt::Debug for ScriptRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptRunner").finish_non_exhaustive()
    }
}

impl ScriptRunner {
    /// A runner over the engine that mints and the broker that redeems.
    ///
    /// They must share a ledger — `LocalBroker::new(engine.ledger().clone())` —
    /// or every token will be [`Unknown`](orrery_policy::TokenError::Unknown).
    #[must_use]
    pub fn new(engine: Arc<PolicyEngine>, broker: Arc<dyn Broker>) -> Self {
        Self {
            engine,
            broker,
            audit: orrery_audit::null(),
        }
    }

    /// Also record what the runner itself decided.
    ///
    /// The engine records every capability decision on its own audit; this one
    /// is for the run as a whole. Pass the same sink to both.
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// Run one of a skill's bundled scripts.
    ///
    /// # Errors
    ///
    /// [`SkillError::NoGrant`] when the skill declares none — that is the rule,
    /// not an edge case. [`SkillError::NoSuchScript`] when the file is not
    /// there, [`SkillError::SpawnDenied`] when policy refuses to start it at
    /// all, and [`SkillError::Broker`] when the process could not be created.
    pub async fn run(
        &self,
        skill: &SkillRef,
        script: &str,
        args: &[String],
        caller: &AgentScope,
        budget: ToolBudget,
    ) -> Result<ScriptRun, SkillError> {
        // 1 · No grant, no scripts. Before the path is even resolved.
        let Some(spec) = &skill.grant else {
            return Err(SkillError::NoGrant {
                skill: skill.name.clone(),
            });
        };
        let scope = self.scope_for(skill, spec.apply_to(&caller.grant), caller);
        let subject = subject_of(&skill.name);

        let path = skill.script_path(script);
        if !path.is_file() {
            return Err(SkillError::NoSuchScript {
                skill: skill.name.clone(),
                script: script.to_owned(),
            });
        }

        // 2 · Policy decides whether it may start, and mints the one token the
        //     broker will redeem to create the process.
        let call = CallId::new();
        let spawn = SpawnSpec {
            program: path.display().to_string(),
            args: args.to_vec(),
            cwd: Some(skill.dir().to_path_buf()),
            env: std::collections::BTreeMap::new(),
        };
        let decision = self.engine.check(
            &PendingCall::spawn(spawn.command_text()).in_call(call),
            &subject,
            &scope,
        );
        let Decision::Allow { token, .. } = decision else {
            return Err(SkillError::SpawnDenied {
                skill: skill.name.clone(),
                script: script.to_owned(),
                reason: reason_of(&decision),
            });
        };

        // 3 · The broker creates it, contained and watched. The budget is
        //     enforced here, not checked afterwards.
        let child = self
            .broker
            .spawn(token, spawn, &budget)
            .await
            .map_err(|e| SkillError::Broker {
                skill: skill.name.clone(),
                message: e.to_string(),
            })?;
        let output = child.wait().await.map_err(|e| SkillError::Broker {
            skill: skill.name.clone(),
            message: e.to_string(),
        })?;

        // 4 · Every effect it asked for goes through the same check.
        let (plain, requested) = split_effects(&String::from_utf8_lossy(&output.stdout));
        let mut effects = Vec::with_capacity(requested.len());
        for effect in requested {
            let outcome = self.broker_effect(skill, &effect, &subject, &scope, call).await;
            effects.push(EffectRecord { effect, outcome });
        }

        Ok(ScriptRun {
            output: plain,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.code,
            effects,
            timed_out: output.timed_out,
            truncated: output.truncated,
        })
    }

    /// The scope the script runs under: the skill's grant, already narrowed
    /// against the agent that loaded it.
    fn scope_for(&self, skill: &SkillRef, grant: Grant, caller: &AgentScope) -> AgentScope {
        AgentScope {
            agent: format!("skill:{}", skill.name),
            branch: caller.branch,
            // A script is not a model: it is offered no tools at all.
            tools: Vec::new(),
            grant,
        }
    }

    async fn broker_effect(
        &self,
        skill: &SkillRef,
        effect: &Effect,
        subject: &Subject,
        scope: &AgentScope,
        call: CallId,
    ) -> EffectOutcome {
        let Effect::Write { path, contents } = effect;
        let decision = self
            .engine
            .check(&PendingCall::write(path.clone()).in_call(call), subject, scope);
        let Decision::Allow { token, .. } = decision else {
            let outcome = EffectOutcome::Denied {
                rule: decision.rule(),
                reason: reason_of(&decision),
            };
            // The engine already recorded the capability decision. This one
            // says which skill's script it was, which the capability event
            // cannot: it names the subject, not the file.
            self.audit.append(orrery_audit::AuditEvent::tool_call(
                call,
                format!("skill.{}.write", skill.name),
                &serde_json::json!({ "path": path }),
                orrery_audit::CallOutcome::Denied,
            ));
            tracing::warn!(
                target: "orrery.skills.scripts",
                skill = %skill.name,
                path = %path,
                "a skill script asked to write outside its grant; refused"
            );
            return outcome;
        };

        match self.broker.write(token, Path::new(path), false).await {
            Ok(mut handle) => {
                let written = match handle.write_all(contents.as_bytes()).await {
                    Ok(()) => handle.commit().await,
                    Err(e) => Err(e),
                };
                match written {
                    Ok(()) => {
                        self.audit.append(orrery_audit::AuditEvent::tool_call(
                            call,
                            format!("skill.{}.write", skill.name),
                            &serde_json::json!({ "path": path }),
                            orrery_audit::CallOutcome::Ok,
                        ));
                        EffectOutcome::Applied
                    }
                    Err(e) => EffectOutcome::Failed {
                        message: e.to_string(),
                    },
                }
            }
            Err(e) => EffectOutcome::Failed {
                message: e.to_string(),
            },
        }
    }
}

/// Split a script's stdout into plain output and the effects it requested.
fn split_effects(stdout: &str) -> (String, Vec<Effect>) {
    let mut plain = String::new();
    let mut effects = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') {
            if let Ok(effect) = serde_json::from_str::<Effect>(trimmed) {
                effects.push(effect);
                continue;
            }
        }
        plain.push_str(line);
        plain.push('\n');
    }
    (plain, effects)
}

fn reason_of(decision: &Decision) -> String {
    match decision {
        Decision::Deny { reason, .. } => reason.clone(),
        Decision::Ask { prompt, .. } => prompt.reason.clone(),
        // `Decision` is `#[non_exhaustive]`: a verdict this crate has not been
        // taught about is not a licence to claim a reason it does not have.
        _ => String::new(),
    }
}

/// The directory a skill's scripts live in, for a caller that has only a path.
#[must_use]
pub fn scripts_dir(skill_md: &Path) -> PathBuf {
    skill_md
        .parent()
        .unwrap_or(Path::new("."))
        .join("scripts")
}

#[cfg(test)]
mod tests {
    use super::{Effect, split_effects, subject_of};
    use orrery_proto::Subject;

    #[test]
    fn plain_output_and_effects_are_told_apart() {
        let (plain, effects) = split_effects(
            "hello\n{\"effect\":\"write\",\"path\":\"a.txt\",\"contents\":\"x\"}\n{ not json\n",
        );
        assert_eq!(plain, "hello\n{ not json\n");
        assert_eq!(
            effects,
            vec![Effect::Write {
                path: "a.txt".to_owned(),
                contents: "x".to_owned(),
            }]
        );
    }

    #[test]
    fn a_skill_is_a_sub_agent_under_the_main_agent() {
        assert_eq!(
            subject_of("review"),
            Subject::SubAgent("skill:review".to_owned())
        );
        assert_eq!(subject_of("review").to_string(), "agent:skill:review");
        assert_eq!(
            "agent:skill:review".parse::<Subject>().unwrap(),
            subject_of("review")
        );
    }
}
