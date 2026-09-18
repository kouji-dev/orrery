//! Resolved configuration, into the kernel.
//!
//! # The root cause this module removes
//!
//! Five `TODO(plan-10)` markers — a hardcoded [`PriceTable`], a hardcoded
//! [`RetryPolicy`], a hand-picked [`ProviderChoice`], a kernel-shaped
//! [`ResolvedConfig`] built by hand, and a `ResolvedManifest` with nothing to
//! fill it — all said the same thing in different words: `orrery-harness` did
//! not depend on `orrery-config`, so there was no path from a file on disk to
//! the running kernel. The consequence was measurable rather than theoretical:
//! `maxUsd` was **inert in every build**, because nothing ever gave the kernel a
//! price table and an unpriced model has no money ceiling.
//!
//! This module is that path, and it is the only one: the kernel still takes
//! plain values and still knows nothing about layers, profiles or trust.
//!
//! # The profile is an overlay
//!
//! Every key here is read twice — once under `profile.<name>.`, once bare — and
//! the profile wins. That is the same rule `config explain` prints, so what the
//! command says is in force and what the kernel runs on cannot disagree.
//!
//! # Prices are per model, and absent means absent
//!
//! ```toml
//! [budget]
//! maxUsd = 2.50          # or maxMicroUsd = 2_500_000
//!
//! [prices."claude-sonnet-4-5"]
//! inputPerMillion = 3_000_000    # micro-USD per million input tokens
//! outputPerMillion = 15_000_000
//! ```
//!
//! A model nobody priced stays unpriced. A price table that guessed would make
//! a budget report "under budget" for a session that spent a hundred dollars,
//! which is worse than a ceiling that does not fire.

use orrery_config::provenance::Provenanced;
use orrery_kernel::{KernelConfig, PriceTable, RetryPolicy};
use orrery_proto::Budget;

/// One key, read through the profile overlay first and the bare key second.
struct Keys<'a> {
    values: &'a Provenanced,
    prefix: Option<String>,
}

impl<'a> Keys<'a> {
    fn new(values: &'a Provenanced, profile: &str) -> Self {
        let prefix = if profile.is_empty() {
            None
        } else {
            Some(format!("profile.{profile}."))
        };
        Self { values, prefix }
    }

    /// Every spelling of one key, closest first.
    fn candidates(&self, key: &str) -> Vec<String> {
        match &self.prefix {
            Some(prefix) => vec![format!("{prefix}{key}"), key.to_owned()],
            None => vec![key.to_owned()],
        }
    }

    fn str(&self, key: &str) -> Option<&'a str> {
        self.candidates(key)
            .into_iter()
            .find_map(|k| self.values.str(&k))
    }

    fn int(&self, key: &str) -> Option<i64> {
        self.candidates(key)
            .into_iter()
            .find_map(|k| self.values.int(&k))
    }

    fn bool(&self, key: &str) -> Option<bool> {
        self.candidates(key)
            .into_iter()
            .find_map(|k| self.values.bool(&k))
    }

    /// A money amount, written either as micro-USD or as plain dollars.
    ///
    /// `maxUsd = 2.5` is what a person writes; `maxMicroUsd` is what the kernel
    /// counts in. Both are read, the explicit micro amount wins, and a negative
    /// number is treated as absent rather than as a ceiling of zero — "stop
    /// immediately" is not what anybody means by a negative budget.
    fn micro_usd(&self, key: &str) -> Option<u64> {
        if let Some(micro) = self.int(&format!("{key}MicroUsd")) {
            return u64::try_from(micro).ok();
        }
        let dollars = self
            .candidates(&format!("{key}Usd"))
            .into_iter()
            .find_map(|k| self.values.winner(&k))?;
        let amount = dollars
            .value
            .as_float()
            .or_else(|| dollars.value.as_integer().map(|n| n as f64))?;
        if amount < 0.0 {
            return None;
        }
        Some((amount * 1_000_000.0) as u64)
    }

    fn u32(&self, key: &str) -> Option<u32> {
        self.int(key).and_then(|n| u32::try_from(n).ok())
    }

    fn u64(&self, key: &str) -> Option<u64> {
        self.int(key).and_then(|n| u64::try_from(n).ok())
    }
}

/// Fold a resolved configuration into the shape the kernel runs on.
///
/// `base` is what a build already decided — the model id the provider actually
/// answers to, most of all — and every field a file does not mention is left
/// exactly as it was. A config that says nothing changes nothing, which is what
/// makes turning this on safe for a workspace with no `.orrery` at all.
#[must_use]
pub fn kernel_config(values: &Provenanced, profile: &str, base: KernelConfig) -> KernelConfig {
    let keys = Keys::new(values, profile);
    let mut out = base;

    if let Some(model) = keys.str("model") {
        out.model = model.to_owned();
    }
    if let Some(prompt) = keys.str("systemPrompt") {
        out.system_prompt = prompt.to_owned();
    }

    out.budget = Budget {
        max_turns: keys.u32("budget.maxTurns").unwrap_or(out.budget.max_turns),
        max_tokens: keys.u64("budget.maxTokens").unwrap_or(out.budget.max_tokens),
        wall_clock_ms: keys
            .u64("budget.wallClockMs")
            .unwrap_or(out.budget.wall_clock_ms),
        max_micro_usd: keys.micro_usd("budget.max").or(out.budget.max_micro_usd),
    };

    out.retry = RetryPolicy {
        max_attempts: keys
            .u32("retry.maxAttempts")
            .unwrap_or(out.retry.max_attempts),
        base_ms: keys.u64("retry.baseMs").unwrap_or(out.retry.base_ms),
        max_ms: keys.u64("retry.maxMs").unwrap_or(out.retry.max_ms),
        jitter: keys.bool("retry.jitter").unwrap_or(out.retry.jitter),
    };

    if let Some(n) = keys.u64("maxOutputTokens") {
        out.max_output_tokens = n;
    }
    if let Some(n) = keys.u64("contextReserveTokens") {
        out.context_reserve_tokens = n;
    }

    let prices = price_table(values, profile);
    if !prices.is_empty() {
        out.prices = prices;
    }
    out
}

