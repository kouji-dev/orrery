//! Layers, subjects, and the one sync `fn` that decides.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arc_swap::ArcSwap;
use orrery_audit::{Audit, AuditEvent};
use orrery_proto::{
    AgentScope, Aspect, CallId, Capability, ConsentAnswerKind, ConsentPrompt, Layer, PromptId,
    RuleId, Subject,
};

use crate::call::PendingCall;
use crate::error::{PolicyError, Warning};
use crate::explain::{Explanation, RuleMatch};
use crate::r#match::Compiled;
use crate::parse::{self, LoadedLayer};
use crate::rule::{Rule, RuleList};
use crate::token::{CapabilityToken, ResolvedScope, TokenLedger, TokenMinter};

/// What was decided. **Denial is a value**, not an error.
#[non_exhaustive]
#[derive(Debug)]
pub enum Decision {
    /// Allowed. The token is single-use and good for this call only.
    Allow {
        /// The permission itself.
        token: CapabilityToken,
        /// What allowed it.
        rule: RuleId,
    },
    /// The user has to be asked.
    Ask {
        /// What to show them. Boxed: a prompt can carry a whole surface, and a
        /// `Decision` is returned from every single check.
        prompt: Box<ConsentPrompt>,
        /// What requires the ask.
        rule: RuleId,
        /// What happens if nobody answers, or if consent is turned off.
        fallback: Box<Decision>,
        /// The call this is about, so an answer mints a token tied to it and
        /// revoked with it. A prompt is a thing shown to a person and carries
        /// no call id of its own.
        call: CallId,
        /// What the answer would permit.
        aspect: Aspect,
        /// The resolved target the answer would permit it over.
        scope: ResolvedScope,
    },
    /// Refused.
    Deny {
        /// What refused it.
        rule: RuleId,
        /// Why, in words a person can act on.
        reason: String,
    },
}

/// The three verdicts, ordered, with `Deny` lowest.
///
/// This is the ordering narrowing runs under: a handler may only move a
/// decision **down** it.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// Refused.
    Deny,
    /// The user has to be asked.
    Ask,
    /// Allowed.
    Allow,
}

impl From<RuleList> for Verdict {
    fn from(list: RuleList) -> Self {
        match list {
            RuleList::Deny => Verdict::Deny,
            RuleList::Ask => Verdict::Ask,
            RuleList::Allow => Verdict::Allow,
        }
    }
}

impl Decision {
    /// Where this sits on the ordering.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        match self {
            Decision::Allow { .. } => Verdict::Allow,
            Decision::Ask { .. } => Verdict::Ask,
            Decision::Deny { .. } => Verdict::Deny,
        }
    }

    /// The rule that produced it.
    #[must_use]
    pub fn rule(&self) -> RuleId {
        match self {
            Decision::Allow { rule, .. }
            | Decision::Ask { rule, .. }
            | Decision::Deny { rule, .. } => *rule,
        }
    }

    /// The narrower of two decisions, with ties going to the first.
    ///
    /// This is the whole of narrowing enforcement: a handler returns a
    /// `Decision` and the engine keeps `min(proposed, returned)`.
    #[must_use]
    pub fn min(self, other: Decision) -> Decision {
        if other.verdict() < self.verdict() {
            other
        } else {
            self
        }
    }

    /// A refusal with no rule behind it.
    #[must_use]
    pub fn denied(reason: impl Into<String>) -> Decision {
        Decision::Deny {
            rule: no_rule(),
            reason: reason.into(),
        }
    }
}

/// The id a decision carries when no rule produced it — the closed default, a
/// handler's refusal, a panic.
#[must_use]
pub fn no_rule() -> RuleId {
    RuleId::from_uuid(uuid::Uuid::nil())
}

