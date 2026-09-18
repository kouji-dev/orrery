//! Step inputs and step conditions.
//!
//! Deliberately **not** a language. A step's input is either a literal or a
//! reference to an earlier step's typed return, and a condition compares
//! declared values. Neither can call out, loop, or name anything the loader did
//! not already typecheck — which is what makes a workflow a graph that can be
//! checked before it runs rather than a program that has to be run to be
//! understood. Evaluation and the load-time typecheck are plan 11.

use serde::{Deserialize, Serialize};

/// A value, or a pointer to one.
///
/// # Variant order is load-bearing
///
/// `#[serde(untagged)]` tries variants **in declaration order**, and
/// [`Expr::Literal`] accepts anything — so `Ref` is declared first. Swap them
/// and `{"ref":"step1"}` silently parses as a literal map, which typechecks,
/// serialises and does the wrong thing at run time.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Expr {
    /// The output of an earlier step, optionally addressed into.
    Ref {
        /// The step's name.
        r#ref: String,
        /// A path of keys into that step's return value.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<Vec<String>>,
    },
    /// A value written down where it is used.
    Literal(serde_json::Value),
}

/// How two [`Expr`]s are compared.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CmpOp {
    /// Equal.
    Eq,
    /// Not equal.
    Ne,
    /// Less than.
    Lt,
    /// Less than or equal.
    Le,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Ge,
    /// The left-hand side is one of the right-hand side's members.
    In,
    /// The left-hand side contains the right-hand side.
    Contains,
}

/// A condition on declared values.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Predicate {
    /// One comparison.
    Cmp {
        /// The left-hand side.
        lhs: Expr,
        /// The comparison.
        op: CmpOp,
        /// The right-hand side.
        rhs: Expr,
    },
    /// Every one of these. An empty `all` is true.
    All(Vec<Predicate>),
    /// Any one of these. An empty `any` is false.
    Any(Vec<Predicate>),
    /// The opposite of this one.
    Not(Box<Predicate>),
}