/// Every `[prices.<model>]` table in force, profile overlay included.
///
/// Read off the key list rather than deserialised, so that a model id with a
/// dot in it — every Anthropic id has one — still resolves: the model name is
/// whatever sits between `prices.` and the final field.
#[must_use]
pub fn price_table(values: &Provenanced, profile: &str) -> PriceTable {
    let mut table = PriceTable::empty();
    let mut roots = Vec::new();
    if !profile.is_empty() {
        roots.push(format!("profile.{profile}.prices"));
    }
    roots.push("prices".to_owned());

    let mut seen: Vec<String> = Vec::new();
    for root in roots {
        let dotted = format!("{root}.");
        for key in values.keys_under(&root) {
            let Some(rest) = key.strip_prefix(&dotted) else {
                continue;
            };
            let Some((model, field)) = rest.rsplit_once('.') else {
                continue;
            };
            if field != "inputPerMillion" || seen.iter().any(|m| m == model) {
                continue;
            }
            let input = values.int(key).and_then(|n| u64::try_from(n).ok());
            let output = values
                .int(&format!("{dotted}{model}.outputPerMillion"))
                .and_then(|n| u64::try_from(n).ok());
            let (Some(input), Some(output)) = (input, output) else {
                continue;
            };
            // A quoted key keeps its quotes in the dotted path; the model id is
            // what is inside them.
            let model = model.trim_matches('"').to_owned();
            table = table.with(model.clone(), input, output);
            seen.push(model);
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use orrery_config::layer::LayerFile;
    use orrery_proto::{Layer, Usage};

    fn values(text: &str) -> Provenanced {
        orrery_config::merge::merge(&[LayerFile::new(Layer::User, "user.toml", text)])
            .expect("the fixture parses")
            .values
    }

    #[test]
    fn a_file_that_says_nothing_changes_nothing() {
        let base = KernelConfig::default();
        let out = kernel_config(&values(""), "", KernelConfig::default());
        assert_eq!(out.model, base.model);
        assert_eq!(out.budget, base.budget);
        assert_eq!(out.retry, base.retry);
        assert_eq!(out.max_output_tokens, base.max_output_tokens);
        assert!(out.prices.is_empty(), "and nothing is priced");
    }

    #[test]
    fn budget_retry_and_prices_come_from_the_file() {
        let out = kernel_config(
            &values(
                "model = \"m\"\n\
                 [budget]\nmaxTurns = 4\nmaxUsd = 2.5\n\
                 [retry]\nmaxAttempts = 1\njitter = false\n\
                 [prices.m]\ninputPerMillion = 3000000\noutputPerMillion = 15000000\n",
            ),
            "",
            KernelConfig::default(),
        );
        assert_eq!(out.model, "m");
        assert_eq!(out.budget.max_turns, 4);
        assert_eq!(out.budget.max_micro_usd, Some(2_500_000));
        assert_eq!(out.retry.max_attempts, 1);
        assert!(!out.retry.jitter);

        // And the ceiling is no longer inert: the table prices the model.
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 0,
            ..Usage::default()
        };
        assert_eq!(out.prices.micro_usd("m", &usage), Some(3_000_000));
        assert_eq!(out.prices.micro_usd("another", &usage), None);
    }

    #[test]
    fn the_profile_overlay_wins() {
        let vals = values(
            "model = \"base\"\n\
             [budget]\nmaxTurns = 4\n\
             [profile.review]\nmodel = \"reviewer\"\n\
             [profile.review.budget]\nmaxTurns = 1\n",
        );
        let out = kernel_config(&vals, "review", KernelConfig::default());
        assert_eq!(out.model, "reviewer");
        assert_eq!(out.budget.max_turns, 1);

        // A profile that sets nothing falls through to the bare keys.
        let plain = kernel_config(&vals, "other", KernelConfig::default());
        assert_eq!(plain.model, "base");
        assert_eq!(plain.budget.max_turns, 4);
    }

    #[test]
    fn a_model_id_with_dots_in_it_still_prices() {
        let table = price_table(
            &values(
                "[prices.\"claude-sonnet-4-5\"]\n\
                 inputPerMillion = 3000000\noutputPerMillion = 15000000\n",
            ),
            "",
        );
        let usage = Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Usage::default()
        };
        assert_eq!(
            table.micro_usd("claude-sonnet-4-5", &usage),
            Some(18_000_000)
        );
    }
}