/// How a session answers `ask`.
///
/// `ask` is the **verdict**; consent is the **interaction**. A rule in the `ask`
/// list produces [`Decision::Ask`], which the kernel turns into a
/// `consent.request` frame; the answer comes back as `consent.answer`. A profile
/// with [`ConsentMode::Never`] resolves every ask to its fallback without
/// prompting anybody.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum ConsentMode {
    /// Ask the user.
    #[default]
    Ask,
    /// Never ask: every ask takes its fallback.
    Never,
}

/// An answer to a [`ConsentPrompt`], or the absence of one.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConsentAnswer {
    /// They answered.
    Answered(ConsentAnswerKind),
    /// Nobody answered before the deadline.
    TimedOut,
}

/// Every layer's rules, compiled against one workspace root.
#[derive(Debug)]
pub struct ResolvedRules {
    root: PathBuf,
    /// Compiled, in evaluation order: every deny, then every ask, then every
    /// allow. Within a list, layers run **closest first** and rules in the order
    /// they were written, so first match wins with specificity irrelevant.
    rules: Vec<Compiled>,
    warnings: Vec<Warning>,
}

impl ResolvedRules {
    /// An empty rule set, which denies everything.
    #[must_use]
    pub fn empty(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            rules: Vec::new(),
            warnings: Vec::new(),
        }
    }

    /// The workspace root paths resolve against.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What load raised.
    #[must_use]
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// The rule behind an id.
    #[must_use]
    pub fn rule(&self, id: RuleId) -> Option<&Rule> {
        self.rules.iter().find(|c| c.rule.id == id).map(|c| &c.rule)
    }

    /// How many rules there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether there are none, in which case everything is denied.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The first rule that matches this call for this subject, and its verdict.
    ///
    /// Lists in order — deny, ask, allow — and **first match wins**: an allow
    /// can never carve an exception out of a deny, because the deny list has
    /// already been walked.
    fn first_match(&self, call: &PendingCall, subject: &Subject) -> Option<&Compiled> {
        self.rules
            .iter()
            .find(|c| c.rule.subject == *subject && c.matches(call))
    }

    /// Whether this subject has any rules written about it at all.
    ///
    /// A subject with **no rule file of its own inherits its parent's set**; a
    /// subject with one narrows it. "A rule file narrows a sub-agent, it never
    /// widens one" is about what a file does, not about the absence of one —
    /// and a sub-agent nobody wrote rules for should be able to do what the
    /// agent that spawned it can do, not nothing.
    #[must_use]
    pub fn mentions(&self, subject: &Subject) -> bool {
        self.rules.iter().any(|c| c.rule.subject == *subject)
    }

    /// Every rule that was looked at for this subject, matched or not.
    fn considered(&self, call: &PendingCall, subject: &Subject) -> Vec<RuleMatch> {
        self.rules
            .iter()
            .filter(|c| c.rule.subject == *subject && c.rule.aspect == call.aspect)
            .map(|c| RuleMatch::of(&c.rule, c.matches(call)))
            .collect()
    }
}

/// Builds a [`ResolvedRules`] from config layers.
///
/// Layers may be added in any order; the builder sorts them so that **deny runs
/// before ask before allow**, and within a list the closest layer first. That is
/// what makes deny a union across layers and a managed deny final: the managed
/// deny has already been walked before any allow is looked at, from any layer.
#[derive(Debug)]
pub struct PolicyBuilder {
    root: PathBuf,
    loaded: Vec<LoadedLayer>,
    forbid_regex: bool,
}

