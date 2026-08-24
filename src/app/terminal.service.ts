import { DestroyRef, inject, Injectable, signal, WritableSignal } from "@angular/core";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { FitAddon } from "@xterm/addon-fit";
import { ISearchOptions, SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { ITheme, Terminal } from "@xterm/xterm";
import { AgentsStore } from "./stores/agents.store";
import { codeMetrics } from "./ui/density";
import {
  discardTerminalQueue,
  flushTerminalQueue,
  writeScheduled,
} from "./terminal-output-scheduler";

interface TermHandle {
  term: Terminal;
  fit: FitAddon;
  search: SearchAddon;
  ro?: ResizeObserver;
  /** WebGL renderer is loaded once, after the terminal is first opened in the DOM. */
  webglLoaded?: boolean;
  /** Focus listener is hooked once, after the terminal first has a textarea (post-open). */
  focusHooked?: boolean;
}

const MONO =
  '"JetBrains Mono", "Cascadia Code", ui-monospace, "SF Mono", Menlo, Consolas, monospace';

/** Leading ratio for the terminal, held flat across densities (see handle()). */
const TERM_LINE_HEIGHT = 1.25;

/**
 * Owns one persistent xterm `Terminal` per agent. The instance outlives any
 * single view: output is written to it as it streams (even while its pane is
 * hidden), and the DOM is re-parented into whichever host attaches. Keystrokes
 * and resizes are forwarded to the agent's PTY through the backend.
 */
@Injectable({ providedIn: "root" })
export class TerminalService {
  private agents = inject(AgentsStore);
  private handles = new Map<string, TermHandle>();
  private titleCb?: (id: string, title: string) => void;

  constructor() {
    // Root-service teardown (app shutdown / test env): dispose every terminal.
    inject(DestroyRef).onDestroy(() => {
      for (const id of [...this.handles.keys()]) this.dispose(id);
    });
  }

  /** Re-apply the current app theme to every live terminal. Called by the terminal
   *  component when the theme toggles — xterm only reads its theme at attach time,
   *  so without this an already-open terminal keeps the old theme until re-mounted. */
  retheme() {
    const theme = this.theme();
    for (const h of this.handles.values()) h.term.options.theme = theme;
  }

  /** Re-apply the current density's code metrics to every live terminal.
   *  xterm reads fontSize once at construction, so without this a density switch
   *  leaves already-open terminals at the old glyph size until they are disposed.
   *  Changing the glyph size changes how many cols/rows fit, so each terminal is
   *  refit and the new geometry pushed to its PTY — otherwise the process keeps
   *  wrapping to the old width and TUIs render corrupt. */
  redensify() {
    const { fontSize } = codeMetrics();
    for (const [id, h] of this.handles) {
      if (h.term.options.fontSize === fontSize) continue;
      h.term.options.fontSize = fontSize;
      if (!h.term.element) continue; // not mounted — attach() will fit it
      this.refit(h);
      void this.agents.resize(id, h.term.rows, h.term.cols).catch(() => {});
    }
  }

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
  private bumpRevision(id: string) {
    this.revSignal(id).update((n) => n + 1);
  }

  // The agent whose xterm last GAINED focus — i.e. the terminal the user types
  // in. LOCAL bookkeeping only since A0.2: the backend no longer has a
  // single-focus fast path (`agent_focus` is superseded by the interest
  // subscription — every VISIBLE terminal pane is `stream` mode and drains
  // per frame). Kept for the drop-target fallback (`focusedAgentId`). It is
  // cleared when the focused agent's process exits or its terminal is
  // disposed — NOT on blur.
  private sentFocusId: string | null = null;
  private setFocused(id: string | null) {
    this.sentFocusId = id;
  }

  /** The agent whose terminal currently holds the typing focus (fast-path), or
   *  null. Used as the fallback target when a file is dropped somewhere that
   *  isn't over a specific terminal. */
  focusedAgentId(): string | null {
    return this.sentFocusId;
  }

  /** Paste text into an agent's terminal — it flows to the PTY through the same
   *  `onData` path as a keyboard paste (bracketed-paste aware). No-op (returns
   *  false) if that agent has no live terminal. */
  pasteToAgent(id: string, text: string): boolean {
    const h = this.handles.get(id);
    if (!h) return false;
    h.term.paste(text);
    h.term.focus();
    return true;
  }

  // Per-agent live search result { index (0-based, -1 = none), count } — bumped by
  // the SearchAddon's onDidChangeResults so the search box can show "n / total".
  private searchRes = signal<Record<string, { index: number; count: number }>>(
    {},
  );
  readonly searchResults = this.searchRes.asReadonly();

  /** Subscribe to OSC window-title changes from any agent's terminal. */
  onTitle(cb: (id: string, title: string) => void) {
    this.titleCb = cb;
  }

  /** Get — lazily creating — the persistent terminal for an agent. */
  private handle(id: string): TermHandle {
    let h = this.handles.get(id);
    if (!h) {
      const term = new Terminal({
        allowProposedApi: true,
        cursorBlink: true,
        fontFamily: MONO,
        fontSize: codeMetrics().fontSize,
        // xterm's lineHeight is a RATIO, not px. It stays constant across
        // densities on purpose: density moves the glyph size, and the terminal
        // keeps the same leading at every size.
        lineHeight: TERM_LINE_HEIGHT,
        scrollback: 5000,
        theme: this.theme(),
      });
      const fit = new FitAddon();
      term.loadAddon(fit);

      // Open clickable URLs in the system browser via Tauri's opener — a plain
      // window.open is blocked in the webview, so route through the opener plugin.
      term.loadAddon(
        new WebLinksAddon((_event, uri) => {
          void import("@tauri-apps/plugin-opener")
            .then((m) => m.openUrl(uri))
            .catch(() => {});
        }),
      );

      // Correct wide-char/emoji/box-drawing widths for the agent TUIs.
      term.loadAddon(new Unicode11Addon());
      term.unicode.activeVersion = "11";

      // OSC 52 clipboard: lets programs running in the terminal (agent CLIs, vim,
      // tmux…) read/write the system clipboard via escape sequences. Complements
      // the Ctrl/Cmd+Shift+C/V user copy/paste keybindings below.
      term.loadAddon(new ClipboardAddon());

      // In-buffer search; the component drives it via findNext/findPrevious/clearSearch.
      const search = new SearchAddon();
      term.loadAddon(search);
      // surface match position/count so the search box can show "n / total"
      search.onDidChangeResults((r) =>
        this.searchRes.update((m) => ({
          ...m,
          [id]: { index: r.resultIndex, count: r.resultCount },
        })),
      );

      // Clipboard. xterm renders to a canvas, so the native browser copy can't see
      // the selection — we do it ourselves and preventDefault so the webview's own
      // Ctrl+C/V handling doesn't also fire.
      //  • Copy:  Ctrl/Cmd+Shift+C, or Ctrl/Cmd+C WHEN there's a selection
      //           (a bare Ctrl+C with no selection still passes ^C through to interrupt).
      //  • Paste: Ctrl/Cmd+V or Ctrl/Cmd+Shift+V.
      term.attachCustomKeyEventHandler((e) => {
        if (e.type !== "keydown") return true;
        // Shift/Ctrl+Enter → newline, not submit. xterm would send a plain CR
        // (the same as Enter), so the agent CLI submits the message. Backslash
        // + CR is the sequence Claude Code's own /terminal-setup binds for
        // "insert a line break"; plain Enter still submits.
        if (e.key === "Enter" && (e.shiftKey || e.ctrlKey) && !e.altKey && !e.metaKey) {
          void this.agents.input(id, "\\\r").catch(() => {});
          e.preventDefault();
          return false;
        }
        const mod = e.ctrlKey || e.metaKey;
        if (!mod) return true;
        const k = e.key.toLowerCase();
        if (k === "c" && (e.shiftKey || term.hasSelection())) {
          const sel = term.getSelection();
          if (sel) void navigator.clipboard.writeText(sel).catch(() => {});
          e.preventDefault();
          return false;
        }
        if (k === "v") {
          void navigator.clipboard
            .readText()
            .then((t) => {
              if (t) term.paste(t);
            })
            .catch(() => {});
          e.preventDefault();
          return false;
        }
        return true;
      });

      term.onData((data) => void this.agents.input(id, data).catch(() => {}));
      term.onResize(
        ({ cols, rows }) =>
          void this.agents.resize(id, rows, cols).catch(() => {}),
      );
      term.onTitleChange((title) => this.titleCb?.(id, title)); // live agent state
      h = { term, fit, search };
      this.handles.set(id, h);
    }
    return h;
  }

  // ── A1.2 snapshot recovery ────────────────────────────────────────────────
  // Live chunks with seq <= minSeq are already inside a replayed snapshot —
  // writing them again would duplicate output. Set by recover(); reset on
  // dispose (a fresh terminal starts from the live stream again).
  private minSeq = new Map<string, number>();
  // Agents whose terminal went stale (dropped out of `stream` interest while
  // hidden — chunks were never shipped). Their next attach() replays the
  // backend scrollback snapshot instead of resuming mid-stream.
  private stale = new Set<string>();
  // While a recovery is in flight, live chunks are parked here instead of the
  // terminal — they would land BEFORE the clear+replay and be lost. recover()
  // flushes them (seq-deduped) after the snapshot is written.
  private recovering = new Map<string, { chunk: string; seq?: number }[]>();

  /** Mark an agent's terminal stale (its live stream was interrupted — e.g. it
   *  left `stream` interest mode). The next attach() replays the snapshot. */
  markStale(id: string) {
    this.stale.add(id);
  }

  /** Replay the snapshot NOW when the terminal is stale but still mounted —
   *  the re-enter-stream-without-re-attach case (window minimize/restore
   *  cycle: hidden → digest → shown → stream, terminal never detached). */
  recoverIfStale(id: string) {
    if (this.stale.has(id) && this.handles.get(id)?.term.element?.isConnected) {
      void this.recover(id);
    }
  }

  /** Write a raw PTY chunk to the agent's terminal. Visible terminals (element
   *  in the DOM) write direct; hidden ones go through the shared scheduler so
   *  background floods can't starve the focused terminal or pin memory.
   *  `seq` (the backend's cumulative byte count) gates snapshot dedup: chunks
   *  at or below the last replayed snapshot's endSeq are silently dropped. */
  write(id: string, chunk: string, seq?: number) {
    const parked = this.recovering.get(id);
    if (parked) {
      parked.push({ chunk, seq });
      return;
    }
    if (seq !== undefined && seq <= (this.minSeq.get(id) ?? -1)) return;
    const h = this.handle(id);
    writeScheduled(id, h.term, chunk, {
      visible: h.term.element?.isConnected === true,
      // xterm `write` cb fires once parsed — buffer is current for tail()
      onParsed: () => this.bumpRevision(id),
    });
  }

  /**
   * A1.2 recovery: replace the terminal's content with the backend scrollback
   * snapshot. Runs when a stale/hidden terminal becomes visible again or after
   * a webview reload (fresh terminal for a known agent): discard the queued
   * hidden backlog → term.clear() → replay the snapshot → drop live chunks
   * with seq <= endSeq (they're inside the snapshot) → resume the stream.
   * Live chunks arriving mid-recovery are parked and flushed after the replay
   * so nothing can land between clear and snapshot.
   */
  private async recover(id: string): Promise<void> {
    if (this.recovering.has(id)) return; // one recovery at a time
    this.stale.delete(id);
    this.recovering.set(id, []);
    try {
      const snap = await this.agents.snapshot(id);
      const h = this.handles.get(id);
      if (!h) return;
      discardTerminalQueue(id); // queued pre-snapshot backlog is inside `text`
      // Why reset(), not clear(): xterm's clear() deliberately KEEPS the
      // current (bottom) line — an un-terminated live chunk would survive the
      // wipe and the snapshot would concatenate onto it. reset() (full RIS)
      // also drops parser/SGR state, which must not leak into the replay.
      h.term.reset();
      if (snap.text) h.term.write(snap.text, () => this.bumpRevision(id));
      this.minSeq.set(id, snap.endSeq);
    } catch {
      this.stale.add(id); // backend unavailable — retry on the next attach
    } finally {
      const parked = this.recovering.get(id) ?? [];
      this.recovering.delete(id);
      for (const p of parked) this.write(id, p.chunk, p.seq);
    }
  }

  /**
   * Last `n` NON-EMPTY rendered rows from an agent's terminal buffer — the
   * authoritative, fully ANSI/VT-interpreted text the user sees in the full
   * view. Returns [] if no terminal exists yet. No box-drawing stripping: the
   * buffer already holds the final rendered frame, so what's here is real
   * content (a stale TUI frame can't linger behind the live one).
   */
  tail(id: string, n: number): string[] {
    const h = this.handles.get(id);
    if (!h) return [];
    const buf = h.term.buffer.active;
    const out: string[] = [];
    const total = buf.baseY + h.term.rows; // scrollback base + visible rows
    for (let i = 0; i < total; i++) {
      const s = buf.getLine(i)?.translateToString(true).trim();
      if (s) out.push(s);
    }
    return out.slice(-n);
  }

  /** Current fitted size of an agent's terminal, if one exists (no side effects). */
  size(id: string): { rows: number; cols: number } | null {
    const h = this.handles.get(id);
    return h ? { rows: h.term.rows, cols: h.term.cols } : null;
  }

  /** Push the terminal's current size to the PTY (call once the process is running). */
  syncSize(id: string) {
    const h = this.handles.get(id);
    if (!h) return;
    this.refit(h);
    void this.agents.resize(id, h.term.rows, h.term.cols).catch(() => {});
    // Restart while the terminal kept DOM focus: exit() released the mux fast
    // path, and with no blur in between the textarea's focus listener never
    // re-fires — re-claim explicitly when we are still the active element.
    if (h.term.textarea && document.activeElement === h.term.textarea) {
      this.setFocused(id);
    }
  }

  /** Note in the terminal view that the process ended. */
  exit(id: string) {
    flushTerminalQueue(id); // queued output lands before the exit notice
    this.handles.get(id)?.term.write("\r\n\x1b[2m▪ process exited\x1b[0m\r\n");
    // the focused agent stopped → release the mux fast path (clicking back
    // into the terminal after a restart re-claims it via the focus listener)
    if (this.sentFocusId === id) this.setFocused(null);
  }

  /** A dim, non-output hint (e.g. idle state) without faking program output. */
  hint(id: string, text: string) {
    flushTerminalQueue(id);
    this.handle(id).term.write(`\x1b[2m${text}\x1b[0m\r\n`);
  }

  // ── Search ────────────────────────────────────────────────────────────────
  // Translucent accent tints (mirroring --ui-sel / --ui-sel-2) so the matched
  // text keeps its own ANSI color and stays legible on BOTH themes — a solid
  // accent fill drowned the glyphs, worst on the light paper ramp.
  private searchDecorations(): ISearchOptions["decorations"] {
    const cs = getComputedStyle(document.documentElement);
    const accent = cs.getPropertyValue("--accent").trim() || "#a855f7";
    const rgb = cs.getPropertyValue("--accent-rgb").trim() || "168, 85, 247";
    return {
      matchBackground: `rgba(${rgb}, 0.28)`,
      matchBorder: `rgba(${rgb}, 0.55)`,
      matchOverviewRuler: accent,
      activeMatchBackground: `rgba(${rgb}, 0.5)`,
      activeMatchBorder: accent,
      activeMatchColorOverviewRuler: accent,
    };
  }

  /** Find the next match for `query` (no-op if the terminal doesn't exist). */
  findNext(id: string, query: string, opts?: ISearchOptions): boolean {
    const h = this.handles.get(id);
    if (!h) return false;
    return h.search.findNext(query, {
      decorations: this.searchDecorations(),
      ...opts,
    });
  }

  /** Find the previous match for `query` (no-op if the terminal doesn't exist). */
  findPrevious(id: string, query: string, opts?: ISearchOptions): boolean {
    const h = this.handles.get(id);
    if (!h) return false;
    return h.search.findPrevious(query, {
      decorations: this.searchDecorations(),
      ...opts,
    });
  }

  /** Clear search highlight decorations (no-op if the terminal doesn't exist). */
  clearSearch(id: string) {
    this.handles.get(id)?.search.clearDecorations();
  }

  /** Mount the agent's terminal into `el`, sizing it to fit; returns a detach fn. */
  attach(id: string, el: HTMLElement): () => void {
    // A fresh handle for an agent means the webview (re)loaded — the xterm
    // buffer is empty while the backend ring still holds the history. Either
    // that or an explicit stale mark (the live stream was interrupted while
    // hidden) triggers the A1.2 snapshot replay.
    const fresh = !this.handles.has(id);
    const h = this.handle(id);
    if (fresh || this.stale.has(id)) void this.recover(id);
    h.term.options.theme = this.theme(); // pick up the current light/dark theme
    // Same for density: a terminal that streamed while hidden missed the
    // redensify() broadcast (that fires from a MOUNTED terminal component), so
    // re-read the code metrics on the way in. The refit below sizes it.
    h.term.options.fontSize = codeMetrics().fontSize;
    if (!h.term.element) {
      el.replaceChildren(); // evict any terminal a prior agent left in this host
      h.term.open(el);
    } else if (h.term.element.parentElement !== el) {
      el.replaceChildren(h.term.element); // move our instance in, evicting any prior one
    }
    // xterm gains focus → tell the backend mux so this agent's output rides
    // the per-frame fast path. The textarea only exists once opened, so the
    // listener is hooked here (once per terminal); the focus() call below
    // fires it for the initial attach too.
    if (!h.focusHooked && h.term.textarea) {
      h.focusHooked = true;
      h.term.textarea.addEventListener("focus", () => this.setFocused(id));
    }
    flushTerminalQueue(id); // catch up on output that queued while hidden
    this.loadWebgl(h); // needs a canvas — only safe once the terminal is in the DOM
    this.refit(h);
    const ro = new ResizeObserver(() => this.refit(h));
    ro.observe(el);
    h.ro = ro;
    // first layout pass may not have sized the host yet — fit + focus next tick
    queueMicrotask(() => {
      this.refit(h);
      h.term.focus();
    });
    return () => {
      ro.disconnect();
      if (h.ro === ro) h.ro = undefined;
    };
  }

  /** Drop an agent's terminal entirely (on agent/worktree removal). */
  dispose(id: string) {
    const h = this.handles.get(id);
    if (!h) return;
    if (this.sentFocusId === id) this.setFocused(null); // gone → clear the typing-focus sentinel
    h.ro?.disconnect();
    discardTerminalQueue(id);
    this.revs.delete(id);
    this.minSeq.delete(id);
    this.stale.delete(id);
    this.recovering.delete(id);
    h.term.dispose();
    this.handles.delete(id);
  }

  private refit(h: TermHandle) {
    try {
      h.fit.fit();
    } catch {
      // host not laid out yet — a later ResizeObserver tick will retry
    }
  }

  /**
   * Load the WebGL renderer once, after the terminal has a canvas in the DOM.
   * On context loss we dispose the addon (xterm falls back to the DOM renderer),
   * and any construction failure is swallowed so we degrade gracefully.
   */
  private loadWebgl(h: TermHandle) {
    if (h.webglLoaded || !h.term.element) return;
    h.webglLoaded = true;
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      h.term.loadAddon(webgl);
    } catch {
      // no WebGL available — keep the default renderer
    }
  }

  /* xterm parses colors itself (hex/rgb only — no var()/color-mix), so the
   * ANSI ramps are concrete values tuned to the design tokens per theme:
   * dark mirrors the graphite semantic hues; light is the One Light-flavored
   * palette re-tuned to clear AA on the paper ramp — xterm's stock VGA
   * palette is dark-background-only and unreadable on #dedede. */
  private static readonly ANSI_DARK = {
    black: "#17181a",
    red: "#e06c7a", //   --sem-del
    green: "#57c98a", // --sem-add
    yellow: "#d9a441", // --sem-attn
    blue: "#5e9cf5", //  --sem-change
    magenta: "#b082f7", // --accent-fill
    cyan: "#3fb6c4", //  --sem-live
    white: "#b0b1b3", // --ink-2
    brightBlack: "#68696b", // --ink-4
    brightRed: "#ef8a95",
    brightGreen: "#7fd7a5",
    brightYellow: "#e6bc6b",
    brightBlue: "#85b4f8",
    brightMagenta: "#c9a8f5",
    brightCyan: "#6cc9d4",
    brightWhite: "#dee0e2", // --ink
  } as const;

  private static readonly ANSI_LIGHT = {
    black: "#3b3b3b", // --ink
    red: "#bf4240", //   --sem-del (light)
    green: "#1e7d3e", // --sem-add (light)
    yellow: "#996100", // --sem-attn (light)
    blue: "#1c6dc8", //  --sem-change (light)
    magenta: "#752cb3", // --accent-fill (light)
    cyan: "#00788b", //  --sem-live (light)
    white: "#6e6e6e", // --ink-3 — "white" text must stay visible on paper
    brightBlack: "#515151", // --ink-2
    brightRed: "#a83234",
    brightGreen: "#146b33",
    brightYellow: "#7a4d00",
    brightBlue: "#1a5da8",
    brightMagenta: "#8d4dcc",
    brightCyan: "#006277",
    brightWhite: "#3b3b3b", // bright-white output darkens, never vanishes
  } as const;

  private theme(): ITheme {
    const root = document.documentElement;
    const cs = getComputedStyle(root);
    const light = root.getAttribute("data-theme") === "light";
    const v = (name: string, fallback: string) =>
      cs.getPropertyValue(name).trim() || fallback;
    const accentRgb = v("--accent-rgb", light ? "141, 77, 204" : "168, 85, 247");
    return {
      background: v("--bg", light ? "#dedede" : "#121315"),
      foreground: v("--ink-2", light ? "#515151" : "#b0b1b3"),
      cursor: v("--accent", light ? "#8d4dcc" : "#a855f7"),
      cursorAccent: v("--bg", light ? "#dedede" : "#121315"),
      selectionBackground: `rgba(${accentRgb}, ${light ? 0.22 : 0.28})`,
      ...(light ? TerminalService.ANSI_LIGHT : TerminalService.ANSI_DARK),
    };
  }
}
