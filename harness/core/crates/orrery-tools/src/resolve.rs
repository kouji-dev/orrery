//! One resolver: a name the model emitted to the tool it meant.

use orrery_proto::{AgentScope, ToolRef};

use crate::registry::{Entry, ExtState, LedgerEntry, Registry};

/// How many registered names `did_you_mean` will measure a distance against.
///
/// Decision (open question 1): cap, do not index. Levenshtein over every name
/// is fine at 50 tools and silly at 5000, and a trigram index is a data
/// structure to keep in step with the name table for a suggestion nobody is
/// entitled to. Past the cap the suggestion list is simply shorter, which is a
/// worse hint and never a wrong answer.
const MAX_SUGGESTION_SCAN: usize = 512;

/// The greatest edit distance still worth offering as a suggestion.
const MAX_SUGGESTION_DISTANCE: usize = 2;

/// How many suggestions to offer.
const MAX_SUGGESTIONS: usize = 3;

/// What a call name turned out to mean.
///
/// `Ambiguous` carries what it **chose**, not just the candidates: the call
/// proceeds and the event stream records that a choice was made. A duplicate
/// tool name must not exit 1.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// Exactly one tool.
    Ok {
        /// Which one.
        r#ref: ToolRef,
    },
    /// More than one tool, resolved and reported.
    Ambiguous {
        /// Everything the name could have meant.
        candidates: Vec<ToolRef>,
        /// What it was taken to mean: the closest layer, then the earliest
        /// registration.
        chose: ToolRef,
    },
    /// No tool, and the nearest names by edit distance.
    Unknown {
        /// The name as called.
        name: String,
        /// What the caller might have meant.
        did_you_mean: Vec<String>,
    },
}

impl Resolution {
    /// The reference this resolution settled on, if any.
    #[must_use]
    pub fn r#ref(&self) -> Option<&ToolRef> {
        match self {
            Resolution::Ok { r#ref } | Resolution::Ambiguous { chose: r#ref, .. } => Some(r#ref),
            Resolution::Unknown { .. } => None,
        }
    }
}

impl Registry {
    /// Resolve a name the model emitted, within a scope.
    ///
    /// Order: a user-declared alias, then a fully-qualified `ext.name`, then the
    /// short-name table. Only tools this scope can see and whose extension is
    /// [`ExtState::Live`] are considered, so a name outside the scope is
    /// `Unknown` rather than a tool the model was never offered.
    #[must_use]
    pub fn resolve(&self, call_name: &str, scope: &AgentScope) -> Resolution {
        if let Some(r#ref) = self.aliases.get(call_name) {
            if self.is_callable(r#ref, scope) {
                return Resolution::Ok {
                    r#ref: r#ref.clone(),
                };
            }
        }

        if let Ok(r#ref) = call_name.parse::<ToolRef>() {
            if self.is_callable(&r#ref, scope) {
                return Resolution::Ok { r#ref };
            }
        }

        let mut candidates: Vec<&Entry> = self
            .shorts
            .get(call_name)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter(|r| self.is_callable(r, scope))
            .filter_map(|r| self.entries.get(r))
            .collect();

        match candidates.len() {
            0 => Resolution::Unknown {
                name: call_name.to_owned(),
                did_you_mean: self.did_you_mean(call_name, scope),
            },
            1 => Resolution::Ok {
                r#ref: candidates[0].r#ref.clone(),
            },
            _ => {
                // Closest layer first, then earliest registration. A stable
                // sort on the reverse layer keeps registration order for ties.
                candidates.sort_by_key(|e| (std::cmp::Reverse(e.layer), e.order));
                let chose = candidates[0].r#ref.clone();
                let candidates: Vec<ToolRef> =
                    candidates.iter().map(|e| e.r#ref.clone()).collect();
                self.record(LedgerEntry::Ambiguous {
                    name: call_name.to_owned(),
                    candidates: candidates.clone(),
                    chose: chose.clone(),
                });
                Resolution::Ambiguous { candidates, chose }
            }
        }
    }

    /// Whether this reference exists, is live and is inside the scope.
    pub(crate) fn is_callable(&self, r#ref: &ToolRef, scope: &AgentScope) -> bool {
        self.entries
            .get(r#ref)
            .is_some_and(|e| e.state == ExtState::Live && crate::visible::in_scope(scope, r#ref))
    }

    fn did_you_mean(&self, call_name: &str, scope: &AgentScope) -> Vec<String> {
        let mut scored: Vec<(usize, String)> = self
            .entries
            .values()
            .filter(|e| e.state == ExtState::Live && crate::visible::in_scope(scope, &e.r#ref))
            .take(MAX_SUGGESTION_SCAN)
            .flat_map(|e| [e.spec.name.clone(), e.r#ref.to_string()])
            .filter_map(|name| {
                let d = distance(call_name, &name);
                (d <= MAX_SUGGESTION_DISTANCE).then_some((d, name))
            })
            .collect();
        scored.sort();
        scored.dedup_by(|a, b| a.1 == b.1);
        scored.truncate(MAX_SUGGESTIONS);
        scored.into_iter().map(|(_, name)| name).collect()
    }
}

/// Levenshtein distance, two rows at a time.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != *cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::distance;

    #[test]
    fn distance_is_symmetric_and_zero_on_equal() {
        assert_eq!(distance("search", "search"), 0);
        assert_eq!(distance("serch", "search"), 1);
        assert_eq!(distance("search", "serch"), 1);
        assert_eq!(distance("", "abc"), 3);
    }
}