impl PolicyBuilder {
    /// A builder rooted at a workspace.
    ///
    /// The root is canonicalised once, so every path pattern and every call
    /// target resolve against the same real directory.
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root =
            dunce::canonicalize(root.as_ref()).unwrap_or_else(|_| root.as_ref().to_path_buf());
        Self {
            root,
            loaded: Vec::new(),
            forbid_regex: false,
        }
    }

    /// Forbid the `re:` escape hatch outright.
    ///
    /// Open question 2 of the plan, decided: **yes, a managed layer can forbid
    /// it.** It is the same shape as `ext`-style load-time policy, it costs one
    /// flag, and an organisation that has decided nobody writes unreadable
    /// permission rules should not have to review every layer to enforce it.
    #[must_use]
    pub fn forbid_regex(mut self) -> Self {
        self.forbid_regex = true;
        self
    }

    /// Add one config file's rules.
    ///
    /// # Errors
    ///
    /// When the file does not parse, or uses `re:` while it is off.
    pub fn layer_toml(
        mut self,
        text: &str,
        file: impl AsRef<Path>,
        layer: Layer,
        regex_allowed: bool,
    ) -> Result<Self, PolicyError> {
        let allowed = regex_allowed && !self.forbid_regex;
        let loaded = parse::load_toml(text, file, layer, allowed)?;
        self.loaded.push(loaded);
        Ok(self)
    }

    /// Add rules built in code.
    #[must_use]
    pub fn layer_rules(mut self, layer: Layer, rules: Vec<Rule>) -> Self {
        self.loaded.push(LoadedLayer {
            layer,
            rules,
            warnings: Vec::new(),
        });
        self
    }

    /// Compile everything.
    ///
    /// # Errors
    ///
    /// When a pattern does not compile.
    pub fn build(self) -> Result<ResolvedRules, PolicyError> {
        let mut warnings = Vec::new();
        let mut flat: Vec<Rule> = Vec::new();
        for loaded in self.loaded {
            warnings.extend(loaded.warnings);
            flat.extend(loaded.rules);
        }
        if self.forbid_regex {
            if let Some(bad) = flat.iter().find(|r| r.selector.regex.is_some()) {
                return Err(PolicyError::RegexForbidden {
                    rule: bad.text.clone(),
                });
            }
        }

        // Deny before ask before allow; within a list, closest layer first, and
        // within a layer, the order the rules were written.
        let mut indexed: Vec<(usize, Rule)> = flat.into_iter().enumerate().collect();
        indexed.sort_by(|(ai, a), (bi, b)| {
            a.list
                .cmp(&b.list)
                .then_with(|| b.layer.cmp(&a.layer))
                .then_with(|| ai.cmp(bi))
        });

        let mut rules = Vec::with_capacity(indexed.len());
        for (_, rule) in indexed {
            rules.push(Compiled::new(rule, &self.root)?);
        }
        Ok(ResolvedRules {
            root: self.root,
            rules,
            warnings,
        })
    }
}

/// Rules, the matcher, the minter and the audit, behind one sync `check`.
///
/// # Rules decide whether to ask; the broker and the token decide what can be
/// touched
///
/// Claude Code's docs are candid that a `Bash(curl *)` deny stops
/// `curl https://x` but not `/usr/bin/curl https://x` or `sh -c 'curl
/// https://x'`. Patterns describe intent; they do not enforce. A `spawn` grant
/// is enforced where the process is created, by `orrery-broker`, against a
/// token — not by the string that matched.
#[derive(Debug)]
pub struct PolicyEngine {
    rules: ArcSwap<ResolvedRules>,
    minter: TokenMinter,
    audit: Audit,
    consent: ConsentMode,
}

impl PolicyEngine {
    /// An engine over a rule set.
    ///
    /// Takes a [`ResolvedRules`] or an `Arc` of one, so a composition root that
    /// resolved the rules once — and hands the *same* set to `permissions
    /// explain` and to the kernel — does not have to compile them twice.
    #[must_use]
    pub fn new(rules: impl Into<Arc<ResolvedRules>>) -> Self {
        Self {
            rules: ArcSwap::new(rules.into()),
            minter: TokenMinter::new(Arc::new(TokenLedger::new())),
            audit: orrery_audit::null(),
            consent: ConsentMode::default(),
        }
    }

    /// Record every decision in an audit stream.
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// Use a minter — and therefore a ledger — the broker already holds.
    #[must_use]
    pub fn with_minter(mut self, minter: TokenMinter) -> Self {
        self.minter = minter;
        self
    }

