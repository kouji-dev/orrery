import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  input,
  output,
  signal,
  untracked,
  viewChild,
} from "@angular/core";
import { KjButtonComponent, KjKbdComponent } from "@kouji-ui/components";
import { ReviewStore } from "../../agents/review.store";
import { Agent } from "../../models";
import { IconComponent } from "../../shared/icon.component";
import { UiStore } from "../../ui/ui.store";
import { fileName } from "../../utils";
import { unwrapSelection } from "./md-highlight";
import { rangeToLines } from "./md-source-map";

/** A text selection turned into a comment anchor (design `pending`). */
interface Pending {
  snippet: string;
  fromLine: number;
  toLine: number;
  /** Scroller-content coordinates of the selection's last client rect. */
  x: number;
  y: number;
  h: number;
  range: Range;
}

interface Hover {
  id: string;
  x: number;
  y: number;
}

/** Leave grace before a hover card closes — enough to cross the gap. */
const HOVER_GRACE_MS = 260;

/**
 * Selection → note layer for the markdown preview (design `MarkdownPreview`
 * onMouseUp / startCompose / save / onOver / onOut). Rendered INSIDE the
 * scroller as `display: contents`, so its absolute children (the floating
 * note button, the composer, the hover card) position against the scroller's
 * content box and scroll with the document.
 *
 * The scroller and the rendered body are plain element inputs: the events
 * live on the scroller (a selection ends anywhere in it) while the guards
 * test containment against the `.rte-view` body.
 */
@Component({
  selector: "app-md-review-overlay",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjButtonComponent, KjKbdComponent],
  host: {
    style: "display:contents",
    "(document:keydown.escape)": "onEscape()",
  },
  template: `
    @if (pending(); as p) {
      @if (!compose()) {
        <button class="md-note-btn floating" [style.left.px]="p.x + 4" [style.top.px]="p.y + p.h / 2 - 10"
                (mousedown)="startCompose($event)" title="Comment on selection" aria-label="Comment on selection">
          <app-icon name="chat" size="sm" />
        </button>
      } @else {
        <div class="rc-composer" [style.left.px]="composerLeft()" [style.top.px]="p.y + p.h + 10"
             (mouseup)="$event.stopPropagation()" (click)="$event.stopPropagation()">
          <div class="hd">
            <app-icon name="chat" size="sm" color="var(--ui-ink)" />
            <span>Comment on <b>selection · {{ lineLabel(p.fromLine, p.toLine) }}</b></span>
            <span class="q">review · queued for agent</span>
          </div>
          <textarea #ta rows="3" [value]="draft()" (input)="draft.set(ta.value)" (keydown)="onKey($event)"
                    placeholder="Leave a note for the agent — what to change and why…"></textarea>
          <div class="ft">
            <span class="hint"><kj-kbd kjSize="xs">⌘⏎</kj-kbd> save <span style="margin:0 var(--sp-2)">·</span> <kj-kbd kjSize="xs">esc</kj-kbd> cancel</span>
            <div style="margin-left:auto;display:flex;gap:var(--sp-3)">
              <kj-button kjVariant="outline" kjSize="xs" class="btn" (click)="cancel()">Cancel</kj-button>
              <kj-button kjVariant="default" kjSize="xs" class="btn" [kjDisabled]="!draft().trim()" (click)="save()">Save</kj-button>
            </div>
          </div>
        </div>
      }
    }
    @if (hoverComment(); as hc) {
      <div class="md-card" [style.left.px]="hoverLeft()" [style.top.px]="hover()!.y"
           (mouseenter)="clearHoverTimer()" (mouseleave)="hover.set(null)" (mouseup)="$event.stopPropagation()">
        <div class="hd">
          <span class="you">YOU</span>You
          <span class="tnum" style="font-size:var(--fs-badge);color:var(--ink-4)">· {{ lineLabel(hc.fromLine, hc.toLine) }}</span>
          <span class="chip" style="font-size:var(--fs-micro);padding:0 var(--sp-2);color:var(--ui-ink);border-color:var(--ui-sel-2)">pending</span>
        </div>
        <div style="white-space:pre-wrap;word-break:break-word">{{ hc.note }}</div>
        <div class="ft">
          <kj-button kjVariant="ghost" kjSize="xs" class="btn" (click)="remove(hc.id)"><app-icon name="trash" size="sm" />Remove</kj-button>
        </div>
      </div>
    }
  `,
})
export class MdReviewOverlayComponent {
  readonly agent = input.required<Agent>();
  readonly path = input.required<string>();
  /** The `.md-body` scroll container — events + coordinate origin. */
  readonly scroller = input.required<HTMLElement>();
  /** The `.rte-view` rendered document — selection containment. */
  readonly body = input.required<HTMLElement>();
  /** Bumped by the parent after every render pass; a pass moves the text, so
   *  a pending selection's rect is stale and gets dropped. */
  readonly renderGen = input<number>(0);
  /** Fired when the overlay left a `.md-sel` wrap behind (cancel/save) — the
   *  parent re-renders and re-highlights. (A remove needs no signal: the
   *  parent already re-renders on the store's comment list.) */
  readonly changed = output<void>();

