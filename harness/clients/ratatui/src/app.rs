//! The client itself: the store, the split screen and the event loop.
//!
//! # What this crate is not allowed to do
//!
//! Decoding, `seq` tracking and store mutation live in `orrery-client`. Nothing
//! here parses an AG-UI event: [`App::apply`] hands the frame to
//! [`SurfaceStore`] and reacts to the [`StoreChange`]s that come back. If a
//! future change tempts somebody to read `frame.event` here, it belongs in the
//! SDK instead — that is what keeps five renderers agreeing.
//!
//! # The loop
//!
//! Frames in, [`Outgoing`] out, one draw per frame budget rather than one per
//! frame. A settled turn is printed into scrollback exactly once, by id, and
//! [`App::printed`] is what makes "exactly once" checkable.

use std::collections::HashMap;

use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use orrery_agui::Frame;
use orrery_client::{ClientError, PromptView, StoreChange, SurfaceStore, TurnView};

use crate::composer::{Action, Composer};
use crate::event::{FrameSource, Outgoing};
use crate::footer::Footer;
use crate::live::LiveRegion;
use crate::scrollback::Scrollback;
use crate::theme::Theme;
use crate::widgets;

/// How long a batch of appends is allowed to accumulate before a redraw.
///
/// A thousand `TEXT_MESSAGE_CONTENT` deltas in a second must cost one frame,
/// not a thousand: the terminal is the slow part, and redrawing per delta is
/// how a TUI ends up slower than `cat`.
pub const FRAME_BUDGET: std::time::Duration = std::time::Duration::from_millis(33);

/// Restores the terminal on the way out, panic or not.
///
/// A `Drop` and not a line at the end of `run`: a panic in a widget must not
/// leave somebody's shell in raw mode with no echo. The closure is whatever
/// undoes the setup, so the test can assert the guard fires without a tty.
pub struct RestoreGuard<F: FnMut()> {
    restore: F,
}

impl<F: FnMut()> RestoreGuard<F> {
    /// Arm it.
    pub fn new(restore: F) -> Self {
        Self { restore }
    }
}

impl<F: FnMut()> Drop for RestoreGuard<F> {
    fn drop(&mut self) {
        (self.restore)();
    }
}

/// The client.
pub struct App {
    store: SurfaceStore,
    width: u16,
    theme: Theme,
    live: LiveRegion,
    footer: Footer,
    composer: Composer,
    /// Settled turns already printed into scrollback, in the order they went.
    printed: Vec<String>,
    /// When each prompt was raised, on this client's clock.
    raised: HashMap<String, u64>,
    /// The clock. Driven by the loop in the binary and by the tests directly,
    /// so a deadline test does not have to sleep.
    now_ms: u64,
    running: bool,
    dirty: bool,
    draws: u64,
}

impl App {
    /// A client for a terminal this wide.
    #[must_use]
    pub fn new(width: u16) -> Self {
        Self {
            store: SurfaceStore::new(),
            width: width.max(1),
            theme: Theme::colour(),
            live: LiveRegion::default(),
            footer: Footer::new("execute"),
            composer: Composer::new(),
            printed: Vec::new(),
            raised: HashMap::new(),
            now_ms: 0,
            running: true,
            dirty: false,
            draws: 0,
        }
    }

    /// Use this palette. Monochrome is the degraded terminal.
    #[must_use]
    pub fn with_theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// What it has understood so far.
    #[must_use]
    pub fn store(&self) -> &SurfaceStore {
        &self.store
    }

    /// The composer, for the key tests.
    #[must_use]
    pub fn composer(&self) -> &Composer {
        &self.composer
    }

    /// The composer, mutably.
    pub fn composer_mut(&mut self) -> &mut Composer {
        &mut self.composer
    }

    /// The live region's cursor state.
    pub fn live_mut(&mut self) -> &mut LiveRegion {
        &mut self.live
    }

    /// The footer.
    pub fn footer_mut(&mut self) -> &mut Footer {
        &mut self.footer
    }

    /// Whether the loop is still going.
    #[must_use]
    pub fn running(&self) -> bool {
        self.running
    }

    /// How many times the live region has been drawn. The frame-budget assert.
    #[must_use]
    pub fn draws(&self) -> u64 {
        self.draws
    }

    /// Which turns have gone into scrollback, oldest first.
    #[must_use]
    pub fn printed(&self) -> &[String] {
        &self.printed
    }

