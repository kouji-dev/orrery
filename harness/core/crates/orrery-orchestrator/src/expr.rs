//! Evaluating an `Expr` and a `Predicate`. Data in, data out.
//!
//! # It cannot call out
//!
//! [`eval`] is a plain `fn` over an [`Env`], and an `Env` is a map of JSON
//! values that derives `Serialize`. There is no handle in the signature, no
//! `async`, no `&dyn` anything: a step input cannot read a file, reach a host or
//! ask a model, and that is a property of the types rather than a convention.
//! `expr::cannot_call_out` pins the signature itself.

use std::collections::BTreeMap;

use orrery_proto::{CmpOp, Expr, Predicate};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What earlier steps returned, by name.
///
/// Serialisable, which is the point: a handle cannot live in here.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Env {
    #[serde(flatten)]
    values: BTreeMap<String, Value>,
}

impl Env {
    /// An empty environment.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record what a step returned.
    pub fn bind(&mut self, step: impl Into<String>, value: Value) {
        self.values.insert(step.into(), value);
    }

    /// What a step returned, if it has run.
    #[must_use]
    pub fn get(&self, step: &str) -> Option<&Value> {
        self.values.get(step)
    }

    /// Whether a step has run.
    #[must_use]
    pub fn has(&self, step: &str) -> bool {
        self.values.contains_key(step)
    }

    /// Every step that has run, in name order.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.values.keys().map(String::as_str).collect()
    }
}

/// An expression that cannot be evaluated.
///
/// Every variant is something the load-time typecheck should already have
/// caught; they exist because a `Workflow` can also be built in code, and
/// because an `Any`-shaped return is unchecked by definition.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExprError {
    /// A `ref` to a step that has not run.
    #[error("`{step}` has not run: the steps that have are {}", available.join(", "))]
    NoSuchStep {
        /// The name that was referenced.
        step: String,
        /// What could have been meant.
        available: Vec<String>,
    },
    /// A path into a value that does not have it.
    #[error("`{step}` returned no `{key}` (the path was {})", path.join("."))]
    NoSuchKey {
        /// Which step.
        step: String,
        /// The whole path.
        path: Vec<String>,
        /// The key that was missing.
        key: String,
    },
    /// Two values that cannot be compared with this operator.
    #[error("`{op}` does not compare {lhs} with {rhs}")]
    NotComparable {
        /// The operator, as written.
        op: String,
        /// What was on the left.
        lhs: String,
        /// What was on the right.
        rhs: String,
    },
}

/// Evaluate one expression.
///
/// **Sync, total and pure.** No I/O is reachable from here.
///
/// # Errors
///
/// [`ExprError`] when a reference names a step that has not run or a path the
/// value does not have.
pub fn eval(expr: &Expr, env: &Env) -> Result<Value, ExprError> {
    match expr {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Ref { r#ref, path } => {
            let mut value = env.get(r#ref).ok_or_else(|| ExprError::NoSuchStep {
                step: r#ref.clone(),
                available: env.names().iter().map(|s| (*s).to_owned()).collect(),
            })?;
            let path = path.clone().unwrap_or_default();
            for key in &path {
                value = value.get(key).ok_or_else(|| ExprError::NoSuchKey {
                    step: r#ref.clone(),
                    path: path.clone(),
                    key: key.clone(),
                })?;
            }
            Ok(value.clone())
        }
        _ => Ok(Value::Null),
    }
}

/// Evaluate one predicate.
///
/// # Errors
///
/// [`ExprError`] from either side, or when the two sides cannot be compared.
pub fn holds(predicate: &Predicate, env: &Env) -> Result<bool, ExprError> {
    match predicate {
        Predicate::Cmp { lhs, op, rhs } => {
            let lhs = eval(lhs, env)?;
            let rhs = eval(rhs, env)?;
            compare(&lhs, *op, &rhs)
        }
        // An empty `all` is true and an empty `any` is false, exactly as
        // `orrery-proto` documents them.
        Predicate::All(inner) => {
            for p in inner {
                if !holds(p, env)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Predicate::Any(inner) => {
            for p in inner {
                if holds(p, env)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Predicate::Not(inner) => Ok(!holds(inner, env)?),
        _ => Ok(false),
    }
}

fn compare(lhs: &Value, op: CmpOp, rhs: &Value) -> Result<bool, ExprError> {
    let not_comparable = || ExprError::NotComparable {
        op: format!("{op:?}").to_lowercase(),
        lhs: lhs.to_string(),
        rhs: rhs.to_string(),
    };
    Ok(match op {
        CmpOp::Eq => lhs == rhs,
        CmpOp::Ne => lhs != rhs,
        CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => {
            let (a, b) = match (lhs.as_f64(), rhs.as_f64()) {
                (Some(a), Some(b)) => (a, b),
                _ => return Err(not_comparable()),
            };
            match op {
                CmpOp::Lt => a < b,
                CmpOp::Le => a <= b,
                CmpOp::Gt => a > b,
                _ => a >= b,
            }
        }
        CmpOp::In => match rhs {
            Value::Array(items) => items.contains(lhs),
            Value::String(haystack) => lhs.as_str().is_some_and(|needle| haystack.contains(needle)),
            _ => return Err(not_comparable()),
        },
        CmpOp::Contains => match lhs {
            Value::Array(items) => items.contains(rhs),
            Value::String(haystack) => rhs.as_str().is_some_and(|needle| haystack.contains(needle)),
            Value::Object(map) => rhs.as_str().is_some_and(|key| map.contains_key(key)),
            _ => return Err(not_comparable()),
        },
        _ => return Err(not_comparable()),
    })
}
