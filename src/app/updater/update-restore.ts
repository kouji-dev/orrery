// Terminal continuation across an in-app update relaunch. "Install & relaunch"
// exits the process, which tears every PTY down gracefully (reset_running) —
// so the backend's crash-only InterruptedAgents path never fires and the
// restarted app would come up with dead terminals.
//
// The workspace LAYOUT (tabs/panes) needs no snapshot here: UiStore persists
// it continuously and restores it on every launch. This module only carries
// WHICH agents had a live terminal, so the relaunch continues their recorded
// CLI sessions (`--resume`) even when the `autoResume` setting is off.
//
// SettingsStore.install() writes the list right before UpdateInstall; UiStore
// drains it (one-shot) at construction and AgentRuntimeService acts on it.

const KEY = "orrery:update-restore";
/** A list older than this is a failed install that never relaunched. */
const MAX_AGE_MS = 15 * 60_000;

interface UpdateResume {
  v: 1;
  at: number;
  /** Agents with a live PTY at install time. */
  resume: string[];
}

export function saveUpdateResume(resume: string[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify({ v: 1, at: Date.now(), resume }));
  } catch {
    /* storage unavailable — the relaunch just starts clean */
  }
}

export function clearUpdateResume(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    /* ignore */
  }
}

/** One-shot drain: read + remove. Empty when absent, malformed, or stale. */
export function drainUpdateResume(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    localStorage.removeItem(KEY);
    const s = JSON.parse(raw) as UpdateResume;
    if (s?.v !== 1 || !Array.isArray(s.resume)) return [];
    if (Date.now() - s.at > MAX_AGE_MS) return [];
    return s.resume;
  } catch {
    return [];
  }
}
