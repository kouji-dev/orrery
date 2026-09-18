//! Translation #5 · the whole workflow dataflow is typechecked **at load**.
//!
//! Step *N* may only `ref` steps `< N`, and each ref's path must typecheck
//! against the target's declared `returns`. A workflow must not fail mid-run on
//! a type error after paying for three model calls — which is exactly what a
//! run-time check would do, because the type error is on the step nobody
//! reaches until the expensive ones have finished.
//!
//! Every error names the file and the step.

use std::collections::BTreeMap;

use orrery_proto::{Expr, Predicate};
use serde::{Deserialize, Serialize};

use crate::step::{Catalogue, NamedStep, Step, TypeShape};

/// A workflow, as loaded.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    /// What it is called.
    pub name: String,
    /// Where it was written, for every error message.
    #[serde(default)]
    pub file: String,
    /// What the whole workflow may spend, across steps.
    #[serde(default)]
    pub budget: orrery_proto::Budget,
    /// Its steps, in order.
    #[serde(default, rename = "step")]
    pub steps: Vec<NamedStep>,
}

impl Workflow {
    /// Read one from TOML. This does **not** typecheck it; call [`check`].
    ///
    /// # Errors
    ///
    /// [`LoadError::Syntax`] when the file is not TOML or not this shape of
    /// TOML — which includes a `loop` step with no `until`, since the field has
    /// no serde default.
    pub fn from_toml_str(text: &str, file: &str) -> Result<Workflow, LoadError> {
        let mut wf: Workflow = toml::from_str(text).map_err(|e| LoadError::Syntax {
            file: file.to_owned(),
            message: e.message().to_owned(),
        })?;
        wf.file = file.to_owned();
        Ok(wf)
    }

    /// Load and typecheck in one go, which is what a session does.
    ///
    /// # Errors
    ///
    /// Anything [`from_toml_str`](Workflow::from_toml_str) or [`check`] returns.
    pub fn load(text: &str, file: &str, catalogue: &Catalogue) -> Result<Workflow, LoadError> {
        let wf = Workflow::from_toml_str(text, file)?;
        check(&wf, catalogue)?;
        Ok(wf)
    }
}

/// A workflow that does not load.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    /// It is not TOML, or not this shape of it.
    #[error("{file}: {message}")]
    Syntax {
        /// Which file.
        file: String,
        /// What the parser said.
        message: String,
    },
    /// Two steps with the same name: a `ref` could not say which.
    #[error("{file}: `{step}` is declared twice, so a `ref` to it is ambiguous")]
    DuplicateStep {
        /// Which file.
        file: String,
        /// Which name.
        step: String,
    },
    /// A step referencing a step that has not run yet.
    #[error(
        "{file}: step `{step}` refers to `{target}`, which runs later — \
         a step may only refer to steps before it"
    )]
    ForwardRef {
        /// Which file.
        file: String,
        /// The step that does the referring.
        step: String,
        /// What it refers to.
        target: String,
    },
    /// A step referencing a step that does not exist at all.
    #[error(
        "{file}: step `{step}` refers to `{target}`, and no step by that name is declared{}",
        if known.is_empty() { String::new() } else { format!(": there is {}", known.join(", ")) }
    )]
    NoSuchStep {
        /// Which file.
        file: String,
        /// The step that does the referring.
        step: String,
        /// What it refers to.
        target: String,
        /// The names it could have meant.
        known: Vec<String>,
    },
    /// A path into a return value that does not have it.
    ///
    /// **Boxed**, because it is the one wide variant and a `Result<_,
    /// LoadError>` is what every load returns.
    #[error(transparent)]
    BadPath(Box<BadPath>),
    /// A `Loop` whose cap is zero: that is not a loop, it is a step that never
    /// runs.
    #[error("{file}: step `{step}` is a loop with `max_iterations = 0`")]
    ZeroCap {
        /// Which file.
        file: String,
        /// Which step.
        step: String,
    },
}

/// The detail of a [`LoadError::BadPath`].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error(
    "{file}: step `{step}` reads `{target}.{}`, and `{target}` returns {shape}{}",
    path.join("."),
    if keys.is_empty() { String::new() } else { format!(" with {}", keys.join(", ")) }
)]
pub struct BadPath {
    /// Which file.
    pub file: String,
    /// The step that does the reading.
    pub step: String,
    /// What it reads from.
    pub target: String,
    /// The path it reads.
    pub path: Vec<String>,
    /// What the target actually returns.
    pub shape: String,
    /// The keys it does have.
    pub keys: Vec<String>,
}

impl LoadError {
    /// Which file the problem is in.
    #[must_use]
    pub fn file(&self) -> &str {
        match self {
            LoadError::Syntax { file, .. }
            | LoadError::DuplicateStep { file, .. }
            | LoadError::ForwardRef { file, .. }
            | LoadError::NoSuchStep { file, .. }
            | LoadError::ZeroCap { file, .. } => file,
            LoadError::BadPath(bad) => &bad.file,
        }
    }

    /// Which step, when the problem is in one.
    #[must_use]
    pub fn step(&self) -> Option<&str> {
        match self {
            LoadError::Syntax { .. } => None,
            LoadError::DuplicateStep { step, .. }
            | LoadError::ForwardRef { step, .. }
            | LoadError::NoSuchStep { step, .. }
            | LoadError::ZeroCap { step, .. } => Some(step),
            LoadError::BadPath(bad) => Some(&bad.step),
        }
    }
}

