import { inject, Injectable, OnDestroy, signal, WritableSignal } from "@angular/core";
import { ClipboardAddon } from "@xterm/addon-clipboard";
import { FitAddon } from "@xterm/addon-fit";
import { ISearchOptions, SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { ITheme, Terminal } from "@xterm/xterm";
import { AgentsStore } from "./stores/agents.store";
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

/**
 * Owns one persistent xterm `Terminal` per agent. The instance outlives any
 * single view: output is written to it as it streams (even while its pane is
 * hidden), and the DOM is re-parented into whichever host attaches. Keystrokes
 * and resizes are forwarded to the agent's PTY through the backend.
 */
@Injectable({ providedIn: "root" })
export class TerminalService implements OnDestroy {
  private agents = inject(AgentsStore);
  private handles = new Map<string, TermHandle>();
  private titleCb?: (id: string, title: string) => void;

  /** Re-apply the current app theme to every live terminal. Called by the terminal
   *  component when the theme toggles — xterm only reads its theme at attach time,
   *  so without this an already-open terminal keeps the old theme until re-mounted. */
  retheme() {
    const theme = this.theme();
    for (const h of this.handles.values()) h.term.options.theme = theme;
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
  /** Current revision for one agent (reactive — changes on each parsed write). */
  revOf(id: string): number {
    return this.revSignal(id)();
  }
  private bumpRevision(id: string) {
    this.revSignal(id).update((n) => n + 1);
  }

  // The agent whose xterm last GAINED focus — i.e. the terminal the user types
  // in. The backend output mux drains that agent every ~16ms frame (keystroke
  // echo needs the fast path) and holds everyone else to a ~150ms cadence, so
  // we tell it on every change. Tracks the last value actually sent: gains are
  // invoked once per change (no flapping on repeated focus of the same
  // terminal), and it is cleared only when the focused agent's process exits
  // or its terminal is disposed — NOT on blur, so the visible terminal keeps
  // the fast path while the user works elsewhere in the app.
  private sentFocusId: string | null = null;
  private setFocused(id: string | null) {
    if (id === this.sentFocusId) return;
    const prev = this.sentFocusId;
    this.sentFocusId = id;
    void this.agents.focus(id).catch(() => {
      // Invoke failed — roll back the sentinel so the next call with the same
      // id is not suppressed by the dedup guard and can retry the IPC.
      this.sentFocusId = prev;
    });
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
        fontSize: 12,
        lineHeight: 1.25,
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
  // Subtle accent-tinted decorations so matches read against the dark theme.
  private searchDecorations(): ISearchOptions["decorations"] {
    const cs = getComputedStyle(document.documentElement);
    const accent = cs.getPropertyValue("--accent").trim() || "#a855f7";
    return {
      matchBackground: accent,
      matchBorder: accent,
      matchOverviewRuler: accent,
      activeMatchBackground: accent,
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
    const h = this.handle(id);
    h.term.options.theme = this.theme(); // pick up the current light/dark theme
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
    if (this.sentFocusId === id) this.setFocused(null); // gone → release the fast path
    h.ro?.disconnect();
    discardTerminalQueue(id);
    this.revs.delete(id);
    h.term.dispose();
    this.handles.delete(id);
  }

  ngOnDestroy() {
    for (const id of [...this.handles.keys()]) this.dispose(id);
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

  private theme(): ITheme {
    const cs = getComputedStyle(document.documentElement);
    const v = (name: string, fallback: string) =>
      cs.getPropertyValue(name).trim() || fallback;
    return {
      background: v("--bg", "#0b0d10"),
      foreground: v("--ink-2", "#c8ccd4"),
      cursor: v("--accent", "#a855f7"),
      cursorAccent: v("--bg", "#0b0d10"),
      selectionBackground: v("--sel", "rgba(168,85,247,0.25)"),
    };
  }
}
