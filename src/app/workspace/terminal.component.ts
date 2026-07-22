import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  input,
  signal,
  viewChild,
} from "@angular/core";
import { Agent } from "../models";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";

/**
 * Hosts the agent's persistent xterm terminal. The Terminal instance lives in
 * TerminalService (survives tab switches); this component just attaches it to
 * the visible host and re-attaches when the shown agent changes.
 *
 * Ctrl+F (while the terminal area is focused) toggles a small search box
 * overlaid top-right; Enter -> next, Shift+Enter -> previous, Esc -> close.
 */
@Component({
  selector: "app-terminal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--bg)">
      <div style="color:var(--ink-4);padding:var(--sp-4) var(--sp-6) var(--sp-2);font-size:var(--fs-xs)">── session: {{ agent().worktree }} · {{ agent().branch }} ──</div>
      <div style="flex:1;min-height:0;position:relative">
        <div #host [attr.data-agent-id]="agent().id" style="position:absolute;inset:0;padding:var(--sp-1) var(--sp-5) var(--sp-4)"></div>

        @if (searchOpen()) {
          <div
            style="position:absolute;top:8px;right:14px;z-index:5;display:flex;align-items:center;gap:var(--sp-2);
                   padding:var(--sp-2) var(--sp-2) var(--sp-2) var(--sp-4);background:var(--panel);border:1px solid var(--hair);
                   border-radius:6px;box-shadow:0 4px 14px rgba(0,0,0,0.35)"
            (keydown)="onBoxKeydown($event)"
          >
            <input
              #searchInput
              type="text"
              placeholder="Find"
              [value]="query()"
              (input)="onQuery($event)"
              spellcheck="false"
              autocomplete="off"
              style="width:160px;background:transparent;border:0;outline:0;color:var(--ink-2);
                     font-size:var(--fs-ui);font-family:inherit"
            />
            @if (matchInfo()) {
              <span class="tnum" style="font-size:var(--fs-xs);color:var(--ink-4);padding:0 var(--sp-1);white-space:nowrap">{{ matchInfo() }}</span>
            }
            <button type="button" title="Previous (Shift+Enter)" (click)="prev()" [style]="btn">↑</button>
            <button type="button" title="Next (Enter)" (click)="next()" [style]="btn">↓</button>
            <button type="button" title="Close (Esc)" (click)="close()" [style]="btn">✕</button>
          </div>
        }
      </div>
    </div>
  `,
})
export class TerminalComponent {
  private terminals = inject(TerminalService);
  private ui = inject(UiStore);
  private cdr = inject(ChangeDetectorRef);
  readonly agent = input.required<Agent>();

  private host = viewChild.required<ElementRef<HTMLDivElement>>("host");
  private searchInput = viewChild<ElementRef<HTMLInputElement>>("searchInput");
  private detach: (() => void) | null = null;
  private shownId: string | null = null;

  readonly searchOpen = signal(false);
  readonly query = signal("");

  /** "n / total" for the current search, or "" when there's no active query. */
  readonly matchInfo = computed(() => {
    if (!this.query()) return "";
    const r = this.terminals.searchResults()[this.agent().id];
    return r && r.count > 0 ? `${r.index + 1}/${r.count}` : "0/0";
  });

  /** Shared style for the tiny prev/next/close affordances. */
  readonly btn =
    "display:inline-flex;align-items:center;justify-content:center;width:var(--sp-8);height:var(--sp-8);" +
    "background:transparent;border:0;border-radius:4px;color:var(--ink-3);cursor:pointer;font-size:var(--fs-sm);line-height:1";

  constructor() {
    const destroyRef = inject(DestroyRef);
    // Ctrl/Cmd+F opens OUR search box. Capture phase + preventDefault stops the
    // webview's native find-in-page bar (which a bubble-phase / xterm handler is
    // too late to cancel); window-level so it works regardless of focus.
    window.addEventListener("keydown", this.onWindowKeydown, true);
    destroyRef.onDestroy(() => {
      this.detach?.();
      window.removeEventListener("keydown", this.onWindowKeydown, true);
    });
    // re-theme live terminals when the app theme/palette toggles (microtask so
    // UiStore has set [data-theme] / the --accent vars before xterm reads them)
    effect(() => {
      void this.ui.tweaks();
      queueMicrotask(() => this.terminals.retheme());
    });
    // (re)attach the persistent terminal after render, whenever the agent changes
    afterRenderEffect(() => {
      const el = this.host().nativeElement;
      const id = this.agent().id;
      if (id === this.shownId) return;
      if (this.shownId) this.close(); // dropping the old terminal's search state
      this.shownId = id;
      this.detach?.();
      this.detach = this.terminals.attach(id, el);
    });
    // focus the input when the box opens
    afterRenderEffect(() => {
      if (this.searchOpen()) this.searchInput()?.nativeElement.focus();
    });
  }

  /** Ctrl/Cmd+F → open our search box + suppress the webview's native find bar. */
  private onWindowKeydown = (e: KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && (e.key === "f" || e.key === "F")) {
      e.preventDefault();
      e.stopPropagation();
      this.open();
      queueMicrotask(() => this.searchInput()?.nativeElement.focus());
    }
  };

  /** Keys handled inside the search box itself. */
  onBoxKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      this.close();
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.shiftKey ? this.prev() : this.next();
    }
  }

  onQuery(e: Event) {
    this.query.set((e.target as HTMLInputElement).value);
    // re-run incrementally so highlights track typing
    if (this.query()) this.next();
    else this.terminals.clearSearch(this.shownId ?? this.agent().id);
  }

  open() {
    this.searchOpen.set(true);
    this.cdr.markForCheck();
  }

  close() {
    this.searchOpen.set(false);
    this.query.set("");
    this.terminals.clearSearch(this.shownId ?? this.agent().id);
    this.cdr.markForCheck();
  }

  next() {
    const q = this.query();
    if (q) this.terminals.findNext(this.shownId ?? this.agent().id, q, { incremental: true });
  }

  prev() {
    const q = this.query();
    if (q) this.terminals.findPrevious(this.shownId ?? this.agent().id, q);
  }
}
