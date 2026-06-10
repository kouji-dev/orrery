//! Global PTY-output multiplexer: ONE `agent://output` emit per ~16ms frame
//! TOTAL, regardless of agent count.
//!
//! Why: each per-agent batcher used to `app.emit` its own flushes — on Windows
//! every emit is a PostMessageW + ExecuteScript on the Win32 UI thread (the
//! same thread Tauri invoke crosses twice), so 5 streaming agents ≈ 625
//! UI-thread jobs/s and every command gained 100-350ms. Now the per-agent
//! batchers push their UTF-8-safe chunks here, and a single drain thread
//! coalesces ALL agents' pending output into one array-payload emit per frame.
//! Idle costs nothing: the drain thread parks on a condvar — zero wakeups,
//! zero emits — until the next push.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Unfocused agents drain once per this many frames (~150ms at the 16ms app
/// frame). Their chunks keep coalescing losslessly in `pending` — they just
/// ship older — while the FOCUSED agent (the terminal the user types in)
/// drains every frame so keystroke echo stays snappy. When only background
/// agents stream, the whole loop runs at the slow cadence: fewer frames,
/// fewer Win32 UI-thread emit jobs.
const UNFOCUSED_FRAMES: u32 = 9;

/// One agent's coalesced output inside a multiplexed frame. The event payload
/// is an array of these — one entry per agent that produced output during the
/// frame. `seq` is that agent's cumulative emitted-byte count (monotonic per
/// agent; the snapshot-dedup foundation), carried over from its batcher.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OutputEntry {
    pub id: String,
    pub chunk: String,
    pub seq: u64,
}

#[derive(Default)]
struct State {
    /// agent id → (coalesced chunk, last seq). BTreeMap so a frame's entry
    /// order is deterministic.
    pending: BTreeMap<String, (String, u64)>,
    /// The agent whose terminal the user is typing in (`agent_focus`). It
    /// drains EVERY frame; everyone else only at the slow cadence.
    focused: Option<String>,
    /// agent id → when its pending output was last drained. Missing entry =
    /// never drained, which ships immediately (a new agent's first output is
    /// not delayed). Entries are removed by `take()` so exits don't leak.
    last_drain: HashMap<String, Instant>,
    /// True while the drain thread has taken a frame out of `pending` but its
    /// emit has not finished — the drained-but-unemitted window. `take()`
    /// waits this out so the exit path can't emit `agent://exit` while the
    /// agent's tail frame is still in flight.
    draining: bool,
    shutdown: bool,
}

#[derive(Default)]
pub struct OutputMux {
    state: Mutex<State>,
    cv: Condvar,
}

impl OutputMux {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a batcher flush for `id`: appends to the agent's pending chunk and
    /// advances its seq (batcher seqs are cumulative, so "last wins" keeps it
    /// monotonic). Wakes the drain thread if it is parked.
    pub fn push(&self, id: &str, chunk: String, seq: u64) {
        {
            let mut st = self.state.lock().unwrap();
            let e = st.pending.entry(id.to_string()).or_default();
            e.0.push_str(&chunk);
            e.1 = seq;
        }
        self.cv.notify_one();
    }

    /// Set (or clear, with `None`) the focused agent — the one whose terminal
    /// the user is typing in. The focused agent drains every frame; everyone
    /// else at the slow cadence. Wakes the drain thread so a newly focused
    /// agent's held-back output ships on the next frame, not when its cadence
    /// timer expires.
    pub fn set_focus(&self, id: Option<String>) {
        self.state.lock().unwrap().focused = id;
        self.cv.notify_all();
    }

    /// Remove and return `id`'s pending output, if any. The exit path uses
    /// this to force-drain an agent ahead of its `agent://exit` so no queued
    /// output event lands after the exit; it doubles as the stop/remove
    /// cleanup (the entry is gone either way).
    ///
    /// Waits out any in-flight drain emit first: the drain thread emits with
    /// the lock RELEASED (pushers must never block on UI-thread emit latency),
    /// so without the wait this could return while the agent's tail frame is
    /// drained-but-unemitted — and `agent://exit` would beat the tail.
    pub fn take(&self, id: &str) -> Option<OutputEntry> {
        let mut st = self.state.lock().unwrap();
        while st.draining {
            st = self.cv.wait(st).unwrap();
        }
        st.last_drain.remove(id); // the agent is gone — don't leak its cadence stamp
        st.pending.remove(id).map(|(chunk, seq)| OutputEntry {
            id: id.to_string(),
            chunk,
            seq,
        })
    }

