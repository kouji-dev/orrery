import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  ChangeDetectorRef,
  Component,
  DestroyRef,
  ElementRef,
  inject,
  input,
  signal,
  viewChild,
} from "@angular/core";
import { Agent } from "../models";
import { TerminalService } from "../terminal.service";

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
    <div
      style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--bg)"
      (keydown)="onAreaKeydown($event)"
    >
      <div style="color:var(--ink-4);padding:8px 14px 4px;font-size:10.5px">── session: {{ agent().worktree }} · {{ agent().branch }} ──</div>
      <div style="flex:1;min-height:0;position:relative">
        <div #host style="position:absolute;inset:0;padding:2px 10px 8px"></div>

        @if (searchOpen()) {
          <div
            style="position:absolute;top:8px;right:14px;z-index:5;display:flex;align-items:center;gap:4px;
                   padding:4px 4px 4px 8px;background:var(--panel);border:1px solid var(--hair);
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
                     font-size:12px;font-family:inherit"
            />
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
  private cdr = inject(ChangeDetectorRef);
  readonly agent = input.required<Agent>();

  private host = viewChild.required<ElementRef<HTMLDivElement>>("host");
  private searchInput = viewChild<ElementRef<HTMLInputElement>>("searchInput");
  private detach: (() => void) | null = null;
  private shownId: string | null = null;

  readonly searchOpen = signal(false);
  readonly query = signal("");

  /** Shared style for the tiny prev/next/close affordances. */
  readonly btn =
    "display:inline-flex;align-items:center;justify-content:center;width:20px;height:20px;" +
    "background:transparent;border:0;border-radius:4px;color:var(--ink-3);cursor:pointer;font-size:11px;line-height:1";

  constructor() {
    inject(DestroyRef).onDestroy(() => this.detach?.());
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

  /** Ctrl+F anywhere in the terminal area toggles the search box. */
  onAreaKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      e.stopPropagation();
      this.searchOpen() ? this.close() : this.open();
    }
  }

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
