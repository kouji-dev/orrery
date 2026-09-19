//! The one namespace: registration, the name table, precedence and the ledger.

use std::sync::Arc;

use indexmap::IndexMap;
use orrery_audit::{Audit, AuditEvent};
use orrery_proto::{Contribution, ContributionKind, ExtId, Layer, ToolRef};
use parking_lot::Mutex;

use crate::budget::ToolBudget;
use crate::dispatch::{AllowAll, PolicyCheck, ToolHost, ToolInterceptor, UnavailableHost};

/// One tool, as registered.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    /// The name inside its extension, unqualified.
    pub name: String,
    /// What it does, for the model.
    pub description: String,
    /// JSON Schema for the input.
    pub input_schema: serde_json::Value,
    /// Whether the effect is all-or-nothing.
    pub atomic: bool,
    /// The declared ceiling from the tool's manifest, when it names one. Folded
    /// into the effective budget by [`ToolBudget::effective`].
    pub ceiling: Option<ToolBudget>,
}

impl ToolSpec {
    /// A tool with an open object schema and no declared ceiling.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            input_schema: serde_json::json!({ "type": "object" }),
            atomic: false,
            ceiling: None,
        }
    }

    /// Set the description shown to the model.
    #[must_use]
    pub fn described(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the input schema validated at the boundary.
    #[must_use]
    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.input_schema = schema;
        self
    }

    /// Mark the effect all-or-nothing.
    #[must_use]
    pub fn atomic(mut self, atomic: bool) -> Self {
        self.atomic = atomic;
        self
    }

    /// Declare the tool's own budget ceiling.
    #[must_use]
    pub fn with_ceiling(mut self, ceiling: ToolBudget) -> Self {
        self.ceiling = Some(ceiling);
        self
    }
}

/// Where the owning extension is in its lifecycle.
///
/// Plan 06 landed the generation-keyed instance table in `orrery_host`, which
/// is the owner of record. This stays a deliberate copy, not a leftover: it
/// lets [`crate::visible`] and [`crate::resolve`] hide a draining extension
/// with a map lookup instead of an async call across crates on the hot path.
/// The host keeps the two in step through [`Registry::set_state`] — see
/// `orrery_host::unload`, which flips an extension to `Dead` there.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum ExtState {
    /// Loaded and serving calls.
    #[default]
    Live,
    /// Finishing what it has; no new calls.
    Draining,
    /// Gone.
    Dead,
}

/// A registered tool: its reference, the layer it came from and its spec.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The fully-qualified name.
    pub r#ref: ToolRef,
    /// Which layer contributed the winning registration.
    pub layer: Layer,
    /// What was registered.
    pub spec: ToolSpec,
    /// Whether the owning extension is still serving.
    pub state: ExtState,
    /// Registration order, so ordering within a layer is insertion order.
    pub order: usize,
}

/// Something the registry decided that a person may later have to explain.
///
/// Ambiguity is logged, never fatal: a duplicate tool name must not exit 1.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LedgerEntry {
    /// The same fully-qualified name was registered at two layers.
    Shadowed {
        /// The name both claimed.
        r#ref: ToolRef,
        /// The layer that took it.
        winner: Layer,
        /// The layer that lost it.
        loser: Layer,
    },
    /// A short name matched more than one tool, and one was chosen.
    Ambiguous {
        /// The short name as called.
        name: String,
        /// Everything it could have meant.
        candidates: Vec<ToolRef>,
        /// What it was taken to mean.
        chose: ToolRef,
    },
}

/// Every tool from every source, under one namespace, behind one dispatch path.
///
/// # Two directions, deliberately
///
/// For a **name**, the closest layer wins: a project-level `git.status` shadows
/// a user-level one, and a short name two extensions claim resolves to the
/// closer of the pair. For a **permission**, a managed deny is final and no
/// closer layer can lift it (section 4.8). Naming is a convenience; permission
/// is a boundary. Do not reach for layer precedence when the question is
/// "may I".
pub struct Registry {
    pub(crate) entries: IndexMap<ToolRef, Entry>,
    /// Short name to every reference claiming it, in registration order.
    pub(crate) shorts: IndexMap<String, Vec<ToolRef>>,
    /// User-declared short-name aliases (section 7; plan 10 writes here).
    pub(crate) aliases: IndexMap<String, ToolRef>,
    pub(crate) ledger: Mutex<Vec<LedgerEntry>>,
    /// The same decisions, in the one stream that answers "why did this
    /// happen". The in-crate ledger stays: it is the cheap, synchronous read
    /// that `resolve` and the CLI use. The audit is where a decision goes to be
    /// kept.
    pub(crate) audit: Audit,
    /// The host. Private, and no public method returns it: `dispatch` is the
    /// only way to reach a tool, which is what makes the policy check
    /// unavoidable.
    host: Arc<dyn ToolHost>,
    pub(crate) policy: Arc<dyn PolicyCheck>,
    pub(crate) interceptors: Vec<Arc<dyn ToolInterceptor>>,
    next_order: usize,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("entries", &self.entries.len())
            .field("aliases", &self.aliases.len())
            .finish_non_exhaustive()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// An empty registry with no host and an allow-all policy.
    ///
    /// Dispatching against it answers
    /// [`Outcome::Unloaded`](orrery_proto::Outcome::Unloaded); use
    /// [`Registry::with_host`] for a registry that can actually call.
    #[must_use]
    pub fn new() -> Self {
        Self::with_host(Arc::new(UnavailableHost))
    }