    /// Set what an `ask` verdict does.
    #[must_use]
    pub fn with_consent(mut self, consent: ConsentMode) -> Self {
        self.consent = consent;
        self
    }

    /// The ledger the broker redeems against.
    #[must_use]
    pub fn ledger(&self) -> &Arc<TokenLedger> {
        self.minter.ledger()
    }

    /// The rules in force right now.
    #[must_use]
    pub fn rules(&self) -> arc_swap::Guard<Arc<ResolvedRules>> {
        self.rules.load()
    }

    /// Swap the rule set without blocking a single in-flight `check`.
    pub fn reload(&self, rules: ResolvedRules) {
        self.rules.store(Arc::new(rules));
    }

    /// **Sync. Pure. No I/O is possible from here.**
    ///
    /// Not "fast enough": being a non-async `fn` makes "policy performs no I/O"
    /// a compile-time property rather than a convention.
    ///
    /// A subject's effective set is its own rules **intersected with its
    /// parent's**: a rule file narrows a sub-agent, it never widens one. With no
    /// rule matching at all the answer is [`Decision::Deny`], because a
    /// permission system whose default is "yes" is a permission system in name.
    #[must_use]
    pub fn check(&self, call: &PendingCall, subject: &Subject, scope: &AgentScope) -> Decision {
        let rules = self.rules.load();
        let mut decision = self.dress(&rules, call, &outcome(&rules, call, subject, Some(scope)));

        if self.consent == ConsentMode::Never {
            if let Decision::Ask { fallback, .. } = decision {
                decision = *fallback;
            }
        }

        self.record(call, subject, &decision, &rules);
        decision
    }

    /// Fold an answer into an [`Decision::Ask`]. Consumes the decision.
    #[must_use]
    pub fn consent(&self, decision: Decision, answer: ConsentAnswer) -> Decision {
        let Decision::Ask {
            rule,
            fallback,
            call,
            aspect,
            scope,
            ..
        } = decision
        else {
            return decision;
        };
        match answer {
            ConsentAnswer::TimedOut => *fallback,
            ConsentAnswer::Answered(
                ConsentAnswerKind::AllowOnce | ConsentAnswerKind::AllowAlways,
            ) => Decision::Allow {
                token: self.minter.mint(call, aspect, scope, rule),
                rule,
            },
            ConsentAnswer::Answered(_) => Decision::Deny {
                rule,
                reason: "the user refused".to_owned(),
            },
        }
    }

    /// Dry-run a call: what would happen, which rule would do it, and where
    /// that rule is written.
    ///
    /// The verdict is [`PolicyEngine::check`]'s verdict, because both come from
    /// the same [`outcome`]: naming the rule, the layer, the file and the line
    /// is the *only* thing this adds. It has to be the only thing. When the two
    /// worked the answer out separately they disagreed about a subject nobody
    /// had written a rule about, and the tool list — which is built from this
    /// side — came out empty for a run that would have been allowed.
    #[must_use]
    pub fn explain(&self, call: &PendingCall, subject: &Subject) -> Explanation {
        self.explained(call, subject, None)
    }

    /// The same dry run, against a scope's grant as well as the rules.
    ///
    /// [`PolicyEngine::explain`] answers as a scope that narrows nothing would;
    /// this answers for one particular sub-agent, ceiling included.
    #[must_use]
    pub fn explain_in(
        &self,
        call: &PendingCall,
        subject: &Subject,
        scope: &AgentScope,
    ) -> Explanation {
        self.explained(call, subject, Some(scope))
    }