    /// Move the clock. Deadlines are measured against it.
    pub fn advance(&mut self, ms: u64) {
        self.now_ms = self.now_ms.saturating_add(ms);
        if self.prompt().is_some() {
            self.dirty = true;
        }
    }

    /// Re-wrap for a new width.
    ///
    /// Only the live region. What is already in scrollback belongs to the
    /// terminal, which reflows it — or does not — by its own rules; a client
    /// that tried to redraw it would fight the terminal and lose the selection.
    pub fn resize(&mut self, width: u16) {
        self.width = width.max(1);
        self.dirty = true;
    }

    /// How wide it thinks the terminal is.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Apply one frame and say what the kernel should hear about it.
    pub fn apply(&mut self, frame: &Frame) -> Vec<Outgoing> {
        let mut out = Vec::new();
        for change in self.store.apply(frame) {
            match change {
                StoreChange::GapDetected { expected, .. } => {
                    // A hole is a reason to ask for the missing frames, not to
                    // throw the turn away: re-attach from the last seq we are
                    // sure of, which is the one before the expected.
                    out.push(Outgoing::Reattach {
                        since: expected.saturating_sub(1),
                    });
                }
                StoreChange::TurnStarted { .. } => self.footer.in_turn = true,
                StoreChange::TurnSettled { .. } => self.footer.in_turn = false,
                StoreChange::PromptRaised { prompt } => {
                    self.raised.insert(prompt, self.now_ms);
                }
                _ => {}
            }
        }
        self.dirty = true;
        out
    }

    /// The consent prompt awaiting an answer, if there is one.
    #[must_use]
    pub fn prompt(&self) -> Option<&PromptView> {
        self.store
            .state()
            .turns
            .iter()
            .flat_map(|t| t.prompts.iter())
            .find(|p| p.resolved.is_none())
    }

    /// What is left of a prompt's deadline.
    #[must_use]
    pub fn remaining_ms(&self, prompt: &PromptView) -> u64 {
        let raised = self.raised.get(&prompt.id).copied().unwrap_or(self.now_ms);
        prompt
            .deadline_ms
            .saturating_sub(self.now_ms.saturating_sub(raised))
    }

    /// Print every settled turn that has not been printed yet.
    ///
    /// Once, by id. A turn that is already in `printed` is skipped however many
    /// times the store reports it settled, which is what keeps a re-attach from
    /// duplicating the transcript.
    ///
    /// # Errors
    ///
    /// Whatever the terminal said.
    pub fn flush_scrollback(&mut self, sink: &mut dyn Scrollback) -> std::io::Result<()> {
        let width = sink.width().max(1);
        let pending: Vec<String> = self
            .store
            .settled()
            .map(|t| t.id.clone())
            .filter(|id| !self.printed.contains(id))
            .collect();
        for id in pending {
            let Some(turn) = self.store.turn(&id).cloned() else {
                continue;
            };
            let height = Self::turn_height(&turn, width);
            let theme = self.theme;
            sink.insert_before(height, &mut |buf: &mut Buffer| {
                Self::draw_turn(&turn, buf, &theme);
            })?;
            self.printed.push(id);
        }
        Ok(())
    }

    /// The whole live area: surfaces, consent bar, composer, footer.
    ///
    /// Drawn only when something changed, so a thousand appends inside one
    /// frame budget cost one draw.
    pub fn draw_if_dirty(&mut self, buf: &mut Buffer) -> bool {
        if !self.dirty {
            return false;
        }
        self.draw(buf);
        true
    }

    /// Draw unconditionally. `^L`, and the first frame.
    pub fn draw(&mut self, buf: &mut Buffer) {
        let area = buf.area;
        self.draws += 1;
        self.dirty = false;
        let footer_h = 1u16;
        let composer_h = self.composer.height();
        let prompt = self.prompt().cloned();
        let consent_h = if prompt.is_some() {
            widgets::question::CONSENT_HEIGHT
        } else {
            0
        };
        let chrome = footer_h
            .saturating_add(composer_h)
            .saturating_add(consent_h);
        let live_h = area.height.saturating_sub(chrome);

        let live = self.store.live().cloned();
        self.live.render(
            live.as_ref(),
            Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: live_h,
            },
            buf,
            &self.theme,
        );

