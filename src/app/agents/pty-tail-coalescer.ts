/**
 * Buffers per-agent PTY chunks and flushes them as ONE batch per interval.
 * Why: updating a Record-signal per chunk publishes a new map identity to every
 * consumer on every chunk of every agent; coalescing caps that fanout at one
 * publish per interval regardless of output rate.
 */
export function createPtyTailCoalescer(
  flush: (byAgent: Map<string, string>) => void,
  intervalMs = 80,
): { push: (id: string, chunk: string) => void; flush: () => void; dispose: () => void } {
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
    flush() {
      if (timer !== null) clearTimeout(timer);
      fire();
    },
    dispose() {
      if (timer !== null) clearTimeout(timer);
      timer = null;
      pending.clear();
    },
  };
}