    fn explained(
        &self,
        call: &PendingCall,
        subject: &Subject,
        scope: Option<&AgentScope>,
    ) -> Explanation {
        let rules = self.rules.load();
        let outcome = outcome(&rules, call, subject, scope);
        let inherited = outcome.subject != *subject;
        let reason = match (outcome.rule, &outcome.refusal) {
            (_, Some(why)) => why.clone(),
            (Some(rule), None) if inherited => format!(
                "`{}` in the {} list, inherited from `{}`",
                rule.text,
                rule.list.keyword(),
                outcome.subject
            ),
            (Some(rule), None) => {
                format!("`{}` in the {} list", rule.text, rule.list.keyword())
            }
            (None, None) => "no rule matched, and the default is to refuse".to_owned(),
        };
        Explanation {
            subject: subject.clone(),
            request: call.match_text(),
            verdict: outcome.verdict,
            rule: outcome.rule.map(|rule| RuleMatch::of(rule, true)),
            considered: subjects_for(&rules, subject)
                .iter()
                .flat_map(|s| rules.considered(call, s))
                .collect(),
            reason,
        }
    }

    /// Turn an [`Outcome`] into a decision: mint the token, build the prompt.
    ///
    /// Nothing here chooses anything — the choosing happened in [`outcome`].
    fn dress(&self, rules: &ResolvedRules, call: &PendingCall, outcome: &Outcome<'_>) -> Decision {
        let Some(matched) = outcome.rule else {
            return Decision::denied(
                outcome
                    .refusal
                    .clone()
                    .unwrap_or_else(|| "no rule matched, and the default is to refuse".to_owned()),
            );
        };
        let rule = matched.id;
        match outcome.verdict {
            Verdict::Deny => Decision::Deny {
                rule,
                reason: format!("`{}` denies `{}`", matched.text, call.match_text()),
            },
            Verdict::Allow => Decision::Allow {
                token: self
                    .minter
                    .mint(call.call, call.aspect, resolved_scope(call, rules), rule),
                rule,
            },
            Verdict::Ask => Decision::Ask {
                prompt: Box::new(ConsentPrompt {
                    id: PromptId::new(),
                    subject: outcome.subject.clone(),
                    capabilities: vec![Capability::scoped(call.aspect, [call.target.clone()])],
                    reason: format!("`{}` asks before `{}`", matched.text, call.match_text()),
                    rule: Some(rule),
                    surface: None,
                }),
                rule,
                fallback: Box::new(Decision::Deny {
                    rule,
                    reason: "nobody answered, and an unanswered ask refuses".to_owned(),
                }),
                call: call.call,
                aspect: call.aspect,
                scope: resolved_scope(call, rules),
            },
        }
    }

    fn record(
        &self,
        call: &PendingCall,
        subject: &Subject,
        decision: &Decision,
        rules: &ResolvedRules,
    ) {
        let rule = rules.rule(decision.rule());
        self.audit.append(AuditEvent::CapabilityDecision {
            subject: subject.clone(),
            request: call.match_text(),
            verdict: match decision.verdict() {
                Verdict::Allow => orrery_audit::Verdict::Allow,
                Verdict::Ask => orrery_audit::Verdict::Ask,
                Verdict::Deny => orrery_audit::Verdict::Deny,
            },
            rule: rule.map(|r| r.id),
            rule_text: rule.map(|r| r.text.clone()),
            layer: rule.map(|r| r.layer),
            reason: match decision {
                Decision::Deny { reason, .. } => Some(reason.clone()),
                _ => None,
            },
        });
    }
}

/// What the rules say about one call, before anybody dresses it up.
///
/// **The one thing both `check` and `explain` are built from.** A decision
/// mints a token from it and an explanation prints the rule behind it; neither
/// works the answer out for itself, so neither can drift from the other.
#[derive(Debug)]
struct Outcome<'r> {
    /// Whose rules answered — not always the subject that asked, because a
    /// subject nobody wrote about inherits.
    subject: Subject,
    /// The rule that answered, when a rule did.
    rule: Option<&'r Rule>,
    /// What the answer is.
    verdict: Verdict,
    /// Why, when no rule allowed it: the closed default, or the scope's own
    /// ceiling. `None` whenever `rule` is `Some`.
    refusal: Option<String>,
}