    /// An empty registry over a host.
    #[must_use]
    pub fn with_host(host: Arc<dyn ToolHost>) -> Self {
        Self {
            entries: IndexMap::new(),
            shorts: IndexMap::new(),
            aliases: IndexMap::new(),
            ledger: Mutex::new(Vec::new()),
            audit: orrery_audit::null(),
            host,
            policy: Arc::new(AllowAll),
            interceptors: Vec::new(),
            next_order: 0,
        }
    }

    /// Send every ledger decision to an audit stream as well.
    ///
    /// Without one, decisions are still recorded in the in-crate ledger and
    /// emitted as `tracing` events; they are simply not kept.
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// Replace the policy check every dispatch runs through.
    #[must_use]
    pub fn with_policy(mut self, policy: Arc<dyn PolicyCheck>) -> Self {
        self.policy = policy;
        self
    }

    /// Add an interceptor to the `tool.*` phases.
    pub fn intercept(&mut self, interceptor: Arc<dyn ToolInterceptor>) {
        self.interceptors.push(interceptor);
    }

    /// Register one tool under an extension at a layer.
    ///
    /// A second registration of the same fully-qualified name does not fail:
    /// the closer layer keeps the name and the loser goes to the ledger.
    pub fn register(&mut self, ext: &ExtId, layer: Layer, spec: ToolSpec) {
        let r#ref = ToolRef {
            ext: ext.clone(),
            name: spec.name.clone(),
        };

        if let Some(existing) = self.entries.get_mut(&r#ref) {
            let held = existing.layer;
            let (winner, loser) = if layer > held {
                existing.layer = layer;
                existing.spec = spec;
                (layer, held)
            } else {
                (held, layer)
            };
            self.record(LedgerEntry::Shadowed {
                r#ref,
                winner,
                loser,
            });
            return;
        }

        let order = self.next_order;
        self.next_order += 1;
        self.shorts
            .entry(spec.name.clone())
            .or_default()
            .push(r#ref.clone());
        self.entries.insert(
            r#ref.clone(),
            Entry {
                r#ref,
                layer,
                spec,
                state: ExtState::Live,
                order,
            },
        );
    }

    /// Register everything an extension contributed, tools only.
    ///
    /// The other [`ContributionKind`]s belong to somebody else's table.
    pub fn register_contributions(
        &mut self,
        ext: &ExtId,
        layer: Layer,
        contributions: &[Contribution],
    ) {
        for c in contributions
            .iter()
            .filter(|c| c.kind == ContributionKind::Tool)
        {
            self.register(ext, layer, ToolSpec::new(&c.name));
        }
    }

    /// Declare a user-level short-name alias.
    ///
    /// Section 7's "short-name aliases are the user's". Plan 10's config writes
    /// here; an alias is checked before the generated short-name table, so a
    /// user can settle an ambiguity permanently.
    pub fn alias(&mut self, alias: impl Into<String>, r#ref: ToolRef) {
        self.aliases.insert(alias.into(), r#ref);
    }

    /// Move every tool of an extension to a new lifecycle state.
    pub fn set_state(&mut self, ext: &ExtId, state: ExtState) {
        for entry in self.entries.values_mut() {
            if entry.r#ref.ext == *ext {
                entry.state = state;
            }
        }
    }

    /// The winning entry for a fully-qualified name.
    #[must_use]
    pub fn entry(&self, r#ref: &ToolRef) -> Option<&Entry> {
        self.entries.get(r#ref)
    }

    /// How many tools are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Everything the registry has decided that a person may have to explain.
    #[must_use]
    pub fn ledger(&self) -> Vec<LedgerEntry> {
        self.ledger.lock().clone()
    }

    pub(crate) fn record(&self, entry: LedgerEntry) {
        tracing::debug!(target: "orrery.load.tools", ?entry, "tool name decided");
        self.audit.append(audit_event(&entry));
        self.ledger.lock().push(entry);
    }

    /// The host, for `dispatch` and for nothing else.
    pub(crate) fn host(&self) -> &Arc<dyn ToolHost> {
        &self.host
    }
}

/// A ledger entry as the audit stream sees it.
///
/// Both shapes are the same question — a name meant more than one thing and one
/// of them won — so they share one event and differ in their candidate list.
fn audit_event(entry: &LedgerEntry) -> AuditEvent {
    match entry {
        LedgerEntry::Ambiguous {
            name,
            candidates,
            chose,
        } => AuditEvent::ToolName {
            name: name.clone(),
            candidates: candidates.iter().map(ToString::to_string).collect(),
            chose: chose.to_string(),
        },
        LedgerEntry::Shadowed {
            r#ref,
            winner,
            loser,
        } => AuditEvent::ToolName {
            name: r#ref.to_string(),
            candidates: vec![
                format!("{ref}@{winner:?}", r#ref = r#ref),
                format!("{ref}@{loser:?}", r#ref = r#ref),
            ],
            chose: format!("{ref}@{winner:?}", r#ref = r#ref),
        },
    }
}