  private review = inject(ReviewStore);
  private ui = inject(UiStore);
  private host = inject<ElementRef<HTMLElement>>(ElementRef);
  private destroyRef = inject(DestroyRef);
  private readonly ta = viewChild<ElementRef<HTMLTextAreaElement>>("ta");

  readonly pending = signal<Pending | null>(null);
  readonly compose = signal(false);
  readonly draft = signal("");
  readonly hover = signal<Hover | null>(null);
  private hoverTimer: ReturnType<typeof setTimeout> | null = null;

  private readonly agentId = computed(() => this.agent().id);
  /** This file's queued comments — the store signal keeps it live. */
  readonly comments = computed(() => {
    const path = this.path();
    return this.review.list(this.agentId()).filter((c) => c.file === path);
  });
  readonly hoverComment = computed(() => {
    const h = this.hover();
    return h ? (this.comments().find((c) => c.id === h.id) ?? null) : null;
  });

  readonly composerLeft = computed(() => {
    const p = this.pending();
    if (!p) return 0;
    const w = untracked(() => this.scroller().clientWidth) || 800;
    return Math.max(24, Math.min(p.x - 120, w - 384));
  });
  readonly hoverLeft = computed(() => Math.max(16, this.hover()?.x ?? 0));

  constructor() {
    // design: the scroller owns mouseup / mouseover / mouseout
    effect((onCleanup) => {
      const s = this.scroller();
      const up = (): void => this.onMouseUp();
      const over = (e: MouseEvent): void => this.onOver(e);
      const out = (e: MouseEvent): void => this.onOut(e);
      s.addEventListener("mouseup", up);
      s.addEventListener("mouseover", over);
      s.addEventListener("mouseout", out);
      onCleanup(() => {
        s.removeEventListener("mouseup", up);
        s.removeEventListener("mouseover", over);
        s.removeEventListener("mouseout", out);
      });
    });

    // a mousedown anywhere outside the overlay's own popups drops the pending
    // selection (a click on the note button stops propagation before this)
    const down = (e: MouseEvent): void => {
      if (!this.pending()) return;
      const t = e.target as Node | null;
      if (t && this.host.nativeElement.contains(t)) return;
      this.dismiss();
    };
    document.addEventListener("mousedown", down);
    this.destroyRef.onDestroy(() => {
      document.removeEventListener("mousedown", down);
      this.clearHoverTimer();
    });

    // another file, or a render pass that moved the text: the rect is stale
    effect(() => {
      this.path();
      this.renderGen();
      untracked(() => this.dismiss(false));
    });

    afterRenderEffect(() => {
      const el = this.ta()?.nativeElement;
      if (el && this.compose()) el.focus();
    });
  }

  lineLabel(from: number, to: number): string {
    return from === to ? `line ${from}` : `lines ${from}–${to}`;
  }