/// Whose rules a call is judged against, in the order they are consulted.
///
/// A subject's effective set is its own rules **intersected with its parent's**:
/// a rule file narrows a sub-agent, it never widens one. A subject with no rule
/// written about it at all inherits its parent's set outright, rather than
/// being silently denied everything.
fn subjects_for(rules: &ResolvedRules, subject: &Subject) -> Vec<Subject> {
    match (parent_of(subject), rules.mentions(subject)) {
        (Some(parent), false) => vec![parent],
        (Some(parent), true) => vec![subject.clone(), parent],
        (None, _) => vec![subject.clone()],
    }
}

/// The answer to one call: the narrowest verdict across the subject chain,
/// then the scope's own grant, which no rule can widen.
///
/// Ties go to the first subject consulted, which is the caller's own, so a
/// denial names the rule that is actually about them.
fn outcome<'r>(
    rules: &'r ResolvedRules,
    call: &PendingCall,
    subject: &Subject,
    scope: Option<&AgentScope>,
) -> Outcome<'r> {
    let mut best: Option<Outcome<'r>> = None;
    for subject in subjects_for(rules, subject) {
        let next = match rules.first_match(call, &subject) {
            Some(matched) => Outcome {
                subject,
                rule: Some(&matched.rule),
                verdict: Verdict::from(matched.rule.list),
                refusal: None,
            },
            None => Outcome {
                refusal: Some(format!(
                    "no rule allows `{}` for `{subject}`",
                    call.match_text()
                )),
                subject,
                rule: None,
                verdict: Verdict::Deny,
            },
        };
        best = Some(match best {
            Some(best) if best.verdict <= next.verdict => best,
            _ => next,
        });
    }
    let mut best = best.expect("a subject chain is never empty");

    // The scope's grant is the sub-agent's declared ceiling. A capability the
    // scope does not carry is not available to it however the rules read — and
    // only a verdict above `Deny` is left to lose, so a rule that already
    // refused keeps the credit for refusing.
    if let Some(scope) = scope {
        if best.verdict > Verdict::Deny && !scope_permits(scope, call) {
            best = Outcome {
                refusal: Some(format!(
                    "`{}` is outside the scope granted to `{}`",
                    call.match_text(),
                    scope.agent
                )),
                subject: best.subject,
                rule: None,
                verdict: Verdict::Deny,
            };
        }
    }
    best
}

fn resolved_scope(call: &PendingCall, rules: &ResolvedRules) -> ResolvedScope {
    if crate::r#match::is_path_aspect(call.aspect) {
        ResolvedScope::new(crate::r#match::normalise_path(&call.target, rules.root()))
            .under(rules.root().to_path_buf())
    } else {
        ResolvedScope::new(call.target.clone())
    }
}

/// Whose rules also apply. An extension and a sub-agent both sit under the
/// agent, and the agent sits under nobody.
fn parent_of(subject: &Subject) -> Option<Subject> {
    match subject {
        Subject::Agent => None,
        _ => Some(Subject::Agent),
    }
}

/// Whether the scope's grant carries this aspect at all.
///
/// The grant is the sub-agent's declared ceiling from plan 01; the rules are the
/// operator's. A call has to clear both.
fn scope_permits(scope: &AgentScope, call: &PendingCall) -> bool {
    if scope.grant.capabilities.is_empty() {
        // An empty grant is the empty grant, not an unqualified one — except
        // for the main agent's default scope, which carries none and is not
        // meant to narrow anything. Treat "no capabilities declared at all" as
        // "the grant is not being used to narrow", and let the rules decide.
        return true;
    }
    scope
        .grant
        .capabilities
        .iter()
        .filter(|c| c.aspect == call.aspect)
        .any(|c| {
            // A pattern aspect is not an exact-match set: the rules narrow it,
            // not this. Only the exact-match aspects are checked by name here.
            c.scope.is_empty()
                || !call.aspect.is_exact_match()
                || c.scope.iter().any(|s| s == &call.target || s == "*")
        })
}
