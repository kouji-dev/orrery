//! `permissions explain`: which rule, in which layer, in which file, on which
//! line — and what it decided.
//!
//! The CLI surface is plan 17. This is the answer it prints.

use std::path::PathBuf;

use orrery_proto::{Layer, RuleId, Subject};

use crate::engine::Verdict;
use crate::rule::{Rule, RuleList};

/// One rule, as an explanation names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleMatch {
    /// So it can be looked up, and so an audit record and an explanation agree.
    pub id: RuleId,
    /// Exactly as written.
    pub text: String,
    /// Which list it was in.
    pub list: RuleList,
    /// Which layer contributed it.
    pub layer: Layer,
    /// The file it was written in.
    pub file: PathBuf,
    /// The line, 1-based.
    pub line: u32,
    /// Whether it matched the call being explained.
    pub matched: bool,
}

impl RuleMatch {
    pub(crate) fn of(rule: &Rule, matched: bool) -> Self {
        Self {
            id: rule.id,
            text: rule.text.clone(),
            list: rule.list,
            layer: rule.layer,
            file: rule.source.file.clone(),
            line: rule.source.line,
            matched,
        }
    }
}

impl std::fmt::Display for RuleMatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} `{}` [{:?}] {}:{}",
            self.list.keyword(),
            self.text,
            self.layer,
            self.file.display(),
            self.line
        )
    }
}

/// What a dry run of one call says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Explanation {
    /// Who was asking.
    pub subject: Subject,
    /// What they asked for, in rule-grammar form.
    pub request: String,
    /// What would happen.
    pub verdict: Verdict,
    /// The rule that would decide it, when one would.
    pub rule: Option<RuleMatch>,
    /// Every rule about this aspect and subject that was looked at.
    pub considered: Vec<RuleMatch>,
    /// The verdict in words.
    pub reason: String,
}

impl std::fmt::Display for Explanation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{} for {}: {:?} — {}",
            self.request, self.subject, self.verdict, self.reason
        )?;
        match &self.rule {
            Some(rule) => writeln!(f, "  by {rule}")?,
            None => writeln!(f, "  by no rule (the default is to refuse)")?,
        }
        for other in self.considered.iter().filter(|r| !r.matched) {
            writeln!(f, "  considered {other}")?;
        }
        Ok(())
    }
}
