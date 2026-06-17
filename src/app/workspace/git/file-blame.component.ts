import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { ScrollingModule } from "@angular/cdk/scrolling";
import { BlameLine } from "../../models";
import { GitInspectStore } from "../../agents/git-inspect.store";
import { AuthorAvatarComponent } from "../../shared/git/author-avatar.component";
import { ShaChipComponent } from "../../shared/git/sha-chip.component";
import { IconComponent } from "../../shared/icon.component";

// ---- constants ----
/** Fixed pixel height of each virtual row (matches design: 12px font × 1.7 lh ≈ 20.4 → 21). */
const LINE_H = 21;
/** Fixed gutter width from the design. */
const GUTTER_W = 196;
/** Fixed line-number column width from the design. */
const LINENO_W = 44;

// ---- age background (mirrors ageBgA from design) ----
// age 0 = newest (almost fully transparent accent), age 1 = oldest (more accent).
function ageBg(age: number): string {
  const pct = 86 + Math.round(age * 11); // 86..97 %
  return `color-mix(in oklch, var(--accent), transparent ${pct}%)`;
}

// ---- relative time ----
// Compact relative-time formatter: "<N>s", "<N>m", "<N>h", "<N>d", "<N>mo", "<N>y".
// `now` is unix seconds. Returns empty string if `when` is falsy.
function relTime(when: number, now: number): string {
  if (!when) return "";
  const diff = Math.max(0, now - when);
  if (diff < 60) return `${diff}s`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)}d`;
  if (diff < 86400 * 365) return `${Math.floor(diff / (86400 * 30))}mo`;
  return `${Math.floor(diff / (86400 * 365))}y`;
}

/** A processed blame row: the raw BlameLine plus computed display fields. */
interface BlameRow extends BlameLine {
  /** True when this line is the first in a run with the same sha (show header). */
  first: boolean;
  /** 0=newest..1=oldest, for the age shading. */
  age: number;
  /** Compact relative-time string (e.g. "3d", "2h"). */
  rel: string;
}

/** Data carried by the hover tooltip signal. */
interface HoverState {
  author: string;
  sha: string;
  summary: string;
  /** Viewport-relative X of the gutter right edge. */
  x: number;
  /** Viewport-relative Y of the gutter row top. */
  y: number;
}

@Component({
  selector: "app-file-blame",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ScrollingModule, AuthorAvatarComponent, ShaChipComponent, IconComponent],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;position:relative;background:var(--bg)">

      <!-- loading / idle overlay -->
      @if (loadable().status === 'loading') {
        <div style="position:absolute;inset:0;display:grid;place-items:center;z-index:10;background:color-mix(in oklch,var(--bg),transparent 30%)">
          <span style="font-size:var(--fs-sm);color:var(--ink-4)">loading blame…</span>
        </div>
      } @else if (loadable().status === 'error') {
        <div style="position:absolute;inset:0;display:grid;place-items:center;z-index:10">
          <span style="font-size:var(--fs-sm);color:var(--st-blocked)">blame unavailable</span>
        </div>
      } @else if (loadable().status === 'idle') {
        <div style="position:absolute;inset:0;display:grid;place-items:center;z-index:10">
          <span style="font-size:var(--fs-sm);color:var(--ink-4)">—</span>
        </div>
      }

      <!-- virtualized blame body -->
      <cdk-virtual-scroll-viewport
        [itemSize]="lineH"
        style="flex:1;min-height:0;font-family:var(--font-mono);font-size:12px;line-height:1.7"
      >
        <div
          *cdkVirtualFor="let ln of rows(); track ln.n"
          style="display:flex;height:21px;overflow:hidden"
        >
          <!-- gutter cell: age-shaded, author bar + (on first line of run) avatar/name/rel/sha -->
          <div
            class="gutter-cell"
            [style.background]="ageBg(ln.age)"
            (mouseenter)="onEnter($event, ln)"
            (mouseleave)="onLeave()"
            (click)="openCommit.emit(ln.sha)"
            style="
              flex: none;
              width: 196px;
              display: flex;
              align-items: center;
              gap: 7px;
              padding: 0 9px 0 0;
              border-right: 1px solid var(--hair);
              cursor: pointer;
              user-select: none;
              overflow: hidden;
            "
          >
            <!-- colored author bar -->
            <span
              [style.background]="authorColor(ln.author)"
              [style.opacity]="0.25 + (1 - ln.age) * 0.7"
              style="flex:none;width:3px;align-self:stretch"
            ></span>

            @if (ln.first) {
              <app-author-avatar [author]="ln.author" [size]="14" />
              <span style="font-size:10px;color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:1;min-width:0">
                {{ ln.author }}
              </span>
              <span class="tnum" style="font-size:9px;color:var(--ink-4);flex:none">{{ ln.rel }}</span>
              <app-sha-chip [sha]="ln.sha" />
            } @else {
              <span style="flex:1"></span>
            }
          </div>

          <!-- line number -->
          <span
            class="tnum"
            style="
              width: 44px;
              flex: none;
              text-align: right;
              padding: 0 11px 0 0;
              color: var(--ink-4);
              user-select: none;
            "
          >{{ ln.n }}</span>

          <!-- code line (plain text, no syntax highlight) -->
          <code
            style="
              flex: 1;
              white-space: pre-wrap;
              word-break: break-word;
              padding-right: 14px;
              color: var(--ink-2);
            "
          >{{ ln.line }}</code>
        </div>
      </cdk-virtual-scroll-viewport>

      <!-- single shared tooltip — rendered only while hovering, no per-row DOM -->
      @if (hover(); as h) {
        <div
          style="
            position: fixed;
            z-index: 80;
            width: 280px;
            background: var(--elev);
            border: 1px solid var(--hair-2);
            border-radius: var(--r-md);
            box-shadow: var(--shadow);
            padding: 11px;
            pointer-events: none;
          "
          [style.left.px]="tooltipLeft(h)"
          [style.top.px]="h.y"
        >
          <div style="display:flex;align-items:center;gap:var(--sp-3);margin-bottom:7px">
            <app-author-avatar [author]="h.author" [size]="20" />
            <span style="font-size:11px;font-weight:600;color:var(--ink);flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
              {{ h.author }}
            </span>
            <app-sha-chip [sha]="h.sha" />
          </div>
          <div style="font-size:10.5px;color:var(--ink-2);line-height:1.45;text-wrap:pretty">
            {{ h.summary }}
          </div>
          <div style="font-size:9px;color:var(--accent-2);margin-top:7px;display:flex;align-items:center;gap:5px">
            <app-icon name="enter" size="sm" [px]="11" />
            click → open commit diff
          </div>
        </div>
      }
    </div>
  `,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
        min-width: 0;
      }
      .gutter-cell:hover {
        filter: brightness(1.08);
      }
    `,
  ],
})
export class FileBlameComponent {
  /** Agent id — used as the first key for GitInspectStore. */
  readonly agent = input.required<string>();
  /** Repo-relative file path to blame. */
  readonly path = input.required<string>();
  /** Emitted when the user clicks a gutter cell (sha of the commit). */
  readonly openCommit = output<string>();

  private readonly store = inject(GitInspectStore);

  /** Exposed so the template can read lineH without a ts class member dance. */
  readonly lineH = LINE_H;

  /** Hover signal — ONE object drives the shared tooltip; null = no tooltip. */
  readonly hover = signal<HoverState | null>(null);

  /** Raw Loadable from the store. */
  readonly loadable = computed(() => this.store.blameFor(this.agent(), this.path()));

  /** Processed rows with `first`, `age`, `rel`. */
  readonly rows = computed<BlameRow[]>(() => {
    const lines = this.loadable().data;
    if (!lines.length) return [];

    const now = Math.floor(Date.now() / 1000);

    // Compute age 0..1 (newest=0, oldest=1) across the set.
    let minWhen = lines[0].when;
    let maxWhen = lines[0].when;
    for (const ln of lines) {
      if (ln.when < minWhen) minWhen = ln.when;
      if (ln.when > maxWhen) maxWhen = ln.when;
    }
    const span = maxWhen - minWhen || 1;

    return lines.map((ln, i) => ({
      ...ln,
      first: i === 0 || lines[i - 1].sha !== ln.sha,
      age: (maxWhen - ln.when) / span,
      rel: relTime(ln.when, now),
    }));
  });

  constructor() {
    // Re-fetch blame whenever agent or path changes.
    effect(() => {
      const agent = this.agent();
      const path = this.path();
      if (agent && path) {
        this.store.loadBlame(agent, path);
      }
    });
  }

  // ---- template helpers ----

  ageBg(age: number): string {
    return ageBg(age);
  }

  /** Derive a stable per-author hue color (same algorithm as AuthorAvatarComponent). */
  authorColor(author: string): string {
    let hash = 0;
    for (let i = 0; i < author.length; i++) {
      hash = ((hash * 31) + author.charCodeAt(i)) >>> 0;
    }
    const hue = (hash % 300 + 30) % 360;
    return `hsl(${hue}, 65%, 62%)`;
  }

  onEnter(event: MouseEvent, ln: BlameRow): void {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.hover.set({
      author: ln.author,
      sha: ln.sha,
      summary: ln.summary,
      x: rect.right,
      y: rect.top,
    });
  }

  onLeave(): void {
    this.hover.set(null);
  }

  /** Clamp tooltip so it doesn't overflow the right edge of the viewport. */
  tooltipLeft(h: HoverState): number {
    return Math.min(h.x + 8, window.innerWidth - 300);
  }
}