/// Typecheck the whole dataflow.
///
/// # Errors
///
/// [`LoadError`] on the first problem, naming the file and the step. Nothing
/// has run, and nothing has been paid for.
pub fn check(workflow: &Workflow, catalogue: &Catalogue) -> Result<(), LoadError> {
    let mut seen: BTreeMap<String, TypeShape> = BTreeMap::new();
    let mut all: Vec<String> = Vec::new();
    collect_names(&workflow.steps, &mut all);
    walk(workflow, &workflow.steps, catalogue, &mut seen, &all)
}

fn collect_names(steps: &[NamedStep], out: &mut Vec<String>) {
    for step in steps {
        out.push(step.name.clone());
        match &step.step {
            Step::Parallel { steps, .. } => collect_names(steps, out),
            Step::Loop { body, .. } => collect_names(body, out),
            _ => {}
        }
    }
}

/// Walk in declaration order, binding each step's declared return as it goes.
///
/// `seen` is what may be referred to; `all` is every name there is, which is
/// what distinguishes "runs later" from "does not exist".
fn walk(
    workflow: &Workflow,
    steps: &[NamedStep],
    catalogue: &Catalogue,
    seen: &mut BTreeMap<String, TypeShape>,
    all: &[String],
) -> Result<(), LoadError> {
    for step in steps {
        if seen.contains_key(&step.name) {
            return Err(LoadError::DuplicateStep {
                file: workflow.file.clone(),
                step: step.name.clone(),
            });
        }
        match &step.step {
            Step::Agent { input, .. } | Step::Tool { input, .. } => {
                check_expr(workflow, &step.name, input, seen, all)?;
            }
            Step::Gate { check, .. } => {
                check_predicate(workflow, &step.name, check, seen, all)?;
            }
            Step::Parallel {
                steps: branches, ..
            } => {
                // A branch may read anything already bound, and its siblings are
                // running at the same time, so it may not read them. Bind them
                // all afterwards.
                for branch in branches {
                    let mut branch_scope = seen.clone();
                    walk(
                        workflow,
                        std::slice::from_ref(branch),
                        catalogue,
                        &mut branch_scope,
                        all,
                    )?;
                }
                for branch in branches {
                    seen.insert(branch.name.clone(), catalogue.returns_of(branch));
                }
            }
            Step::Loop {
                body,
                until,
                max_iterations,
            } => {
                if *max_iterations == 0 {
                    return Err(LoadError::ZeroCap {
                        file: workflow.file.clone(),
                        step: step.name.clone(),
                    });
                }
                // The body runs before the predicate is first consulted, so the
                // predicate may read the body's own steps.
                walk(workflow, body, catalogue, seen, all)?;
                check_predicate(workflow, &step.name, until, seen, all)?;
            }
        }
        seen.insert(step.name.clone(), catalogue.returns_of(step));
    }
    Ok(())
}

fn check_predicate(
    workflow: &Workflow,
    step: &str,
    predicate: &Predicate,
    seen: &BTreeMap<String, TypeShape>,
    all: &[String],
) -> Result<(), LoadError> {
    match predicate {
        Predicate::Cmp { lhs, rhs, .. } => {
            check_expr(workflow, step, lhs, seen, all)?;
            check_expr(workflow, step, rhs, seen, all)
        }
        Predicate::All(inner) | Predicate::Any(inner) => {
            for p in inner {
                check_predicate(workflow, step, p, seen, all)?;
            }
            Ok(())
        }
        Predicate::Not(inner) => check_predicate(workflow, step, inner, seen, all),
        _ => Ok(()),
    }
}

fn check_expr(
    workflow: &Workflow,
    step: &str,
    expr: &Expr,
    seen: &BTreeMap<String, TypeShape>,
    all: &[String],
) -> Result<(), LoadError> {
    let Expr::Ref { r#ref, path } = expr else {
        return Ok(());
    };
    let Some(shape) = seen.get(r#ref) else {
        // Declared, but later: a forward reference. Otherwise it simply is not
        // there. Two different mistakes, two different messages.
        return Err(if all.iter().any(|n| n == r#ref) {
            LoadError::ForwardRef {
                file: workflow.file.clone(),
                step: step.to_owned(),
                target: r#ref.clone(),
            }
        } else {
            LoadError::NoSuchStep {
                file: workflow.file.clone(),
                step: step.to_owned(),
                target: r#ref.clone(),
                known: seen.keys().cloned().collect(),
            }
        });
    };

    let mut at = shape;
    for (depth, key) in path.clone().unwrap_or_default().iter().enumerate() {
        match at.field(key) {
            Some(next) => at = next,
            None => {
                return Err(LoadError::BadPath(Box::new(BadPath {
                    file: workflow.file.clone(),
                    step: step.to_owned(),
                    target: r#ref.clone(),
                    path: path.clone().unwrap_or_default()[..=depth].to_vec(),
                    shape: at.word().to_owned(),
                    keys: at.keys(),
                })));
            }
        }
    }
    Ok(())
}
