# PTY Output Pipeline Implementation Plan (Roadmap Phase 1)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the raw per-read PTY→webview pipe into a budgeted pipeline: batch in Rust (8ms/16KB, UTF-8-safe, byte seq), pace hidden-terminal writes through one shared scheduler with a lossy 2MB cap, and kill the per-chunk signal fanout — so the focused terminal stays responsive and memory stays bounded no matter how many agents flood.

**Architecture:** Rust reader threads forward raw bytes to a per-PTY batcher thread that flushes on 8ms-or-16KB and emits `agent://output {id, chunk, seq}` (seq = cumulative UTF-8 bytes — the Phase 2 snapshot-dedup foundation; the frontend ignores it for now). The frontend routes every xterm write through a shared scheduler: visible terminal writes go direct; hidden terminals drain round-robin at 2×16KB per 16ms tick, capped at 2MB with a drop-warning. Per-agent revision signals and a liveLogs coalescer replace the per-chunk map publishes. Counters surface in the existing DevConsole perf table (Rust emit rate appears as a `agent_output_emit` row for free via `perf::timed`).

**Tech Stack:** Rust (std mpsc + threads, mirrors `watch/mod.rs` patterns), Angular 20 signals, @xterm/xterm 6, vitest (`vi.useFakeTimers`), cargo test.

**Key facts (verified):**
- `src-tauri/src/runtime/mod.rs:143-157` — reader thread emits one `agent://output` per ≤4KB read; `String::from_utf8_lossy` per read can corrupt multibyte chars split across read boundaries (pre-existing bug the batcher fixes).
- `src/app/terminal.service.ts:158-162` — every chunk writes straight into the agent's xterm even when hidden; `terminal.service.ts:53-55` bumps a shared `revision` Record-signal (new map identity) per parsed chunk → every `revOf()` consumer wakes on every chunk of every agent (`mini-term.component.ts:82,108`).
- `agent-runtime.service.ts:163-167` — `liveLogs.update` clones the whole map per chunk (same fanout).
- `perf::timed(cmd, f)` + the DevConsole perf table give per-command call-rate rows automatically — wrapping the batched emit makes the event rate visible with zero new UI.
- `terminal.service.spec.ts` writes to never-attached terminals (no DOM element → scheduler classifies them hidden) — its `flush()` helper must drain the scheduler queue first.
- Visibility proxy: `h.term.element?.isConnected === true` — `attach()` parents the element, eviction disconnects it; never-attached terminals have no element.

**File map:**
- Create: `src-tauri/src/runtime/output_batcher.rs` — pure `batch_loop` + UTF-8 boundary logic
- Modify: `src-tauri/src/runtime/mod.rs` — reader→batcher→emit wiring
- Create: `src/app/terminal-output-scheduler.ts` (+ `.spec.ts`) — shared write scheduler
- Modify: `src/app/terminal.service.ts` — route writes, per-agent revision signals
- Modify: `src/app/terminal.service.spec.ts` — flush helper drains scheduler
- Create: `src/app/agents/pty-tail-coalescer.ts` (+ `.spec.ts`) — liveLogs batching
- Modify: `src/app/agents/agent-runtime.service.ts` — coalescer wiring
- Modify: `src/app/dev-tools/dev-panel.component.ts` — scheduler counters in perf footer

---

### Task 1: UTF-8-safe output batcher core (Rust)

**Files:**
- Create: `src-tauri/src/runtime/output_batcher.rs`
- Modify: `src-tauri/src/runtime/mod.rs` (one line: module declaration)

- [ ] **Step 1: Declare the module and write the failing tests**

Add to `src-tauri/src/runtime/mod.rs` near the top (next to the existing `mod jobobj;`-style declarations):

```rust
pub(crate) mod output_batcher;
```

Create `src-tauri/src/runtime/output_batcher.rs` with the test module first:

