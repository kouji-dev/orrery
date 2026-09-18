//! `materialise`: rows in, messages out.
//!
//! Pure and synchronous, and therefore testable without a database. The backend
//! hands it rows; it hands back messages. Nothing here touches a connection, a
//! lock or a clock.
//!
//! # Order of operations, and why it is this one
//!
//! 1. **Oldest first, never reordered.** Provider prompt caching keys on a
//!    prefix; reordering history invalidates every cache in the session.
//! 2. **The highest watermark wins.** Rows at or below it are replaced by the
//!    [`Summary`](crate::turn::TurnKind::Summary) that covers them — replaced
//!    in the *view*, not in the store. The rows are still there.
//! 3. **Elision takes from the middle.** Never the head, because the head is
//!    the cached prefix, and never the most recent turn, because dropping what
//!    the model just did is how a loop starts. Only when the most recent turn
//!    alone still will not fit does the head yield too.

use orrery_proto::{ContentBlock, Message, MessageRole, Seq, TokenBudget, TurnId};

use crate::turn::{TurnKind, TurnRow};

/// How many tokens a slice of messages will cost.
///
/// An argument to `materialise` rather than a field on the store: the store
/// must not know which provider is bound, and the same branch materialises
/// differently for two models. The real implementations are plan 03's.
pub trait TokenCounter: Send + Sync {
    /// Count them.
    fn count(&self, messages: &[Message]) -> u64;
}

impl<T: TokenCounter + ?Sized> TokenCounter for &T {
    fn count(&self, messages: &[Message]) -> u64 {
        (**self).count(messages)
    }
}

/// Characters over four. A crude counter, good enough for tests and for a
/// provider that reports no tokeniser at all.
#[derive(Copy, Clone, Debug, Default)]
pub struct CharsOverFour;

impl TokenCounter for CharsOverFour {
    fn count(&self, messages: &[Message]) -> u64 {
        let chars: usize = messages
            .iter()
            .flat_map(|m| m.content.iter())
            .map(|b| match b {
                ContentBlock::Text { text } | ContentBlock::Thinking { text } => {
                    text.chars().count()
                }
                ContentBlock::Image { data, .. } => data.len(),
                ContentBlock::ToolUse { input, .. } => input.to_string().chars().count(),
                ContentBlock::ToolResult { outcome, .. } => serde_json::to_string(outcome)
                    .map(|s| s.chars().count())
                    .unwrap_or(0),
                _ => 0,
            })
            .sum();
        (chars / 4) as u64
    }
}

/// The messages, and what it cost to get them down to size.
#[derive(Clone, Debug, PartialEq)]
pub struct Materialised {
    /// Oldest first.
    pub messages: Vec<Message>,
    /// What the counter made of them.
    pub tokens_estimated: u64,
    /// What the budget cut. Exposed because the caller may want to
    /// [`compact`](crate::SessionStore::compact) instead of eliding again next
    /// turn — that is plan 05's compaction trigger.
    pub elided: Vec<TurnId>,
    /// The compaction this view sits on, if any.
    pub watermark: Option<Seq>,
}

/// Walk the rows, apply the watermark, fit the budget.
///
/// `rows` is one branch's ancestry, oldest first; out-of-order input is sorted
/// rather than rejected, because a backend's `ORDER BY` is not this function's
/// problem.
#[must_use]
pub fn materialise(
    rows: &[TurnRow],
    watermark: Option<Seq>,
    budget: TokenBudget,
    counter: &dyn TokenCounter,
) -> Materialised {
    let mut ordered: Vec<&TurnRow> = rows.iter().collect();
    ordered.sort_by_key(|r| r.seq);

    // 2 - the watermark. A summary that covers the elided prefix is hoisted to
    // the head; everything at or below the watermark goes.
    let mut head: Vec<&TurnRow> = Vec::new();
    let mut tail: Vec<&TurnRow> = Vec::new();
    match watermark {
        None => tail.extend(ordered),
        Some(mark) => {
            for row in ordered {
                match &row.kind {
                    // The summary this watermark points at stands in for the
                    // prefix, so it is hoisted to the head.
                    TurnKind::Summary { covers, .. } if covers.1 == mark => head.push(row),
                    // A summary from an earlier, superseded compaction. Still a
                    // row in the store; just not in this view.
                    TurnKind::Summary { covers, .. } if covers.1 < mark => {}
                    _ if row.seq <= mark => {}
                    _ => tail.push(row),
                }
            }
        }
    }
    head.extend(tail);
    let turns = head;

    let rendered: Vec<Message> = turns.iter().map(|r| render(r)).collect();

    // 3 - fit the budget.
    let available = budget.available();
    let mut keep: Vec<bool> = vec![true; turns.len()];
    let mut elided: Vec<TurnId> = Vec::new();

    let collect = |keep: &[bool]| -> Vec<Message> {
        rendered
            .iter()
            .zip(keep)
            .filter(|(_, k)| **k)
            .map(|(m, _)| m.clone())
            .collect()
    };

    let mut messages = collect(&keep);
    if !turns.is_empty() {
        let last = turns.len() - 1;
        // The middle, oldest first.
        let mut i = 1;
        while counter.count(&messages) > available && i < last {
            keep[i] = false;
            elided.push(turns[i].id);
            messages = collect(&keep);
            i += 1;
        }
        // Only then the head, and only if there is something after it.
        if counter.count(&messages) > available && last > 0 && keep[0] {
            keep[0] = false;
            elided.push(turns[0].id);
            messages = collect(&keep);
        }
    }

    elided.sort_unstable();
    let tokens_estimated = counter.count(&messages);
    Materialised {
        messages,
        tokens_estimated,
        elided,
        watermark,
    }
}

/// One row, one message. Kept one-to-one so that eliding a turn elides exactly
/// one message and the caller's `elided` list means what it says.
fn render(row: &TurnRow) -> Message {
    match &row.kind {
        TurnKind::User { input } => {
            let mut content = vec![ContentBlock::Text {
                text: input.text.clone(),
            }];
            content.extend(input.attachments.iter().cloned());
            Message {
                role: MessageRole::User,
                content,
            }
        }
        TurnKind::Assistant { content, .. } => Message {
            role: MessageRole::Assistant,
            content: content.clone(),
        },
        TurnKind::ToolResult { call, outcome, .. } => Message {
            role: MessageRole::User,
            content: vec![ContentBlock::ToolResult {
                call: *call,
                outcome: outcome.clone(),
            }],
        },
        // A join reads back to the model as prose, because the child's own
        // turns are on the child's branch and the parent never saw them.
        TurnKind::BranchResult { child, outcome } => Message {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: format!("[branch {child} {}] {}", outcome.tag(), describe(outcome)),
            }],
        },
        // A summary stands in the conversation where the turns it covers stood,
        // so it is a conversational message and not a second system prompt:
        // assembling the system prompt is plan 05's job, not the store's.
        TurnKind::Summary { text, .. } => Message {
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: text.clone() }],
        },
        TurnKind::Recalled { provider, entries } => Message {
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: entries
                    .iter()
                    .map(|e| format!("[{provider}:{}] {}", e.key, e.text))
                    .collect::<Vec<_>>()
                    .join("\n"),
            }],
        },
    }
}

fn describe(outcome: &crate::turn::BranchOutcome) -> String {
    use crate::turn::BranchOutcome::{Cancelled, Completed, Failed};
    match outcome {
        Completed { summary } => summary.clone(),
        Failed { code, message } => format!("{code}: {message}"),
        Cancelled { reason } => format!("cancelled ({reason:?})"),
    }
}
