//! One line that says what this session is costing and what the keys do.
//!
//! # What it cannot show yet
//!
//! Token counts and cost live in `RUN_FINISHED.result`, which
//! [`SurfaceStore`](orrery_client::SurfaceStore) does not keep — it projects
//! surfaces, not usage. So the footer takes its numbers from whoever *does*
//! know, through [`Footer::usage`], and shows a dash until somebody tells it.
//! Reading them out of the frame here would mean this crate parsing an AG-UI
//! event, which is exactly what the plan's constraints forbid.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::theme::Theme;

/// The footer's state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Footer {
    tokens: Option<u64>,
    /// In thousandths of a unit of currency, so no float lands in a render.
    cost_millis: Option<u64>,
    /// `execute`, `plan`, whatever the session is in.
    mode: String,
    /// Whether a turn is in flight, which changes what `^C` means.
    pub in_turn: bool,
}

impl Footer {
    /// A footer in a named mode.
    #[must_use]
    pub fn new(mode: impl Into<String>) -> Self {
        Self {
            mode: mode.into(),
            ..Self::default()
        }
    }

    /// Tell it what the session has spent.
    pub fn usage(&mut self, tokens: u64, cost_millis: u64) {
        self.tokens = Some(tokens);
        self.cost_millis = Some(cost_millis);
    }

    /// The line, as text. Public because it is what the footer test asserts on.
    #[must_use]
    pub fn line(&self) -> String {
        let tokens = match self.tokens {
            Some(n) if n >= 1000 => format!("{}.{}k", n / 1000, (n % 1000) / 100),
            Some(n) => n.to_string(),
            None => "—".to_owned(),
        };
        let cost = match self.cost_millis {
            Some(m) => format!("${}.{:02}", m / 1000, (m % 1000) / 10),
            None => "—".to_owned(),
        };
        let stop = if self.in_turn {
            "^C cancel"
        } else {
            "^D exit"
        };
        format!(
            "tokens {tokens} · {cost} · {} · {stop}",
            if self.mode.is_empty() {
                "execute"
            } else {
                &self.mode
            }
        )
    }

    /// Draw it.
    pub fn render(&self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        crate::widgets::put(buf, area, 0, &self.line(), theme.muted());
    }
}