    /// End the drain loop: it final-drains whatever is pending, emits it, and
    /// returns. (Tests join on this; the app never calls it — the drain thread
    /// lives for the process.)
    pub fn shutdown(&self) {
        self.state.lock().unwrap().shutdown = true;
        self.cv.notify_all();
    }

    /// Pull EVERYTHING pending, cadence ignored — the shutdown path, so
    /// nothing buffered is ever lost.
    fn drain_all(st: &mut State) -> Vec<OutputEntry> {
        std::mem::take(&mut st.pending)
            .into_iter()
            .map(|(id, (chunk, seq))| OutputEntry { id, chunk, seq })
            .collect()
    }

    /// Pull every ELIGIBLE agent's pending output: the focused agent always;
    /// everyone else only when ≥ `slow` has passed since their last drain
    /// (never-drained agents ship immediately — first output is not delayed).
    /// Also returns how long until the earliest held-back agent comes due
    /// (`None` when nothing was held back), so the drain loop knows how long
    /// it may park.
    fn drain_eligible(
        st: &mut State,
        now: Instant,
        slow: Duration,
    ) -> (Vec<OutputEntry>, Option<Duration>) {
        let mut out = Vec::new();
        let mut next_due: Option<Duration> = None;
        let ids: Vec<String> = st.pending.keys().cloned().collect();
        for id in ids {
            // None = drains now; Some(d) = held back for d more.
            let held_for = st.last_drain.get(&id).and_then(|t| {
                slow.checked_sub(now.duration_since(*t))
                    .filter(|d| !d.is_zero())
            });
            if st.focused.as_deref() == Some(id.as_str()) || held_for.is_none() {
                let (chunk, seq) = st.pending.remove(&id).expect("key came from pending");
                st.last_drain.insert(id.clone(), now);
                out.push(OutputEntry { id, chunk, seq });
            } else if let Some(d) = held_for {
                next_due = Some(next_due.map_or(d, |n| n.min(d)));
            }
        }
        (out, next_due)
    }

    /// The single drain thread's body. Parks on the condvar while nothing is
    /// pending (ZERO idle wakeups). On wake it emits — immediately after an
    /// idle stretch (typing echo stays snappy), otherwise paced so consecutive
    /// emits are ≥ `frame` apart, which caps the emit rate at one per frame no
    /// matter how many agents flood. Each emit carries every ELIGIBLE pending
    /// agent: the focused one every frame, the rest once per ~`UNFOCUSED_FRAMES`
    /// frames (their chunks keep coalescing losslessly meanwhile). When only
    /// held-back agents are pending, no frame is emitted at all — the thread
    /// parks until the earliest comes due (or a push/focus change wakes it).
    pub fn drain_loop(&self, frame: Duration, emit: impl FnMut(Vec<OutputEntry>)) {
        self.drain_loop_with_cadence(frame, frame * UNFOCUSED_FRAMES, emit);
    }