  /** Scroller content-box origin in viewport coords (design `box()`). */
  private box(): { l: number; t: number; w: number } {
    const s = this.scroller();
    const r = s.getBoundingClientRect();
    return { l: r.left - s.scrollLeft, t: r.top - s.scrollTop, w: s.clientWidth };
  }

  private onMouseUp(): void {
    if (this.compose()) return;
    const body = this.body();
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed || !body.contains(sel.anchorNode) || !body.contains(sel.focusNode)) {
      this.pending.set(null);
      return;
    }
    const range = sel.getRangeAt(0);
    const rects = range.getClientRects();
    if (!rects.length) return;
    const last = rects[rects.length - 1];
    const b = this.box();
    const { from, to } = rangeToLines(body, range);
    this.pending.set({
      snippet: sel.toString().replace(/\s+/g, " ").trim().slice(0, 160),
      fromLine: from,
      toLine: to,
      x: last.right - b.l,
      y: last.top - b.t,
      h: last.height,
      range: range.cloneRange(),
    });
  }

  /** mousedown, not click — it must run before the browser collapses the selection. */
  startCompose(e: MouseEvent): void {
    e.preventDefault();
    e.stopPropagation();
    const p = this.pending();
    if (!p) return;
    try {
      const span = document.createElement("span");
      span.className = "md-sel";
      span.appendChild(p.range.extractContents());
      p.range.insertNode(span);
    } catch {
      /* a range the DOM no longer supports — compose without the wrap */
    }
    window.getSelection()?.removeAllRanges();
    this.draft.set("");
    this.compose.set(true);
  }

  onKey(e: KeyboardEvent): void {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      this.save();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      this.cancel();
    }
  }

  onEscape(): void {
    if (this.compose()) this.cancel();
    else this.pending.set(null);
  }

  cancel(): void {
    this.dismiss();
  }

  save(): void {
    const p = this.pending();
    const note = this.draft().trim();
    if (!p || !note) return;
    const agent = this.agent();
    this.review.add(agent.id, {
      file: this.path(),
      view: "file",
      side: "file",
      lang: "markdown",
      fromLine: p.fromLine,
      toLine: p.toLine,
      snippet: p.snippet,
      lines: [],
      note,
      quote: p.snippet,
    });
    this.dismiss();
    this.ui.flash(`comment saved · ${fileName(this.path())}:${p.fromLine} · queued for ${agent.name}`);
  }

  remove(id: string): void {
    this.review.remove(this.agentId(), id);
    this.hover.set(null);
  }

  /**
   * Drop the pending selection. While composing the document carries a
   * `.md-sel` wrap: with `rerender` the parent re-renders (design `gen`
   * bump, which also re-applies highlights after a save); without it — a
   * render pass or file switch already invalidated the DOM — the wrap is
   * unwrapped in place so nothing lingers.
   */
  private dismiss(rerender = true): void {
    const wasComposing = this.compose();
    this.compose.set(false);
    this.pending.set(null);
    this.hover.set(null);
    if (!wasComposing) return;
    if (rerender) this.changed.emit();
    else unwrapSelection(this.body());
  }

  // ----- hover card over a saved mark -----

  private onOver(e: MouseEvent): void {
    const mk = (e.target as HTMLElement).closest<HTMLElement>(".md-mark");
    if (!mk) return;
    this.clearHoverTimer();
    const r = mk.getBoundingClientRect();
    const b = this.box();
    this.hover.set({ id: mk.dataset["id"] ?? "", x: Math.min(r.left - b.l - 140, b.w - 320), y: r.bottom - b.t + 6 });
  }

  private onOut(e: MouseEvent): void {
    if (!(e.target as HTMLElement).closest(".md-mark")) return;
    this.clearHoverTimer();
    this.hoverTimer = setTimeout(() => this.hover.set(null), HOVER_GRACE_MS);
  }

  clearHoverTimer(): void {
    if (this.hoverTimer) clearTimeout(this.hoverTimer);
    this.hoverTimer = null;
  }
}