```rust
//! Coalesces raw PTY bytes into bounded, UTF-8-safe flushes before IPC.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};

    /// Run batch_loop on a thread; returns (byte sender, emitted (chunk, seq) log).
    fn run(
        window: Duration,
        max_bytes: usize,
    ) -> (
        std::sync::mpsc::Sender<Vec<u8>>,
        Arc<Mutex<Vec<(String, u64)>>>,
        std::thread::JoinHandle<()>,
    ) {
        let (tx, rx) = channel::<Vec<u8>>();
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let e = emitted.clone();
        let h = std::thread::spawn(move || {
            batch_loop(rx, window, max_bytes, move |chunk, seq| {
                e.lock().unwrap().push((chunk, seq));
            });
        });
        (tx, emitted, h)
    }

    #[test]
    fn coalesces_small_writes_into_one_emit() {
        let (tx, emitted, h) = run(Duration::from_millis(30), 16 * 1024);
        tx.send(b"a".to_vec()).unwrap();
        tx.send(b"b".to_vec()).unwrap();
        tx.send(b"c".to_vec()).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        {
            let log = emitted.lock().unwrap();
            assert_eq!(log.len(), 1, "one window → one emit: {log:?}");
            assert_eq!(log[0].0, "abc");
        }
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn flushes_immediately_at_max_bytes() {
        let (tx, emitted, h) = run(Duration::from_secs(10), 8); // window never fires
        tx.send(b"0123456789".to_vec()).unwrap(); // 10 ≥ 8 → immediate flush
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(emitted.lock().unwrap().len(), 1, "size flush, no window wait");
        assert_eq!(emitted.lock().unwrap()[0].0, "0123456789");
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn multibyte_char_split_across_reads_stays_intact() {
        let (tx, emitted, h) = run(Duration::from_millis(30), 16 * 1024);
        let e = "é".as_bytes(); // [0xC3, 0xA9]
        tx.send(vec![e[0]]).unwrap();
        std::thread::sleep(Duration::from_millis(120)); // window fires — must hold the partial char
        assert!(emitted.lock().unwrap().is_empty(), "partial char must not emit");
        tx.send(vec![e[1]]).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        {
            let log = emitted.lock().unwrap();
            assert_eq!(log.len(), 1);
            assert_eq!(log[0].0, "é", "no U+FFFD from a split char");
        }
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn seq_accumulates_emitted_bytes() {
        let (tx, emitted, h) = run(Duration::from_millis(30), 16 * 1024);
        tx.send(b"abc".to_vec()).unwrap();
        std::thread::sleep(Duration::from_millis(120));
        tx.send("é".as_bytes().to_vec()).unwrap(); // 2 bytes
        std::thread::sleep(Duration::from_millis(120));
        {
            let log = emitted.lock().unwrap();
            assert_eq!(log[0].1, 3, "seq after first flush = bytes so far");
            assert_eq!(log[1].1, 5, "seq is cumulative UTF-8 bytes");
        }
        drop(tx);
        h.join().unwrap();
    }

    #[test]
    fn disconnect_flushes_remainder_lossy() {
        let (tx, emitted, h) = run(Duration::from_secs(10), 16 * 1024); // window never fires
        tx.send(b"tail".to_vec()).unwrap();
        tx.send(vec![0xC3]).unwrap(); // dangling partial char
        drop(tx); // PTY closed
        h.join().unwrap();
        let log = emitted.lock().unwrap();
        assert_eq!(log.len(), 1, "final flush on disconnect");
        assert!(log[0].0.starts_with("tail"), "buffered bytes emitted");
        assert_eq!(log[0].1, 5, "seq counts the dangling byte too");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml output_batcher`
Expected: FAIL to compile — `batch_loop` not found.

- [ ] **Step 3: Implement `batch_loop`**

Add above the test module in `output_batcher.rs`:

```rust
/// Length of the longest prefix of `bytes` that ends on a UTF-8 character
/// boundary. A trailing incomplete sequence is excluded so a 4KB read boundary
/// can never split a char into two lossy (U+FFFD) halves.
fn utf8_complete_prefix_len(bytes: &[u8]) -> usize {
    let len = bytes.len();
    // start of the last (possibly incomplete) char: a non-continuation byte
    // within the final 4 bytes
    let start = (1..=4.min(len))
        .map(|back| len - back)
        .find(|&i| bytes[i] & 0xC0 != 0x80)
        .unwrap_or(0);
    let first = bytes[start];
    let need = if first < 0x80 {
        1
    } else if first & 0xE0 == 0xC0 {
        2
    } else if first & 0xF0 == 0xE0 {
        3
    } else if first & 0xF8 == 0xF0 {
        4
    } else {
        1 // invalid lead byte — let from_utf8_lossy deal with it
    };
    if len - start < need {
        start
    } else {
        len
    }
}

fn flush(
    pending: &mut Vec<u8>,
    seq: &mut u64,
    emit: &mut impl FnMut(String, u64),
    final_flush: bool,
) {
    if pending.is_empty() {
        return;
    }
    let cut = if final_flush {
        pending.len() // stream over — emit the dangling partial char lossily
    } else {
        utf8_complete_prefix_len(pending)
    };
    if cut == 0 {
        return; // only a partial char buffered — wait for its continuation bytes
    }
    let chunk: Vec<u8> = pending.drain(..cut).collect();
    *seq += chunk.len() as u64;
    emit(String::from_utf8_lossy(&chunk).to_string(), *seq);
}

/// Coalesce incoming byte chunks and emit them as strings: flush when the
/// pending buffer reaches `max_bytes` or `window` after the first pending byte,
/// whichever comes first. `seq` passed to `emit` is the cumulative count of
/// UTF-8 bytes emitted — the dedup foundation for snapshot recovery.
/// Returns when the sender side disconnects, after a final lossy flush.
pub fn batch_loop(
    rx: Receiver<Vec<u8>>,
    window: Duration,
    max_bytes: usize,
    mut emit: impl FnMut(String, u64),
) {
    let mut pending: Vec<u8> = Vec::new();
    let mut seq: u64 = 0;
    // invariant: pending non-empty ⇒ deadline Some (a held partial char keeps
    // rescheduling — a harmless ~window-rate wake while its continuation is late)
    let mut deadline: Option<Instant> = None;
    loop {
        let msg = match deadline {
            None => match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            },
            Some(d) => {
                let now = Instant::now();
                if now >= d {
                    None
                } else {
                    match rx.recv_timeout(d - now) {
                        Ok(m) => Some(m),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            }
        };
        match msg {
            Some(bytes) => {
                if pending.is_empty() {
                    deadline = Some(Instant::now() + window);
                }
                pending.extend_from_slice(&bytes);
                if pending.len() >= max_bytes {
                    flush(&mut pending, &mut seq, &mut emit, false);
                    deadline = if pending.is_empty() {
                        None
                    } else {
                        Some(Instant::now() + window)
                    };
                }
            }
            None => {
                // window elapsed
                flush(&mut pending, &mut seq, &mut emit, false);
                deadline = if pending.is_empty() {
                    None
                } else {
                    Some(Instant::now() + window)
                };
            }
        }
    }
    flush(&mut pending, &mut seq, &mut emit, true);
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml output_batcher`
Expected: 5 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/output_batcher.rs src-tauri/src/runtime/mod.rs
git commit -m "feat(runtime): UTF-8-safe PTY output batcher core

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Wire the reader through the batcher (Rust)

