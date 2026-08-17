import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  input,
} from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { InterestService } from "../agents/interest.service";
import { ActivityKind } from "../models";

// Fallback switch for the PTY-derived preview. When true the preview falls back
// to the agent's DIGEST lines (A0.2: last rendered rows, folded backend-side,
// pushed at 1Hz over agent://digest — no full stream reaches the renderer)
// before/without any hook activity. Needed for un-hooked tools (gemini), whose
// cards would otherwise stay empty. When false the preview is HOOKS-ONLY.
const PTY_FALLBACK = true;

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
// so tool uses a dimmed --sem-live — distinct from the brighter user stream.
const KIND_COLOR: Record<ActivityKind, string> = {
  user: "var(--sem-live)",
  agent: "var(--ink-2)",
  tool: "color-mix(in oklch, var(--sem-live), var(--ink-4) 55%)",
  success: "var(--st-done)",
  error: "var(--st-blocked)",
  question: "var(--ui-ink)",
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
    <div style="background:var(--bg);border:1px solid var(--hair);border-radius:var(--r-sm);padding:var(--sp-3) var(--sp-4);font-size:var(--fs-xs);line-height:1.6;height:60px;box-sizing:border-box;overflow:hidden">
      @for (row of rows(); track $index) {
        <div
          [style.color]="rowColor(row)"
          [style.border-left-color]="row.empty ? 'transparent' : rowColor(row)"
          style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis;border-left:2px solid transparent;padding-left:var(--sp-2)"
        >{{ row.text }}</div>
      }
    </div>
  `,
})
export class MiniTermComponent {
  private runtime = inject(AgentRuntimeService);
  private interest = inject(InterestService);
  private host = inject<ElementRef<HTMLElement>>(ElementRef);
  readonly agentId = input.required<string>();
  // The hook-driven activity detail (latest message content) as up to 3 lines,
  // each carrying its `kind` for colorizing. When present this is the preview. The
  // backend already formats each line (plain assistant text, "💭 …" thinking,
  // "▸ Tool: …" tool use), so we render it verbatim — no extra prefixing here.
  private readonly activityLines = computed<{ text: string; kind: ActivityKind }[]>(() =>
    this.runtime.activityFor(this.agentId()),
  ); // reactive: activity signal
  // Fallback: the agent's DIGEST lines (A0.2) — the last rendered rows folded
  // in Rust and pushed at 1Hz while this card is in the viewport (this card's
  // visibility registration below is exactly what puts the agent in digest
  // mode). The full stream never reaches the renderer for preview-only agents.
  // No kind (null) → rendered as default stream text.
  private readonly tailLines = computed<{ text: string; kind: null }[]>(() =>
    this.runtime
      .digestFor(this.agentId())
      .slice(-3)
      .map((text) => ({ text, kind: null })),
  );
  // Prefer the activity feed. With PTY_FALLBACK on, fall back to the digest
  // when no hook has fired; otherwise (hooks-only) return empty so the "no output
  // yet" row shows until hooks reach us.
  private readonly lines = computed<{ text: string; kind: ActivityKind | null }[]>(() => {
    const acts = this.activityLines();
    if (acts.length) return acts;
    return PTY_FALLBACK ? this.tailLines() : [];
  });
  // Which source drove the current preview: 'hook' (transcript activity), 'pty'
  // (backend digest), or 'none' (nothing yet, hooks-only). Debug aid only —
  // never rendered. With hooks-only this reads 'none' when nothing arrives, making
  // it obvious hooks aren't reaching the card.
  private readonly source = computed<"hook" | "pty" | "none">(() =>
    this.activityLines().length ? "hook" : PTY_FALLBACK ? "pty" : "none",
  );

  constructor() {
    // A0.2: while this card is in the viewport its agent belongs to the digest
    // set (recomputed on overview scroll); off-screen cards drop to none.
    // Registered from an effect (not directly) because inputs aren't readable
    // until after construction; re-registers if the card is re-bound.
    let unobserve: (() => void) | null = null;
    effect(() => {
      const id = this.agentId();
      unobserve?.();
      unobserve = this.interest.observeCard(this.host.nativeElement, id);
    });
    inject(DestroyRef).onDestroy(() => unobserve?.());
    // Debug-only side effect: log the preview content + its driving source on
    // every update. Reads the activity signal so it re-fires on each change,
    // exactly like the preview. console only — no UI tag.
    effect(() => {
      const id = this.agentId();
      const acts = this.runtime.activityFor(id); // reactive dep: activity signal
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
