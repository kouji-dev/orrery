import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  EventEmitter,
  HostListener,
  Input,
  Output,
  computed,
  effect,
  inject,
  signal,
} from "@angular/core";
import { BlameLine } from "../../models";
import { IconComponent } from "../../shared/icon.component";

// ---------------------------------------------------------------------------
// Pure data helpers
// ---------------------------------------------------------------------------

export interface BlameRow {
  n: number;
  sha: string;
  author: string;
  s: string;
  when: number;
  age: number;
  rel: string;
  first: boolean;
  summary: string;
}

/** Stable per-author hue — mirrors authorColor in code-diff.component.ts. */
function authorColor(author: string): string {
  let hash = 0;
  for (let i = 0; i < author.length; i++) hash = ((hash * 31) + author.charCodeAt(i)) >>> 0;
  const hue = ((hash % 300) + 30) % 360;
  return `hsl(${hue}, 60%, 66%)`;
}

/** 1–2 uppercase initials from a display name. */
function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((w) => w[0].toUpperCase())
    .join("");
}

/** Unix seconds → short relative string, e.g. "2h", "3d", "5m". Empty for 0. */
function relTime(when: number, now = Date.now()): string {
  if (!when) return "";
  const secs = Math.floor(now / 1000) - when;
  if (secs < 0) return "now";
  if (secs < 60) return `${secs}s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h`;
  const days = Math.floor(hrs / 24);
  if (days < 30) return `${days}d`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo`;
  return `${Math.floor(months / 12)}y`;
}

/**
 * Age background using the same color-mix formula from the design reference.
 * age 0 = newest → most opaque accent; age 1 = oldest → most transparent.
 */
function ageBgA(age: number): string {
  return `color-mix(in oklch, var(--accent), transparent ${86 + Math.round(age * 11)}%)`;
}

export function blameToRows(lines: BlameLine[], now = Date.now()): BlameRow[] {
  if (!lines.length) return [];

  // Determine max and span for age normalization (skip uncommitted when===0)
  let maxWhen = 0;
  let minWhen = Infinity;
  for (const l of lines) {
    if (l.when > 0) {
      if (l.when > maxWhen) maxWhen = l.when;
      if (l.when < minWhen) minWhen = l.when;
    }
  }
  if (minWhen === Infinity) minWhen = 0;
  const span = maxWhen - minWhen;

  return lines.map((ln, i) => {
    const age = ln.when === 0
      ? 0
      : Math.max(0, Math.min(1, (maxWhen - ln.when) / (span || 1)));
    return {
      n: ln.n,
      sha: ln.sha,
      author: ln.author,
      s: ln.line,
      when: ln.when,
      age,
      rel: relTime(ln.when, now),
      first: i === 0 || lines[i - 1].sha !== ln.sha,
      summary: ln.summary,
    };
  });
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

interface Popup {
  row: BlameRow;
  x: number;
  y: number;
}

/**
 * Annotate (blame) view — ports the FileBlameGutter design reference.
 *
 * NOTE: Input uses decorator @Input backed by a signal (same pattern as
 * ReviewCodeComponent) to avoid NG0950 under vitest JIT.
 */
@Component({
  selector: "app-annotate-blame",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div class="scroll-y" style="flex:1;position:relative;background:var(--bg)">
      <!-- scoped find (B3.3): Ctrl+F while the pointer is over the blame view -->
      @if (findOpen()) {
        <div class="bf-bar" (mousedown)="$event.stopPropagation()">
          <app-icon name="search" size="sm" [px]="11" color="var(--ink-4)" />
          <input
            class="bf-input"
            [value]="query()"
            (input)="query.set($any($event.target).value); activeIdx.set(0)"
            (keydown.enter)="next()"
            (keydown.shift.enter)="prev()"
            (keydown.escape)="closeFind()"
            placeholder="find in blame…"
            spellcheck="false"
          />
          <span class="tnum bf-count">{{ matches().length ? activeIdx() + 1 : 0 }}/{{ matches().length }}</span>
          <button class="btn" (click)="prev()" title="Previous match (Shift+Enter)"><app-icon name="chevronD" size="sm" [px]="10" style="display:inline-flex;transform:rotate(180deg)" /></button>
          <button class="btn" (click)="next()" title="Next match (Enter)"><app-icon name="chevronD" size="sm" [px]="10" /></button>
          <button class="btn" (click)="closeFind()" title="Close (Esc)"><app-icon name="x" size="sm" [px]="10" /></button>
        </div>
      }
      <pre style="margin:0;font-family:var(--font-mono);font-size:12px;line-height:1.7">
        @for (row of rows(); track row.n; let i = $index) {
          <div style="display:flex" [class.bf-hit]="isHit(i)" [class.bf-on]="isActiveHit(i)" [attr.data-bn]="row.n">
            <!-- author column -->
            <div
              (mouseenter)="onEnter($event, row)"
              (mouseleave)="popup.set(null)"
              (click)="onClickRow(row)"
              [style.background]="ageBg(row.age)"
              style="flex:none;width:196px;display:flex;align-items:center;gap:7px;padding:0 9px 0 0;border-right:1px solid var(--hair);cursor:pointer;user-select:none"
            >
              <!-- age stripe -->
              <span [style.background]="colorOf(row.author)"
                    [style.opacity]="0.25 + (1 - row.age) * 0.7"
                    style="flex:none;width:3px;align-self:stretch"></span>
              @if (row.first) {
                <!-- initials chip -->
                <span class="chip" [style.background]="colorOf(row.author)"
                      style="flex:none;font-size:9px;padding:0 4px;color:#000;font-weight:600;border-radius:3px">
                  {{ initialsOf(row.author) }}
                </span>
                <span style="font-size:10px;color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;min-width:0">{{ row.author }}</span>
                <span class="tnum" style="font-size:9px;color:var(--ink-4)">{{ row.rel }}</span>
                <span class="tnum chip" style="font-size:8px;padding:0 4px">{{ row.sha.slice(0,7) }}</span>
              } @else {
                <span style="flex:1"></span>
              }
            </div>
            <!-- line number -->
            <span class="tnum" style="width:44px;flex:none;text-align:right;padding:0 11px 0 0;color:var(--ink-4);user-select:none">{{ row.n }}</span>
            <!-- code -->
            <code style="flex:1;white-space:pre-wrap;word-break:break-word;padding-right:14px;color:var(--ink-2)">{{ row.s }}</code>
          </div>
        }
      </pre>

      <!-- hover popup -->
      @if (popup(); as p) {
        <div [style.left.px]="popupLeft()"
             [style.top.px]="p.y"
             style="position:fixed;z-index:80;width:280px;background:var(--elev);border:1px solid var(--hair-2);border-radius:var(--r-md);box-shadow:var(--shadow);padding:11px;pointer-events:none">
          <div style="display:flex;align-items:center;gap:8px;margin-bottom:7px">
            <!-- initials chip in popup -->
            <span class="chip" [style.background]="colorOf(p.row.author)"
                  style="flex:none;font-size:10px;padding:0 5px;color:#000;font-weight:600;border-radius:3px">
              {{ initialsOf(p.row.author) }}
            </span>
            <span style="font-size:11px;font-weight:600;color:var(--ink);flex:1">{{ p.row.author }}</span>
            <span class="chip tnum" style="font-size:9.5px;padding:0 6px">{{ p.row.sha.slice(0,7) }}</span>
          </div>
          <div style="font-size:10.5px;color:var(--ink-2);line-height:1.45">{{ p.row.summary }}</div>
          <div style="font-size:9px;color:var(--accent-2);margin-top:7px;display:flex;align-items:center;gap:5px">
            <app-icon name="enter" [px]="11"></app-icon>
            click → open commit diff
          </div>
        </div>
      }
    </div>
  `,
  styles: [
    `
      /* scoped-find bar: sticky over the scroller, code-surface metrics */
      .bf-bar {
        position: sticky;
        top: 0;
        z-index: 10;
        display: flex;
        align-items: center;
        gap: var(--sp-2);
        padding: var(--sp-1) var(--sp-4);
        background: var(--elev);
        border-bottom: 1px solid var(--hair-2);
      }
      .bf-input {
        flex: 1;
        min-width: 0;
        max-width: 240px;
        background: var(--panel-2);
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
        padding: var(--sp-1) var(--sp-3);
        color: var(--ink);
        font-family: var(--font-mono);
        font-size: var(--fs-xs);
        outline: none;
      }
      .bf-count {
        font-size: var(--fs-2xs);
        color: var(--ink-4);
      }
      .bf-bar .btn {
        display: flex;
        padding: var(--sp-1);
        border-radius: 4px;
        color: var(--ink-3);
      }
      .bf-hit {
        background: color-mix(in oklch, var(--accent), transparent 92%);
      }
      .bf-on {
        background: color-mix(in oklch, var(--accent), transparent 80%);
        box-shadow: inset 2px 0 0 var(--accent);
      }
    `,
  ],
})
export class AnnotateBlameComponent {
  @Input() set lines(v: BlameLine[]) { this._lines.set(v ?? []); }
  private readonly _lines = signal<BlameLine[]>([]);