**Files:**
- Modify: `src-tauri/src/runtime/mod.rs:138-157` (reader thread block)

- [ ] **Step 1: Replace the per-read emit with the batcher pipeline**

Replace the current reader-thread block (the `// stream stdout/stderr → agent://output ...` comment through its `});`) with:

```rust
        // stream stdout/stderr → batcher → `agent://output`. The batcher thread
        // coalesces reads into ≤8ms / ≥16KB flushes before IPC and tags each
        // flush with a cumulative byte seq (snapshot-dedup foundation).
        // Why: per-read emits (≤4KB) cost one JSON serialization + webview
        // wakeup each — hundreds/sec under agent floods. Batching caps the
        // event rate at ~125/sec per agent regardless of throughput, and the
        // batcher's UTF-8 boundary handling fixes multibyte chars split across
        // read boundaries (from_utf8_lossy per read corrupted them).
        // Neither thread emits `agent://exit`; both end once the master is
        // dropped (reader read fails → sender drops → batcher final-flushes).
        let app_out = app.clone();
        let out_id = id.to_string();
        let (batch_tx, batch_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            output_batcher::batch_loop(
                batch_rx,
                std::time::Duration::from_millis(8),
                16 * 1024,
                move |chunk, seq| {
                    // timed so the emit RATE shows up as a perf-table row —
                    // `agent_output_emit` calls/10s is the batching proof
                    crate::perf::timed("agent_output_emit", || {
                        let _ = app_out.emit(
                            "agent://output",
                            serde_json::json!({ "id": out_id, "chunk": chunk, "seq": seq }),
                        );
                    });
                },
            );
        });
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        // batcher gone (shutdown) → stop reading
                        if batch_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
```

(Exit-ordering note: today `agent://exit` can already overtake the last output reads; the batcher widens that window by ≤8ms. The frontend's `onExit` only appends a status line, and late writes still render — unchanged behavior, documented here so nobody "fixes" it blindly.)

- [ ] **Step 2: Run the full Rust suite + lints**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings && cargo fmt --manifest-path src-tauri/Cargo.toml`
Expected: all green (known flake: rerun once if an unrelated test trips).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/runtime/mod.rs
git commit -m "feat(runtime): batch PTY output before IPC with byte seq

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Shared terminal write scheduler (frontend core)

**Files:**
- Create: `src/app/terminal-output-scheduler.ts`
- Test: `src/app/terminal-output-scheduler.spec.ts`

- [ ] **Step 1: Write the failing tests**

Create `src/app/terminal-output-scheduler.spec.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BACKLOG_WARNING,
  DRAIN_INTERVAL_MS,
  HIDDEN_FIRST_FLUSH_DELAY_MS,
  discardTerminalQueue,
  flushTerminalQueue,
  resetTerminalSchedulerForTests,
  terminalSchedulerStats,
  writeScheduled,
} from "./terminal-output-scheduler";

/** Fake xterm: records writes, fires the parsed callback synchronously. */
function fakeTerm() {
  const writes: string[] = [];
  return {
    writes,
    write(data: string, cb?: () => void) {
      writes.push(data);
      cb?.();
    },
  };
}

