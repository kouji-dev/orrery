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