  @Output() readonly openCommit = new EventEmitter<string>();

  readonly popup = signal<Popup | null>(null);

  readonly rows = computed(() => blameToRows(this._lines()));

  // ----- scoped find (B3.3) -----
  private readonly host = inject(ElementRef<HTMLElement>);
  readonly findOpen = signal(false);
  readonly query = signal("");
  readonly activeIdx = signal(0);
  private hovered = false;

  /** Row indices whose code line contains the query (case-insensitive). */
  readonly matches = computed<number[]>(() => {
    const q = this.query().trim().toLowerCase();
    if (!q) return [];
    const out: number[] = [];
    this.rows().forEach((r, i) => {
      if (r.s.toLowerCase().includes(q)) out.push(i);
    });
    return out;
  });

  constructor() {
    // keep the active hit visible as the query / index changes
    effect(() => {
      const m = this.matches();
      const idx = this.activeIdx();
      const row = m.length ? this.rows()[m[Math.min(idx, m.length - 1)]] : null;
      if (!row) return;
      queueMicrotask(() => {
        this.host.nativeElement
          .querySelector(`[data-bn="${row.n}"]`)
          ?.scrollIntoView({ block: "nearest" });
      });
    });
  }

  isHit(i: number): boolean {
    return this.matches().includes(i);
  }
  isActiveHit(i: number): boolean {
    const m = this.matches();
    return m.length > 0 && m[Math.min(this.activeIdx(), m.length - 1)] === i;
  }
  next(): void {
    const n = this.matches().length;
    if (n) this.activeIdx.update((i) => (i + 1) % n);
  }
  prev(): void {
    const n = this.matches().length;
    if (n) this.activeIdx.update((i) => (i - 1 + n) % n);
  }
  closeFind(): void {
    this.findOpen.set(false);
    this.query.set("");
    this.activeIdx.set(0);
  }