describe("terminal-output-scheduler", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetTerminalSchedulerForTests();
  });
  afterEach(() => vi.useRealTimers());

  it("visible writes go direct, after any queued backlog (order preserved)", () => {
    const t = fakeTerm();
    writeScheduled("a", t, "queued1", { visible: false });
    writeScheduled("a", t, "queued2", { visible: false });
    expect(t.writes).toEqual([]); // hidden → not written yet
    writeScheduled("a", t, "live", { visible: true });
    expect(t.writes).toEqual(["queued1queued2", "live"]); // backlog first, then live
  });

  it("hidden writes drain after the first flush delay, in order", () => {
    const t = fakeTerm();
    writeScheduled("a", t, "one", { visible: false });
    writeScheduled("a", t, "two", { visible: false });
    vi.advanceTimersByTime(HIDDEN_FIRST_FLUSH_DELAY_MS + 1);
    expect(t.writes).toEqual(["onetwo"]);
  });

  it("round-robins across hidden terminals with bounded writes per tick", () => {
    const big = "x".repeat(20 * 1024); // > one drain chunk each
    const ta = fakeTerm();
    const tb = fakeTerm();
    writeScheduled("a", ta, big, { visible: false });
    writeScheduled("b", tb, big, { visible: false });
    vi.advanceTimersByTime(HIDDEN_FIRST_FLUSH_DELAY_MS + 1); // first tick: 2 writes max
    expect(ta.writes.length + tb.writes.length).toBe(2);
    expect(ta.writes.length).toBe(1); // one each — not both from "a"
    expect(tb.writes.length).toBe(1);
    vi.advanceTimersByTime(DRAIN_INTERVAL_MS * 4); // remaining tails drain
    expect(ta.writes.join("")).toBe(big);
    expect(tb.writes.join("")).toBe(big);
  });

  it("caps the hidden backlog: drops the middle, keeps the newest tail, warns once", () => {
    const t = fakeTerm();
    let dropped = 0;
    const chunk = "y".repeat(256 * 1024);
    for (let i = 0; i < 10; i++) {
      // 9th push crosses the 2MB cap → backlog replaced; 10th queues after it
      writeScheduled("a", t, chunk, {
        visible: false,
        onBacklogDropped: () => dropped++,
      });
    }
    expect(dropped).toBe(1); // notified exactly once
    flushTerminalQueue("a");
    // lossy semantics: lose the MIDDLE, keep the warning + the newest tail
    expect(t.writes.join("")).toBe(BACKLOG_WARNING + chunk);
    expect(terminalSchedulerStats().droppedBacklogs).toBe(1);
  });

  it("flushTerminalQueue writes everything queued immediately", () => {
    const t = fakeTerm();
    writeScheduled("a", t, "hello ", { visible: false });
    writeScheduled("a", t, "world", { visible: false });
    flushTerminalQueue("a");
    expect(t.writes.join("")).toBe("hello world");
    vi.advanceTimersByTime(1000);
    expect(t.writes.join("")).toBe("hello world"); // nothing left for the drain
  });

  it("discardTerminalQueue drops silently", () => {
    const t = fakeTerm();
    writeScheduled("a", t, "gone", { visible: false });
    discardTerminalQueue("a");
    vi.advanceTimersByTime(1000);
    expect(t.writes).toEqual([]);
  });

  it("propagates onParsed for both direct and drained writes", () => {
    const t = fakeTerm();
    let parsed = 0;
    writeScheduled("a", t, "h", { visible: false, onParsed: () => parsed++ });
    vi.advanceTimersByTime(HIDDEN_FIRST_FLUSH_DELAY_MS + 1);
    writeScheduled("a", t, "v", { visible: true, onParsed: () => parsed++ });
    expect(parsed).toBe(2);
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `pnpm test`
Expected: FAIL — module `./terminal-output-scheduler` not found.

- [ ] **Step 3: Implement the scheduler**

Create `src/app/terminal-output-scheduler.ts`:

```ts
import { signal } from "@angular/core";

/**
 * Shared write scheduler for all agent terminals. Why one scheduler instead of
 * per-pane writes: every xterm.write schedules that terminal's own parse work
 * on the renderer thread, so N flooding background agents starve the focused
 * terminal. Visible writes go direct (latency path); hidden writes drain
 * round-robin at a bounded rate, with a lossy cap so a noisy background agent
 * can never pin unbounded memory in xterm write buffers.
 */
export type SchedulerTerminal = { write(data: string, callback?: () => void): void };

type WriteOptions = {
  visible: boolean;
  onParsed?: () => void;
  onBacklogDropped?: () => void;
};

type Entry = {
  term: SchedulerTerminal;
  chunks: string[];
  queuedChars: number;
  dropped: boolean;
  onParsed?: () => void;
  onBacklogDropped?: () => void;
};

export const HIDDEN_FIRST_FLUSH_DELAY_MS = 50;
export const DRAIN_INTERVAL_MS = 16;
export const DRAIN_CHUNK_CHARS = 16 * 1024;
export const MAX_WRITES_PER_DRAIN = 2;
export const MAX_HIDDEN_QUEUE_CHARS = 2 * 1024 * 1024;
export const MAX_HIDDEN_QUEUE_CHUNKS = 4096;
// Why CAN (\x18) first: it aborts a partial escape sequence from the dropped
// data so the style reset + warning text cannot be swallowed by a half CSI.
export const BACKLOG_WARNING =
  "\x18\x1b[0m\r\n\x1b[2m[hidden output skipped: backlog exceeded 2 MB]\x1b[0m\r\n";

export type TerminalSchedulerStats = {
  directWrites: number;
  drainedWrites: number;
  queuedChars: number;
  peakQueuedChars: number;
  droppedBacklogs: number;
};

const ZERO_STATS: TerminalSchedulerStats = {
  directWrites: 0,
  drainedWrites: 0,
  queuedChars: 0,
  peakQueuedChars: 0,
  droppedBacklogs: 0,
};

const queues = new Map<string, Entry>();
let drainTimer: ReturnType<typeof setTimeout> | null = null;
const stats = signal<TerminalSchedulerStats>({ ...ZERO_STATS });
/** Live scheduler counters (dev panel perf footer). */
export const terminalSchedulerStats = stats.asReadonly();

function totalQueuedChars(): number {
  let n = 0;
  for (const e of queues.values()) n += e.queuedChars;
  return n;
}

function bumpStats(patch: Partial<TerminalSchedulerStats>): void {
  stats.update((s) => {
    const queuedChars = totalQueuedChars();
    return {
      ...s,
      ...patch,
      queuedChars,
      peakQueuedChars: Math.max(s.peakQueuedChars, queuedChars),
    };
  });
}

function scheduleDrain(delayMs: number): void {
  if (drainTimer !== null || queues.size === 0) return;
  drainTimer = setTimeout(drain, delayMs);
}

/** Up to DRAIN_CHUNK_CHARS from the front of the queue (may overshoot by one chunk). */
function takeChunk(entry: Entry): string {
  let data = "";
  while (entry.chunks.length && data.length < DRAIN_CHUNK_CHARS) {
    data += entry.chunks.shift()!;
  }
  entry.queuedChars = Math.max(0, entry.queuedChars - data.length);
  return data;
}

function drain(): void {
  drainTimer = null;
  let writes = 0;
  let drained = 0;
  while (queues.size > 0 && writes < MAX_WRITES_PER_DRAIN) {
    const next = queues.entries().next().value as [string, Entry];
    const [id, entry] = next;
    queues.delete(id);
    const data = takeChunk(entry);
    if (data) {
      try {
        entry.term.write(data, entry.onParsed);
        drained++;
      } catch {
        // terminal disposed mid-drain — drop its remaining backlog
        entry.chunks.length = 0;
        entry.queuedChars = 0;
      }
      writes++;
    }
    if (entry.chunks.length) {
      queues.set(id, entry); // re-insert at the END → round-robin across terminals
    } else {
      entry.dropped = false;
    }
  }
  bumpStats({ drainedWrites: stats().drainedWrites + drained });
  scheduleDrain(DRAIN_INTERVAL_MS);
}

/** Route one PTY chunk: visible → direct (after any queued backlog, to keep
 *  byte order); hidden → queue for the paced drain. */
export function writeScheduled(
  id: string,
  term: SchedulerTerminal,
  chunk: string,
  opts: WriteOptions,
): void {
  if (!chunk) return;
  if (opts.visible) {
    flushTerminalQueue(id);
    try {
      term.write(chunk, opts.onParsed);
    } catch {
      // disposed terminal — a late chunk racing dispose() is not an error
    }
    bumpStats({ directWrites: stats().directWrites + 1 });
    return;
  }
  let entry = queues.get(id);
  if (!entry) {
    entry = { term, chunks: [], queuedChars: 0, dropped: false };
    queues.set(id, entry);
    scheduleDrain(HIDDEN_FIRST_FLUSH_DELAY_MS);
  }
  entry.term = term;
  entry.onParsed = opts.onParsed;
  entry.onBacklogDropped = opts.onBacklogDropped;
  entry.chunks.push(chunk);
  entry.queuedChars += chunk.length;
  if (
    entry.queuedChars > MAX_HIDDEN_QUEUE_CHARS ||
    entry.chunks.length > MAX_HIDDEN_QUEUE_CHUNKS
  ) {
    // Why lossy: a hidden webview can throttle timers while PTYs keep writing;
    // retaining unbounded backlog would grow until the app stalls. We lose the
    // MIDDLE and keep accepting newer output (warning + newest tail survive).
    // Phase 2 (Rust-owned snapshot) makes the drop fully recoverable.
    const notify = !entry.dropped;
    entry.chunks = [BACKLOG_WARNING];
    entry.queuedChars = BACKLOG_WARNING.length;
    entry.dropped = true;
    bumpStats({
      droppedBacklogs: stats().droppedBacklogs + (notify ? 1 : 0),
    });
    if (notify) entry.onBacklogDropped?.();
    return;
  }
  bumpStats({});
}

/** Synchronously hand the whole queued backlog to xterm (visibility resume,
 *  pre-exit-notice ordering). Bounded by the 2MB cap; xterm parses async. */
export function flushTerminalQueue(id: string): void {
  const entry = queues.get(id);
  if (!entry) return;
  queues.delete(id);
  try {
    while (entry.chunks.length) {
      entry.term.write(takeChunk(entry), entry.onParsed);
    }
  } catch {
    // disposed mid-flush — drop the rest
  }
  entry.dropped = false;
  if (queues.size === 0 && drainTimer !== null) {
    clearTimeout(drainTimer);
    drainTimer = null;
  }
  bumpStats({});
}

/** Drop an agent's queued output without writing (terminal disposal). */
export function discardTerminalQueue(id: string): void {
  queues.delete(id);
  if (queues.size === 0 && drainTimer !== null) {
    clearTimeout(drainTimer);
    drainTimer = null;
  }
  bumpStats({});
}

export function resetTerminalSchedulerForTests(): void {
  queues.clear();
  if (drainTimer !== null) {
    clearTimeout(drainTimer);
    drainTimer = null;
  }
  stats.set({ ...ZERO_STATS });
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `pnpm test`
Expected: all 7 scheduler tests PASS (and the rest of the suite untouched).

- [ ] **Step 5: Commit**

```bash
git add src/app/terminal-output-scheduler.ts src/app/terminal-output-scheduler.spec.ts
git commit -m "feat(terminal): shared write scheduler with lossy hidden cap

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Route TerminalService through the scheduler + per-agent revision signals

**Files:**
- Modify: `src/app/terminal.service.ts`
- Modify: `src/app/terminal.service.spec.ts`

- [ ] **Step 1: Update the spec's flush helper (writes are now scheduler-paced for unattached terminals)**

In `src/app/terminal.service.spec.ts`, add the import and replace the `flush` helper:

```ts
import { flushTerminalQueue } from "./terminal-output-scheduler";
```

```ts
// `write` routes through the shared scheduler; an unattached terminal counts as
// hidden, so drain its queue first, then chain an empty write whose callback
// fires once xterm has parsed everything before it.
function flush(svc: TerminalService, id: string): Promise<void> {
  flushTerminalQueue(id);
  return new Promise((r) => {
    // @ts-expect-error reach the underlying term to chain a flush callback
    svc["handle"](id).term.write("", () => r());
  });
}
```

- [ ] **Step 2: Run the spec to verify it fails**

Run: `pnpm test`
Expected: `terminal.service.spec.ts` may still pass at this point (the service isn't routed yet and `flushTerminalQueue` on an empty queue is a no-op) — that's expected; the helper change is forward-compatible. Proceed.

- [ ] **Step 3: Route writes and convert revision to per-agent signals**

In `src/app/terminal.service.ts`:

Imports — add `WritableSignal` to the Angular import and the scheduler functions:

```ts
import { inject, Injectable, OnDestroy, signal, WritableSignal } from "@angular/core";
import {
  discardTerminalQueue,
  flushTerminalQueue,
  writeScheduled,
} from "./terminal-output-scheduler";
```

Replace the revision block (lines 44-55: `private revision` … `bumpRevision`) with:

```ts
  // Per-agent revision counters, bumped AFTER xterm finishes parsing each write
  // so the buffer is current when read. Per-agent signals (not one Record map):
  // one agent's output recomputes only ITS consumers (mini-term), not every
  // subscriber on every chunk of every agent.
  private revs = new Map<string, WritableSignal<number>>();
  private revSignal(id: string): WritableSignal<number> {
    let s = this.revs.get(id);
    if (!s) {
      s = signal(0);
      this.revs.set(id, s);
    }
    return s;
  }
  /** Current revision for one agent (reactive — changes on each parsed write). */
  revOf(id: string): number {
    return this.revSignal(id)();
  }
  private bumpRevision(id: string) {
    this.revSignal(id).update((n) => n + 1);
  }
```

(The old `readonly rev = this.revision.asReadonly()` is deleted — `revOf` is the only consumer API; verified callers: `mini-term.component.ts` only.)

Replace `write` (lines 157-162) with:

```ts
  /** Write a raw PTY chunk to the agent's terminal. Visible terminals (element
   *  in the DOM) write direct; hidden ones go through the shared scheduler so
   *  background floods can't starve the focused terminal or pin memory. */
  write(id: string, chunk: string) {
    const h = this.handle(id);
    writeScheduled(id, h.term, chunk, {
      visible: h.term.element?.isConnected === true,
      // xterm `write` cb fires once parsed — buffer is current for tail()
      onParsed: () => this.bumpRevision(id),
    });
  }
```

Update `exit` and `hint` to keep byte order with any queued backlog:

```ts
  /** Note in the terminal view that the process ended. */
  exit(id: string) {
    flushTerminalQueue(id); // queued output lands before the exit notice
    this.handles.get(id)?.term.write("\r\n\x1b[2m▪ process exited\x1b[0m\r\n");
  }

  /** A dim, non-output hint (e.g. idle state) without faking program output. */
  hint(id: string, text: string) {
    flushTerminalQueue(id);
    this.handle(id).term.write(`\x1b[2m${text}\x1b[0m\r\n`);
  }
```

In `attach`, drain the backlog as soon as the terminal is in the DOM — after the `h.term.open(el)` / `el.replaceChildren(...)` branch and before `this.loadWebgl(h)`:

```ts
    flushTerminalQueue(id); // catch up on output that queued while hidden
```

In `dispose`, before `h.term.dispose()`:

```ts
    discardTerminalQueue(id);
    this.revs.delete(id);
```

- [ ] **Step 4: Run the suite**

Run: `pnpm test`
Expected: ALL PASS — including the existing `TerminalService.tail` tests (their writes queue as hidden; the updated `flush` helper drains them).

- [ ] **Step 5: Commit**

```bash
git add src/app/terminal.service.ts src/app/terminal.service.spec.ts
git commit -m "feat(terminal): route writes through scheduler; per-agent revision signals

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Coalesce liveLogs tail updates

**Files:**
- Create: `src/app/agents/pty-tail-coalescer.ts`
- Test: `src/app/agents/pty-tail-coalescer.spec.ts`
- Modify: `src/app/agents/agent-runtime.service.ts:158-168`

- [ ] **Step 1: Write the failing tests**

Create `src/app/agents/pty-tail-coalescer.spec.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPtyTailCoalescer } from "./pty-tail-coalescer";

describe("createPtyTailCoalescer", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("batches chunks per agent into one flush per interval", () => {
    const flushes: Map<string, string>[] = [];
    const c = createPtyTailCoalescer((b) => flushes.push(b), 80);
    c.push("a", "one");
    c.push("a", "two");
    c.push("b", "x");
    expect(flushes.length).toBe(0); // nothing before the interval
    vi.advanceTimersByTime(81);
    expect(flushes.length).toBe(1);
    expect(flushes[0].get("a")).toBe("onetwo");
    expect(flushes[0].get("b")).toBe("x");
  });

  it("a later push starts a new batch", () => {
    const flushes: Map<string, string>[] = [];
    const c = createPtyTailCoalescer((b) => flushes.push(b), 80);
    c.push("a", "1");
    vi.advanceTimersByTime(81);
    c.push("a", "2");
    vi.advanceTimersByTime(81);
    expect(flushes.length).toBe(2);
    expect(flushes[1].get("a")).toBe("2");
  });

  it("dispose drops pending without flushing", () => {
    const flushes: Map<string, string>[] = [];
    const c = createPtyTailCoalescer((b) => flushes.push(b), 80);
    c.push("a", "gone");
    c.dispose();
    vi.advanceTimersByTime(1000);
    expect(flushes.length).toBe(0);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `pnpm test`
Expected: FAIL — module not found.

- [ ] **Step 3: Implement and wire**

Create `src/app/agents/pty-tail-coalescer.ts`:

```ts
/**
 * Buffers per-agent PTY chunks and flushes them as ONE batch per interval.
 * Why: updating a Record-signal per chunk publishes a new map identity to every
 * consumer on every chunk of every agent; coalescing caps that fanout at one
 * publish per interval regardless of output rate.
 */
export function createPtyTailCoalescer(
  flush: (byAgent: Map<string, string>) => void,
  intervalMs = 80,
): { push: (id: string, chunk: string) => void; dispose: () => void } {
  let pending = new Map<string, string>();
  let timer: ReturnType<typeof setTimeout> | null = null;
  const fire = (): void => {
    timer = null;
    const batch = pending;
    pending = new Map();
    if (batch.size) flush(batch);
  };
  return {
    push(id, chunk) {
      pending.set(id, (pending.get(id) ?? "") + chunk);
      timer ??= setTimeout(fire, intervalMs);
    },
    dispose() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      pending.clear();
    },
  };
}
```

In `src/app/agents/agent-runtime.service.ts`: import it, add the field, and rewire `onOutput`.

```ts
import { createPtyTailCoalescer } from "./pty-tail-coalescer";
```

Add as a class field (near the other private state, before the constructor):

```ts
  // liveLogs is a Record-signal — one publish per chunk wakes every consumer.
  // The coalescer batches all agents' chunks into one publish per 80ms.
  private tailCoalescer = createPtyTailCoalescer((byAgent) => {
    this.liveLogs.update((m) => {
      const next = { ...m };
      for (const [id, chunk] of byAgent) {
        const prev = (next[id] || []).map((l) => l.s);
        next[id] = appendPtyTail(prev, chunk).map((s) => ({ t: "out" as const, s }));
      }
      return next;
    });
  });
```

Replace the `onOutput` subscription body (lines 159-168) with:

```ts
    // stream output: raw bytes → xterm (scheduler-paced), plain-text tail →
    // liveLogs via the coalescer (one publish per 80ms, not per chunk)
    void this.agentsStore
      .onOutput((id, chunk) => {
        this.lastOutputAt[id] = Date.now();
        this.terminals.write(id, chunk);
        this.tailCoalescer.push(id, chunk);
      })
      .catch(() => {});
```

- [ ] **Step 4: Run the suite**

Run: `pnpm test`
Expected: ALL PASS.

- [ ] **Step 5: Commit**

```bash
git add src/app/agents/pty-tail-coalescer.ts src/app/agents/pty-tail-coalescer.spec.ts src/app/agents/agent-runtime.service.ts
git commit -m "perf(agents): coalesce liveLogs tail updates to one publish per 80ms

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 6: Scheduler counters in the dev panel

**Files:**
- Modify: `src/app/dev-tools/dev-panel.component.ts`

- [ ] **Step 1: Surface the stats signal in the perf footer**

Import and expose:

```ts
import { terminalSchedulerStats } from "../terminal-output-scheduler";
```

Add to the component class (next to `readonly perf = inject(PerfStore)`):

```ts
  /** Live terminal write-scheduler counters (Phase 1 pipeline visibility). */
  readonly termStats = terminalSchedulerStats;
```

In the template's perf footer block, after `<span class="tnum">{{ perfSummary() }}</span>` add:

```html
            <span class="tnum" title="terminal write scheduler: queued chars · peak · dropped backlogs">term {{ termStats().queuedChars }} · pk {{ termStats().peakQueuedChars }} · drop {{ termStats().droppedBacklogs }}</span>
```

- [ ] **Step 2: Run suite + typecheck build**

Run: `pnpm test && pnpm run build`
Expected: green; build clean.

- [ ] **Step 3: Commit**

```bash
git add src/app/dev-tools/dev-panel.component.ts
git commit -m "feat(dev-panel): terminal scheduler counters in perf footer

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 7: End-to-end flood verification

**Files:** `docs/superpowers/plans/2026-06-10-perf-roadmap.md` (docs only)

- [ ] **Step 1: Full suites**

Run: `cargo test --manifest-path src-tauri/Cargo.toml && pnpm test && pnpm run build`
Expected: all green.

- [ ] **Step 2: Live flood smoke (`pnpm dev`)**

1. Start an agent and have it flood output (e.g. run `npx -y cowsay moo & for /l %i in (1,1,50000) do @echo line %i` in its terminal, or any long verbose command).
2. DevConsole perf tab → `agent_output_emit` row: **calls/10s ≤ ~1250** while flooding (was: one event per 4KB read — typically several thousand).
3. With the flooding agent in a BACKGROUND tab and another agent focused: typing in the focused terminal stays fluid; footer `term` counter shows queued chars rising and draining; `drop` stays 0 for normal floods.
4. Force a drop: flood megabytes while the agent is hidden for a while → footer `drop` becomes 1 and switching to the agent shows the `[hidden output skipped…]` line, with live output continuing below.
5. Emoji/CJK-heavy output (e.g. `echo 你好🦀é` in a loop) renders without `�` corruption (UTF-8 boundary fix).

- [ ] **Step 3: Mark roadmap items done**

In `docs/superpowers/plans/2026-06-10-perf-roadmap.md`, annotate Phase 1 items **1, 2 (seq emitted; frontend consumption deferred to Phase 2), 3, 4, 5, 6** with `**DONE 2026-06-…**` plus the observed before/after `agent_output_emit` rates from the smoke.

- [ ] **Step 4: Commit (docs)**

```bash
git add docs/superpowers/plans/2026-06-10-perf-roadmap.md
git commit -m "docs(perf): mark PTY pipeline phase 1 done with observed rates

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Non-goals (explicitly out of scope — Phase 2/3 of the roadmap)

- **Frontend seq consumption / snapshot recovery** — the Rust payload carries `seq` from day one, but dedup-on-restore lands with the Rust-owned ring buffer (roadmap #8).
- **ACK backpressure** (roadmap #7) — builds on this batcher.
- **Interactive fast-lane bypass** (roadmap #11) — measure typing latency first; the 8ms window is likely imperceptible.
- **DEC 2026 frame holding** (roadmap #15) — only if TUI flicker is observed.
- **WebGL visible-only contexts** (roadmap #9) — separate change set.

## Risks & notes

- **Exit-notice ordering:** `agent://exit` can overtake the final output flush by ≤8ms (window) — same race as today, just marginally wider; `exit()` flushes the scheduler queue first so queued bytes always precede the notice within the renderer.
- **Mini-term freshness:** hidden terminals' buffers now lag by drain pacing (≤ ~50ms first flush; bounded drain after). The overview reads `tail()` via `revOf` — bumps fire per drained write, so previews stay live, just paced.
- **`hint()`/`exit()`** write directly to xterm (rare, small) — they flush the queue first to keep ordering.
- **Module-level scheduler state** is shared across the app (by design — that's the point); tests reset via `resetTerminalSchedulerForTests()`.
