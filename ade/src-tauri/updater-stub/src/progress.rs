//! Windows Installer progress-tick model.
//!
//! MSI reports progress through `INSTALLMESSAGE_PROGRESS` records with four
//! integer fields; this struct implements the documented state machine
//! (https://learn.microsoft.com/windows/win32/msi/handling-progress-messages-using-msisetexternalui):
//!   field1 = 0 reset    → field2 total ticks, field3 direction (0 fwd / 1 back),
//!                          field4 = 1 when only generating the execute script
//!   field1 = 1 action   → field2 per-ActionData step, field3 = 1 enables stepping
//!   field1 = 2 report   → consume field2 ticks
//!   field1 = 3 extend   → grow the total by field2 ticks
//!
//! The UI clamps the rendered percent to move only forward (the design's
//! contract), so this model can stay faithful to the raw — occasionally
//! restarting — tick stream.

#[derive(Debug, Default)]
pub struct ProgressModel {
    total: i64,
    done: i64,
    /// Per-ActionData tick step (0 = ActionData messages don't advance).
    step: i64,
}

impl ProgressModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one progress record (fields 1..=4). Unknown subtypes are ignored.
    pub fn on_progress(&mut self, f1: i32, f2: i32, f3: i32, _f4: i32) {
        match f1 {
            0 => {
                // reset — a new phase starts (UI sequence, then execute sequence)
                self.total = f2.max(0) as i64;
                self.done = 0;
                self.step = 0;
                let _ = f3; // direction only affects how a native bar *renders*
            }
            1 => self.step = if f3 == 1 { f2.max(0) as i64 } else { 0 },
            2 => self.done += f2.max(0) as i64,
            3 => self.total += f2.max(0) as i64,
            _ => {}
        }
    }

    /// An `INSTALLMESSAGE_ACTIONDATA` arrived — advance by the action step.
    pub fn on_action_data(&mut self) {
        self.done += self.step;
    }

    /// 0..=100, or None while no phase total is known yet.
    pub fn percent(&self) -> Option<f64> {
        if self.total <= 0 {
            return None;
        }
        Some(((self.done as f64 / self.total as f64) * 100.0).clamp(0.0, 100.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_percent_before_reset() {
        let mut m = ProgressModel::new();
        assert_eq!(m.percent(), None);
        m.on_progress(2, 50, 0, 0); // report before reset — still no total
        assert_eq!(m.percent(), None);
    }

    #[test]
    fn reports_consume_ticks_toward_total() {
        let mut m = ProgressModel::new();
        m.on_progress(0, 200, 0, 0);
        m.on_progress(2, 50, 0, 0);
        assert_eq!(m.percent(), Some(25.0));
        m.on_progress(2, 150, 0, 0);
        assert_eq!(m.percent(), Some(100.0));
        m.on_progress(2, 999, 0, 0); // overshoot clamps
        assert_eq!(m.percent(), Some(100.0));
    }

    #[test]
    fn action_data_steps_only_when_enabled() {
        let mut m = ProgressModel::new();
        m.on_progress(0, 100, 0, 0);
        m.on_action_data(); // step not enabled — no movement
        assert_eq!(m.percent(), Some(0.0));
        m.on_progress(1, 10, 1, 0); // enable: 10 ticks per ActionData
        m.on_action_data();
        m.on_action_data();
        assert_eq!(m.percent(), Some(20.0));
        m.on_progress(1, 10, 0, 0); // disable again
        m.on_action_data();
        assert_eq!(m.percent(), Some(20.0));
    }

    #[test]
    fn extend_grows_the_total() {
        let mut m = ProgressModel::new();
        m.on_progress(0, 100, 0, 0);
        m.on_progress(2, 50, 0, 0);
        assert_eq!(m.percent(), Some(50.0));
        m.on_progress(3, 100, 0, 0); // total 100 → 200
        assert_eq!(m.percent(), Some(25.0));
    }

    #[test]
    fn reset_starts_a_new_phase() {
        let mut m = ProgressModel::new();
        m.on_progress(0, 100, 0, 0);
        m.on_progress(2, 80, 0, 0);
        m.on_progress(0, 400, 0, 1); // execute-script phase begins
        assert_eq!(m.percent(), Some(0.0));
        m.on_progress(2, 100, 0, 0);
        assert_eq!(m.percent(), Some(25.0));
    }
}