  @HostListener("mouseenter") onHostEnter(): void { this.hovered = true; }
  @HostListener("mouseleave") onHostLeave(): void { this.hovered = false; }

  /** Ctrl+F opens the scoped find while the pointer is over this surface. */
  @HostListener("document:keydown", ["$event"])
  onDocKey(e: KeyboardEvent): void {
    if (!this.hovered) return;
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      e.stopPropagation();
      this.findOpen.set(true);
      queueMicrotask(() => {
        (this.host.nativeElement.querySelector(".bf-input") as HTMLInputElement | null)?.focus();
      });
    }
  }

  readonly popupLeft = computed(() => {
    const p = this.popup();
    return p ? Math.min(p.x + 8, window.innerWidth - 300) : 0;
  });

  // ---------------------------------------------------------------------------
  // Expose helpers to template
  // ---------------------------------------------------------------------------

  protected ageBg(age: number): string { return ageBgA(age); }
  protected colorOf(author: string): string { return authorColor(author); }
  protected initialsOf(name: string): string { return initials(name); }

  protected onEnter(e: MouseEvent, row: BlameRow): void {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    this.popup.set({ row, x: r.right, y: r.top });
  }

  protected onClickRow(row: BlameRow): void {
    if (row.sha) this.openCommit.emit(row.sha);
  }
}
