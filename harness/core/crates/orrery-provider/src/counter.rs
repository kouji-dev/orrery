//! Token counting, and how honest it is about itself.

use orrery_proto::{ContentBlock, Message};

/// Counts tokens for one provider family.
pub trait TokenCounter: Send + Sync {
    /// The whole conversation, including whatever per-message framing the
    /// provider adds.
    fn count_messages(&self, messages: &[Message]) -> u64;
    /// One run of text.
    fn count_text(&self, text: &str) -> u64;
    /// True when this is a real tokenizer rather than an estimate.
    ///
    /// Compaction keeps a bigger safety margin when false (translation #6).
    fn is_exact(&self) -> bool {
        false
    }
}

/// Characters over four, plus a fixed per-block overhead.
///
/// Phase 1 ships this for every provider. An exact tokenizer is a large
/// dependency per provider family and the margin covers us — see the plan's
/// open question 1.
#[derive(Copy, Clone, Debug)]
pub struct HeuristicCounter {
    chars_per_token: u64,
    per_block_overhead: u64,
    per_message_overhead: u64,
}

impl Default for HeuristicCounter {
    fn default() -> Self {
        Self::new()
    }
}

impl HeuristicCounter {
    /// The default estimate: four characters a token, four tokens of framing a
    /// block, four more a message.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chars_per_token: 4,
            per_block_overhead: 4,
            per_message_overhead: 4,
        }
    }

    /// Round up: half a token still occupies one.
    const fn chars(&self, n: u64) -> u64 {
        n.div_ceil(self.chars_per_token)
    }

    fn block(&self, block: &ContentBlock) -> u64 {
        let body = match block {
            ContentBlock::Text { text } | ContentBlock::Thinking { text } => {
                self.chars(text.len() as u64)
            }
            // Base64 is not prose; it is billed per tile, not per character.
            // A flat estimate beats a character count that is wrong by 100x.
            ContentBlock::Image { data, .. } => 1_500u64.min(self.chars(data.len() as u64)),
            ContentBlock::ToolUse { name, input, .. } => {
                self.chars(name.len() as u64) + self.chars(json_len(input))
            }
            ContentBlock::ToolResult { outcome, .. } => self.chars(serde_len(outcome)),
            // `ContentBlock` is `#[non_exhaustive]`: an unknown block still
            // costs *something*, so the estimate never shrinks below reality.
            _ => 0,
        };
        body.saturating_add(self.per_block_overhead)
    }
}

fn json_len(v: &serde_json::Value) -> u64 {
    serde_json::to_string(v).map_or(0, |s| s.len() as u64)
}

fn serde_len<T: serde::Serialize>(v: &T) -> u64 {
    serde_json::to_string(v).map_or(0, |s| s.len() as u64)
}

impl TokenCounter for HeuristicCounter {
    fn count_messages(&self, messages: &[Message]) -> u64 {
        messages.iter().fold(0u64, |acc, m| {
            let blocks = m
                .content
                .iter()
                .fold(0u64, |a, b| a.saturating_add(self.block(b)));
            acc.saturating_add(blocks)
                .saturating_add(self.per_message_overhead)
        })
    }

    fn count_text(&self, text: &str) -> u64 {
        self.chars(text.len() as u64)
    }

    fn is_exact(&self) -> bool {
        false
    }
}