        let mut y = area.y.saturating_add(live_h);
        if let Some(prompt) = &prompt {
            let remaining = self.remaining_ms(prompt);
            widgets::question::consent_bar(
                prompt,
                remaining,
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: consent_h,
                },
                buf,
                &self.theme,
            );
            y = y.saturating_add(consent_h);
        }
        self.composer.render(
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: composer_h,
            },
            buf,
            &self.theme,
        );
        y = y.saturating_add(composer_h);
        self.footer.render(
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: footer_h,
            },
            buf,
            &self.theme,
        );
    }

    /// Feed one key.
    pub fn key(&mut self, key: KeyEvent) -> Vec<Outgoing> {
        self.dirty = true;
        // A consent bar takes the keys while it is live: it is the client's own
        // chrome, and typing into the composer under it would be answering a
        // question nobody asked.
        if let Some(prompt) = self.prompt().cloned() {
            let remaining = self.remaining_ms(&prompt);
            if widgets::question::accepts_answer(&prompt, remaining) {
                if let Some(answer) = consent_key(key) {
                    return vec![Outgoing::Consent {
                        prompt: prompt.id,
                        answer,
                    }];
                }
                // Anything else is swallowed; the bar is modal by design.
                return Vec::new();
            }
        }
        if let Some(out) = self.question_key(key) {
            return out;
        }
        match self.composer.key(key, self.footer.in_turn) {
            Action::None => Vec::new(),
            Action::Redraw => {
                self.dirty = true;
                Vec::new()
            }
            Action::Send(Outgoing::Exit) => {
                self.running = false;
                vec![Outgoing::Exit]
            }
            Action::Send(out) => vec![out],
        }
    }

    /// Arrow keys and Enter, when the live turn is asking something.
    fn question_key(&mut self, key: KeyEvent) -> Option<Vec<Outgoing>> {
        let turn = self.store.live()?;
        let view = turn.surfaces.iter().find(|s| {
            matches!(s.kind, orrery_proto::SurfaceKind::Question { .. })
                && s.status != Some(orrery_proto::Status::Done)
        })?;
        let id = view.id.clone();
        let surface = crate::testing::as_surface(view);
        let orrery_proto::SurfaceKind::Question { choices, .. } = &surface.kind else {
            return None;
        };
        let last = choices.len().saturating_sub(1);
        match key.code {
            crossterm::event::KeyCode::Up => {
                self.live.selection.at = self.live.selection.at.saturating_sub(1);
                Some(Vec::new())
            }
            crossterm::event::KeyCode::Down => {
                self.live.selection.at = (self.live.selection.at + 1).min(last);
                Some(Vec::new())
            }
            crossterm::event::KeyCode::Enter => {
                let value = widgets::question::answer(&surface, &self.live.selection)?;
                Some(vec![Outgoing::Intent {
                    surface: id,
                    value,
                }])
            }
            _ => None,
        }
    }

    /// Run until the source ends or `^D`.
    ///
    /// # Errors
    ///
    /// The first transport error, or whatever the terminal said.
    pub async fn run(
        &mut self,
        source: &mut dyn FrameSource,
        sink: &mut dyn Scrollback,
    ) -> Result<(), ClientError> {
        while self.running {
            let Some(next) = source.next_frame().await else {
                break;
            };
            match next {
                Ok(frame) => {
                    self.apply(&frame);
                }
                // A gap is reported by the store as well, but a transport that
                // noticed first is still telling the truth: re-attach and keep
                // going rather than ending the session.
                Err(ClientError::Gap { .. }) => continue,
                Err(err) => return Err(err),
            }
            self.flush_scrollback(sink)
                .map_err(ClientError::Io)?;
        }
        self.running = false;
        self.flush_scrollback(sink)
            .map_err(ClientError::Io)?;
        Ok(())
    }

    /// How many rows a settled turn needs in scrollback.
    #[must_use]
    pub fn turn_height(turn: &TurnView, width: u16) -> u16 {
        LiveRegion::measure(Some(turn), width).max(1)
    }

    /// Draw a settled turn into a scrollback block.
    pub fn draw_turn(turn: &TurnView, buf: &mut Buffer, theme: &Theme) {
        LiveRegion::default().render(Some(turn), buf.area, buf, theme);
    }
}

/// The consent bar's keys. Deliberately not the composer's.
fn consent_key(key: KeyEvent) -> Option<orrery_proto::ConsentAnswerKind> {
    match key.code {
        crossterm::event::KeyCode::Char('a') => Some(orrery_proto::ConsentAnswerKind::AllowOnce),
        crossterm::event::KeyCode::Char('s') => Some(orrery_proto::ConsentAnswerKind::AllowAlways),
        crossterm::event::KeyCode::Char('d') => Some(orrery_proto::ConsentAnswerKind::Deny),
        _ => None,
    }
}