    /// [`drain_loop`] with the unfocused cadence injectable — tests pin `slow`
    /// (e.g. to "longer than the test") for load-robust determinism.
    pub(crate) fn drain_loop_with_cadence(
        &self,
        frame: Duration,
        slow: Duration,
        mut emit: impl FnMut(Vec<OutputEntry>),
    ) {
        let mut last_emit: Option<Instant> = None;
        loop {
            // Park until there is something to do.
            {
                let mut st = self.state.lock().unwrap();
                while st.pending.is_empty() && !st.shutdown {
                    st = self.cv.wait(st).unwrap();
                }
                if st.shutdown {
                    let batch = Self::drain_all(&mut st);
                    if !batch.is_empty() {
                        st.draining = true;
                        drop(st);
                        emit(batch);
                        self.state.lock().unwrap().draining = false;
                        self.cv.notify_all();
                    }
                    return;
                }
            }
            // Frame pacing: pushes landing within the remainder of the frame
            // coalesce into this batch instead of becoming their own emits.
            if let Some(t) = last_emit {
                let since = t.elapsed();
                if since < frame {
                    std::thread::sleep(frame - since);
                }
            }
            // Take the eligible agents; when everything pending is held back,
            // park (under the SAME lock acquisition — a push/focus/shutdown
            // notify between evaluate and wait must not be missed) until the
            // earliest comes due. A focused push wakes this immediately, so the
            // cadence never delays typing echo. Open the draining window BEFORE
            // unlocking, emit with the lock released (pushers stay
            // non-blocking), then close it and wake any take() parked on it.
            let batch = {
                let mut st = self.state.lock().unwrap();
                loop {
                    if st.shutdown {
                        break Vec::new(); // outer loop re-parks → final drain
                    }
                    let (b, next_due) = Self::drain_eligible(&mut st, Instant::now(), slow);
                    if !b.is_empty() {
                        st.draining = true;
                        break b;
                    }
                    match next_due {
                        Some(due_in) => st = self.cv.wait_timeout(st, due_in).unwrap().0,
                        // take() raced everything away — back to idle parking.
                        None => break Vec::new(),
                    }
                }
            };
            if !batch.is_empty() {
                emit(batch);
                last_emit = Some(Instant::now());
                self.state.lock().unwrap().draining = false;
                self.cv.notify_all();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Run drain_loop on a thread; returns (mux, emitted frame log, handle).
    /// Each logged frame is the full Vec<OutputEntry> of one emit.
    fn run(
        frame: Duration,
    ) -> (
        Arc<OutputMux>,
        Arc<Mutex<Vec<Vec<OutputEntry>>>>,
        std::thread::JoinHandle<()>,
    ) {
        let mux = Arc::new(OutputMux::new());
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (m, e) = (mux.clone(), emitted.clone());
        let h = std::thread::spawn(move || {
            m.drain_loop(frame, move |batch| e.lock().unwrap().push(batch));
        });
        (mux, emitted, h)
    }

    fn entry(id: &str, chunk: &str, seq: u64) -> OutputEntry {
        OutputEntry {
            id: id.into(),
            chunk: chunk.into(),
            seq,
        }
    }

    #[test]
    fn multi_agent_pushes_coalesce_into_one_emit_per_frame() {
        let mux = Arc::new(OutputMux::new());
        // Queue BEFORE the drain thread runs so everything is one frame:
        // two flushes for "a" (must coalesce, seq = last) and one for "b".
        mux.push("a", "a1".into(), 2);
        mux.push("a", "a2".into(), 4);
        mux.push("b", "b1".into(), 2);
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (m, e) = (mux.clone(), emitted.clone());
        let h = std::thread::spawn(move || {
            m.drain_loop(Duration::from_millis(10), move |batch| {
                e.lock().unwrap().push(batch)
            });
        });
        std::thread::sleep(Duration::from_millis(150));
        {
            let log = emitted.lock().unwrap();
            assert_eq!(log.len(), 1, "one frame → ONE emit for ALL agents: {log:?}");
            assert_eq!(log[0], vec![entry("a", "a1a2", 4), entry("b", "b1", 2)]);
        }
        mux.shutdown();
        h.join().unwrap();
    }

    #[test]
    fn idle_mux_emits_nothing_after_draining() {
        let (mux, emitted, h) = run(Duration::from_millis(10));
        mux.push("a", "x".into(), 1);
        std::thread::sleep(Duration::from_millis(150)); // long idle stretch
        assert_eq!(
            emitted.lock().unwrap().len(),
            1,
            "no pushes → parked on the condvar, no further emits"
        );
        mux.shutdown();
        h.join().unwrap();
        assert_eq!(emitted.lock().unwrap().len(), 1, "shutdown adds nothing when empty");
    }

    #[test]
    fn flood_is_paced_to_at_most_one_emit_per_frame_and_loses_nothing() {
        let frame = Duration::from_millis(40);
        let (mux, emitted, h) = run(frame);
        let start = Instant::now();
        let mut sent = String::new();
        for i in 0..50 {
            let piece = format!("c{i};");
            sent.push_str(&piece);
            mux.push("a", piece, (i + 1) as u64);
            std::thread::sleep(Duration::from_millis(4)); // ~200ms flood, 10x faster than the frame
        }
        mux.shutdown();
        h.join().unwrap();
        // Bound the emit count from MEASURED elapsed, not the nominal ~200ms:
        // under full-suite parallel load the 4ms sleeps stretch (timer
        // granularity + thread starvation), so more frames legitimately pass
        // and pacing legitimately allows more emits. Consecutive paced emits
        // are >= frame apart, so the hard cap is floor(elapsed/frame)
        // + 1 (immediate first emit) + 1 (unpaced shutdown final drain);
        // +2 extra slack.
        let elapsed = start.elapsed();
        let max_emits = (elapsed.as_millis() / frame.as_millis()) as usize + 4;
        let log = emitted.lock().unwrap();
        assert!(
            log.len() <= max_emits,
            "{}ms elapsed / {}ms frame → ≤ {} emits, got {}",
            elapsed.as_millis(),
            frame.as_millis(),
            max_emits,
            log.len()
        );
        assert!(log.len() >= 2, "a flood spans multiple frames: {}", log.len());
        let joined: String = log.iter().flatten().map(|e| e.chunk.as_str()).collect();
        assert_eq!(joined, sent, "coalescing must preserve every byte, in order");
        let seqs: Vec<u64> = log.iter().flatten().map(|e| e.seq).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "per-agent seq stays monotonic: {seqs:?}");
        assert_eq!(*seqs.last().unwrap(), 50, "last seq = the agent's final batcher seq");
    }

    #[test]
    fn take_force_drains_one_agent_without_touching_others() {
        let mux = OutputMux::new();
        mux.push("a", "tail".into(), 7);
        mux.push("a", "!".into(), 8);
        mux.push("b", "keep".into(), 3);
        assert_eq!(
            mux.take("a"),
            Some(entry("a", "tail!", 8)),
            "take returns the agent's coalesced pending output"
        );
        assert_eq!(mux.take("a"), None, "second take finds nothing — exit emits once");
        assert_eq!(mux.take("b"), Some(entry("b", "keep", 3)), "other agents untouched");
    }

    /// The exit-path race: drain_loop takes pending under lock, UNLOCKS, then
    /// emits. If the exit path's take() lands inside that drained-but-unemitted
    /// window and returns immediately, `agent://exit` goes out while the tail
    /// frame is still in flight — output after exit. take() must WAIT for the
    /// in-flight emit to finish.
    #[test]
    fn take_waits_for_in_flight_drain_emit_so_no_chunk_lands_after_it() {
        let mux = Arc::new(OutputMux::new());
        let log: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (m, l) = (mux.clone(), log.clone());
        let h = std::thread::spawn(move || {
            m.drain_loop(Duration::from_millis(1), move |batch| {
                let _ = started_tx.send(());
                // Slow UI-thread emit: the tail frame is in flight, undelivered.
                std::thread::sleep(Duration::from_millis(100));
                let mut l = l.lock().unwrap();
                for e in batch {
                    l.push(format!("delivered:{}", e.id));
                }
            });
        });
        mux.push("a", "tail".into(), 1);
        // Once the emit closure has started, "a" is already drained out of
        // pending (draining window open) but NOT yet delivered.
        started_rx.recv().unwrap();
        let taken = mux.take("a"); // exit path: must block until the emit lands
        log.lock().unwrap().push("take_returned".into());
        assert_eq!(taken, None, "tail was in the drained frame — nothing left over");
        assert_eq!(
            *log.lock().unwrap(),
            vec!["delivered:a".to_string(), "take_returned".to_string()],
            "no chunk may be delivered after take() returned — exit would beat the tail"
        );
        mux.shutdown();
        h.join().unwrap();
    }

    #[test]
    fn shutdown_final_drains_pending_like_the_batcher_does() {
        let (mux, emitted, h) = run(Duration::from_secs(10)); // frame never paces
        mux.push("a", "last words".into(), 10);
        mux.shutdown();
        h.join().unwrap();
        let log = emitted.lock().unwrap();
        let all: Vec<&OutputEntry> = log.iter().flatten().collect();
        assert_eq!(all.len(), 1);
        assert_eq!(*all[0], entry("a", "last words", 10), "nothing buffered is lost on shutdown");
    }

    /// Run drain_loop_with_cadence on a thread, emitting into an mpsc channel —
    /// the deterministic harness for the focus tests: each `recv` is exactly one
    /// frame, and with `slow` pinned far past the test's lifetime an unfocused
    /// agent can NEVER come due, so "held back" holds under arbitrary load.
    fn run_channel(
        frame: Duration,
        slow: Duration,
    ) -> (
        Arc<OutputMux>,
        std::sync::mpsc::Receiver<Vec<OutputEntry>>,
        std::thread::JoinHandle<()>,
    ) {
        let mux = Arc::new(OutputMux::new());
        let (tx, rx) = std::sync::mpsc::channel();
        let m = mux.clone();
        let h = std::thread::spawn(move || {
            m.drain_loop_with_cadence(frame, slow, move |b| {
                let _ = tx.send(b);
            });
        });
        (mux, rx, h)
    }

    const NEVER: Duration = Duration::from_secs(600);

    #[test]
    fn focused_agent_drains_every_frame_while_unfocused_is_held() {
        let (mux, rx, h) = run_channel(Duration::from_millis(1), NEVER);
        mux.set_focus(Some("f".into()));
        // A never-drained agent's FIRST output ships immediately even unfocused
        // (no cadence stamp yet) — a new agent's first paint is not delayed.
        mux.push("b", "b0".into(), 2);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(10)).unwrap(),
            vec![entry("b", "b0", 2)],
            "first-ever output is not held back"
        );
        // From here "b" sits inside its (600s) cadence window: every frame must
        // carry the focused agent and never "b" — no wall-clock window to miss,
        // so this is exact under any load.
        let mut held = String::new();
        for i in 1..=5u64 {
            mux.push("f", format!("f{i}"), i);
            let piece = format!("b{i}");
            held.push_str(&piece);
            mux.push("b", piece, 2 + i);
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(10)).unwrap(),
                vec![entry("f", &format!("f{i}"), i)],
                "focused drains every frame; unfocused held (and emits no frame of its own)"
            );
        }
        // Shutdown final-drains the held-back agent: older, but lossless.
        mux.shutdown();
        h.join().unwrap();
        let tail: Vec<OutputEntry> = rx.try_iter().flatten().collect();
        assert_eq!(
            tail,
            vec![entry("b", &held, 7)],
            "held chunks coalesce losslessly with the last seq"
        );
    }

    #[test]
    fn focus_switch_mid_stream_flips_who_gets_the_fast_path() {
        let (mux, rx, h) = run_channel(Duration::from_millis(1), NEVER);
        let recv = |what: &str| rx.recv_timeout(Duration::from_secs(10)).expect(what);
        mux.set_focus(Some("f".into()));
        // Stamp both agents' cadence clocks (first output ships immediately).
        mux.push("f", "f0".into(), 1);
        assert_eq!(recv("f's first frame"), vec![entry("f", "f0", 1)]);
        mux.push("b", "b0".into(), 1);
        assert_eq!(recv("b's first frame"), vec![entry("b", "b0", 1)]);
        // Mid-stream: f rides the fast path, b is held.
        mux.push("f", "f1".into(), 2);
        mux.push("b", "b1".into(), 2);
        assert_eq!(recv("pre-switch frame"), vec![entry("f", "f1", 2)]);
        // Switch focus to b: the focus change itself wakes the parked drain
        // thread, so b's HELD chunk ships on the next frame — no new push, no
        // waiting out the cadence timer.
        mux.set_focus(Some("b".into()));
        assert_eq!(
            recv("switch flushes b's held output"),
            vec![entry("b", "b1", 2)]
        );
        // And the roles are now swapped: f is held, b is per-frame.
        mux.push("f", "f2".into(), 3);
        mux.push("b", "b2".into(), 3);
        assert_eq!(recv("post-switch frame"), vec![entry("b", "b2", 3)]);
        mux.shutdown();
        h.join().unwrap();
        let tail: Vec<OutputEntry> = rx.try_iter().flatten().collect();
        assert_eq!(
            tail,
            vec![entry("f", "f2", 3)],
            "the now-unfocused agent's held output final-drains on shutdown"
        );
    }

    #[test]
    fn no_focus_holds_every_agent_to_the_slow_cadence() {
        let frame = Duration::from_millis(5);
        let slow = Duration::from_millis(40);
        let mux = Arc::new(OutputMux::new());
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (m, e) = (mux.clone(), emitted.clone());
        let h = std::thread::spawn(move || {
            m.drain_loop_with_cadence(frame, slow, move |b| e.lock().unwrap().push(b));
        });
        mux.set_focus(None); // explicit: None = NOBODY rides the per-frame path
        let start = Instant::now();
        let (mut sent_a, mut sent_b) = (String::new(), String::new());
        for i in 0..60u64 {
            let (pa, pb) = (format!("a{i};"), format!("b{i};"));
            sent_a.push_str(&pa);
            sent_b.push_str(&pb);
            mux.push("a", pa, i + 1);
            mux.push("b", pb, i + 1);
            std::thread::sleep(Duration::from_millis(2)); // ~120ms flood
        }
        mux.shutdown();
        h.join().unwrap();
        // Bound from MEASURED elapsed (see flood_is_paced): consecutive drains
        // of one unfocused agent are >= slow apart, so the per-agent cap is
        // floor(elapsed/slow) + 1 (immediate first drain) + 1 (shutdown final
        // drain) + 2 slack.
        let elapsed = start.elapsed();
        let max_per_agent = (elapsed.as_millis() / slow.as_millis()) as usize + 4;
        let log = emitted.lock().unwrap();
        for id in ["a", "b"] {
            let frames = log.iter().filter(|f| f.iter().any(|e| e.id == id)).count();
            assert!(
                frames <= max_per_agent,
                "{}ms elapsed / {}ms slow → ≤ {max_per_agent} frames for '{id}', got {frames}",
                elapsed.as_millis(),
                slow.as_millis(),
            );
        }
        let join = |id: &str| -> String {
            log.iter()
                .flatten()
                .filter(|e| e.id == id)
                .map(|e| e.chunk.as_str())
                .collect()
        };
        assert_eq!(join("a"), sent_a, "slow cadence still loses nothing");
        assert_eq!(join("b"), sent_b, "slow cadence still loses nothing");
    }

    // The PUBLIC drain_loop derives the cadence (frame × UNFOCUSED_FRAMES):
    // an unfocused agent flooding while someone ELSE is focused is paced by
    // slow, not by frame — fewer emits when only background agents stream.
    #[test]
    fn unfocused_flood_is_paced_by_the_slow_cadence_not_the_frame() {
        let frame = Duration::from_millis(5); // slow = 45ms via drain_loop
        let mux = Arc::new(OutputMux::new());
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let (m, e) = (mux.clone(), emitted.clone());
        let h = std::thread::spawn(move || {
            m.drain_loop(frame, move |b| e.lock().unwrap().push(b));
        });
        mux.set_focus(Some("f".into())); // focused agent exists but stays silent
        let start = Instant::now();
        let mut sent = String::new();
        for i in 0..60u64 {
            let piece = format!("b{i};");
            sent.push_str(&piece);
            mux.push("b", piece, i + 1);
            std::thread::sleep(Duration::from_millis(2)); // ~120ms flood
        }
        mux.shutdown();
        h.join().unwrap();
        // Same measured-elapsed bound as flood_is_paced, against slow.
        let elapsed = start.elapsed();
        let slow = frame * UNFOCUSED_FRAMES;
        let max_emits = (elapsed.as_millis() / slow.as_millis()) as usize + 4;
        let log = emitted.lock().unwrap();
        assert!(
            log.len() <= max_emits,
            "{}ms elapsed / {}ms slow → ≤ {max_emits} emits, got {}",
            elapsed.as_millis(),
            slow.as_millis(),
            log.len()
        );
        let joined: String = log.iter().flatten().map(|e| e.chunk.as_str()).collect();
        assert_eq!(joined, sent, "cadence holding must preserve every byte, in order");
        let seqs: Vec<u64> = log.iter().flatten().map(|e| e.seq).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "seq stays monotonic: {seqs:?}");
    }

    // Locks the wire shape the frontend types against: an ARRAY of
    // {id, chunk, seq} objects.
    #[test]
    fn frame_serializes_to_the_event_payload_shape() {
        let frame = vec![entry("a", "x", 3), entry("b", "y", 1)];
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            serde_json::json!([
                { "id": "a", "chunk": "x", "seq": 3 },
                { "id": "b", "chunk": "y", "seq": 1 }
            ])
        );
    }
}
