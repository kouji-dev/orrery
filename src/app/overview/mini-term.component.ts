import { ChangeDetectionStrategy, Component, computed, effect, inject, input } from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ActivityKind } from "../models";
import { TerminalService } from "../terminal.service";

// Single switch to re-enable the PTY buffer-tail fallback. When false the preview
// is HOOKS-ONLY (transcript activity); when true it falls back to the xterm buffer
// tail (last 3 rendered rows) before/without any hook. All the PTY/tail code below
// is retained either way — flip this back to `true` to restore the fallback.
const PTY_FALLBACK = false;

// One preview row: its single collapsed line of text + the activity `kind` that
// drives its color. Fallback (PTY tail) rows have no kind → rendered as default
// stream text.
interface Row {
  text: string;
  kind: ActivityKind | null;
  empty: boolean;
}

// Per-kind row color. user=cyan, agent=default stream, tool=dimmed cyan (info),
// success=green, error=red, question=purple, info=muted. No --info token exists,
// so tool uses a dimmed --accent-2 — distinct from the brighter user cyan.
const KIND_COLOR: Record<ActivityKind, string> = {
  user: "var(--accent-2)",
  agent: "var(--ink-2)",
  tool: "color-mix(in oklch, var(--accent-2), var(--ink-4) 55%)",
  success: "var(--st-done)",
  error: "var(--st-blocked)",
  question: "var(--accent)",
  info: "var(--ink-4)",
};

/**
 * Overview-card preview for an agent. Primary source is the HOOK-DRIVEN ACTIVITY
 * detail — the LATEST message content scraped from the agent's transcript (the
 * assistant's text, thinking "💭 …", or tool use "▸ Bash: …"), mirroring the
 * "current message" you'd see in the live terminal. Works even for full-screen
 * TUIs that pin their chrome to the bottom (so the raw buffer tail shows only the
 * input box). Falls back to the last 3 rendered terminal rows for un-hooked tools
 * (gemini) or before the first hook fires.
 *
 * Each of the (up to) 3 rows is colorized by its activity `kind` — and carries a
 * 2px left accent border tinted the same — so the user/agent/tool/success/error/
 * question/info distinction reads at a glance on the dark bg.
 */
@Component({
  selector: "app-mini-term",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <!-- Fixed height = 3 rows (10px * 1.6 = 16px each = 48px) + 12px vertical padding = 60px.
         Always render exactly 3 rows so idle and streaming cards are pixel-identical. -->
    <div style="background:var(--bg);border:1px solid var(--hair);border-radius:var(--r-sm);padding:6px 8px;font-size:10px;line-height:1.6;height:60px;box-sizing:border-box;overflow:hidden">
      @for (row of rows(); track $index) {
        <div
          [style.color]="rowColor(row)"
          [style.border-left-color]="row.empty ? 'transparent' : rowColor(row)"
          style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis;border-left:2px solid transparent;padding-left:5px"
        >{{ row.text }}</div>
      }
    </div>
  `,
})
export class MiniTermComponent {
  private terminals = inject(TerminalService);
  private runtime = inject(AgentRuntimeService);
  readonly agentId = input.required<string>();
  // The hook-driven activity detail (latest message content) as up to 3 lines,
  // each carrying its `kind` for colorizing. When present this is the preview. The
  // backend already formats each line (plain assistant text, "💭 …" thinking,
  // "▸ Tool: …" tool use), so we render it verbatim — no extra prefixing here.
  private readonly activityLines = computed<{ text: string; kind: ActivityKind }[]>(() =>
    this.runtime.activityFor(this.agentId()),
  ); // reactive: activity signal
  // Fallback: the last few rows straight from the authoritative xterm buffer —
  // the same fully-rendered text the user sees in the full terminal. Depending on
  // this agent's revision (bumped after each parsed write) keeps it live; the
  // "process exited" line is written into xterm too, so it shows up naturally.
  // No kind (null) → rendered as default stream text.
  private readonly tailLines = computed<{ text: string; kind: null }[]>(() => {
    const id = this.agentId();
    this.terminals.revOf(id); // reactive dep: re-read on each new parsed write
    return this.terminals.tail(id, 3).map((text) => ({ text, kind: null }));
  });
  // Prefer the activity feed. With PTY_FALLBACK on, fall back to the buffer tail
  // when no hook has fired; otherwise (hooks-only) return empty so the "no output
  // yet" row shows until hooks reach us.
  private readonly lines = computed<{ text: string; kind: ActivityKind | null }[]>(() => {
    const acts = this.activityLines();
    if (acts.length) return acts;
    return PTY_FALLBACK ? this.tailLines() : [];
  });
  // Which source drove the current preview: 'hook' (transcript activity), 'pty'
  // (xterm buffer tail), or 'none' (nothing yet, hooks-only). Debug aid only —
  // never rendered. With hooks-only this reads 'none' when nothing arrives, making
  // it obvious hooks aren't reaching the card.
  private readonly source = computed<"hook" | "pty" | "none">(() =>
    this.activityLines().length ? "hook" : PTY_FALLBACK ? "pty" : "none",
  );

  constructor() {
    // Debug-only side effect: log the preview content + its driving source on
    // every update. Reads the activity signal and terminals.revOf(id) so it
    // re-fires on each change, exactly like the preview. console only — no UI tag.
    effect(() => {
      const id = this.agentId();
      const acts = this.runtime.activityFor(id); // reactive dep: activity signal
      this.terminals.revOf(id); // reactive dep: re-read on each new parsed write
      // Only log when there IS activity — skip the 'none' case entirely so the
      // shared activity signal re-running every card's effect doesn't spam.
      if (!acts.length) return;
      const latestEvent = this.runtime.latestEvent(id);
      console.debug(
        "[mini-term]",
        id,
        "source=hook(" + (latestEvent || "?") + ")",
        this.lines(),
      );
    });
  }
  // Always exactly 3 rows. Pad missing rows with a non-breaking space so each
  // keeps its line-height and the container height never changes. When there's
  // no output yet, "no output yet" sits in row 1 (dim) and rows 2-3 stay blank.
  readonly rows = computed<Row[]>(() => {
    const ls = this.lines();
    const base: { text: string; kind: ActivityKind | null }[] = ls.length
      ? ls
      : [{ text: "no output yet", kind: null }];
    return Array.from({ length: 3 }, (_, i): Row => {
      const r = base[i];
      return r != null
        ? { text: r.text, kind: r.kind, empty: !ls.length }
        : { text: " ", kind: null, empty: true };
    });
  });

  /** Row color: the per-kind tint when a kind is present and the row isn't the
   *  empty "no output yet" placeholder; muted otherwise. PTY-tail rows (no kind)
   *  use the default stream color. */
  rowColor(row: Row): string {
    if (row.empty) return "var(--ink-4)";
    return row.kind ? KIND_COLOR[row.kind] : "var(--ink-2)";
  }
}
