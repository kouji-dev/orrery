import { inject, Injectable, OnDestroy } from "@angular/core";
import { FitAddon } from "@xterm/addon-fit";
import { ITheme, Terminal } from "@xterm/xterm";
import { AgentsStore } from "./stores/agents.store";

interface TermHandle {
  term: Terminal;
  fit: FitAddon;
  ro?: ResizeObserver;
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

  /** Subscribe to OSC window-title changes from any agent's terminal. */
  onTitle(cb: (id: string, title: string) => void) {
    this.titleCb = cb;
  }

  /** Get — lazily creating — the persistent terminal for an agent. */
  private handle(id: string): TermHandle {
    let h = this.handles.get(id);
    if (!h) {
      const term = new Terminal({
        cursorBlink: true,
        fontFamily: MONO,
        fontSize: 12,
        lineHeight: 1.25,
        scrollback: 5000,
        theme: this.theme(),
      });
      const fit = new FitAddon();
      term.loadAddon(fit);
      term.onData((data) => void this.agents.input(id, data).catch(() => {}));
      term.onResize(({ cols, rows }) => void this.agents.resize(id, rows, cols).catch(() => {}));
      term.onTitleChange((title) => this.titleCb?.(id, title)); // live agent state
      h = { term, fit };
      this.handles.set(id, h);
    }
    return h;
  }

  /** Write a raw PTY chunk to the agent's terminal (buffers even if unattached). */
  write(id: string, chunk: string) {
    this.handle(id).term.write(chunk);
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
    this.handles.get(id)?.term.write("\r\n\x1b[2m▪ process exited\x1b[0m\r\n");
  }

  /** A dim, non-output hint (e.g. idle state) without faking program output. */
  hint(id: string, text: string) {
    this.handle(id).term.write(`\x1b[2m${text}\x1b[0m\r\n`);
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
    h.ro?.disconnect();
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

  private theme(): ITheme {
    const cs = getComputedStyle(document.documentElement);
    const v = (name: string, fallback: string) => cs.getPropertyValue(name).trim() || fallback;
    return {
      background: v("--bg", "#0b0d10"),
      foreground: v("--ink-2", "#c8ccd4"),
      cursor: v("--accent", "#a855f7"),
      cursorAccent: v("--bg", "#0b0d10"),
      selectionBackground: v("--sel", "rgba(168,85,247,0.25)"),
    };
  }
}
