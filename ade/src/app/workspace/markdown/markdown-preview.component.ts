import {
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  input,
  signal,
  untracked,
  viewChild,
} from "@angular/core";
import { ReviewStore } from "../../agents/review.store";
import { CommandRegistryService } from "../../commands/command-registry.service";
import { Agent } from "../../models";
import { UiStore } from "../../ui/ui.store";
import { fileDir } from "../../utils";
import { ScrollStateService } from "../scroll-state.service";
import { createMarked } from "./marked.config";
import { renderCodeBlocks } from "./md-code";
import { applyHighlight } from "./md-highlight";
import { renderMermaidBlocks } from "./md-mermaid";
import { MdReviewOverlayComponent } from "./md-review-overlay.component";
import { blockSpans, stampBlocks } from "./md-source-map";

/**
 * Rendered markdown for a file tab (the design's MarkdownPreview body):
 * marked → `.rte-view` reading column inside the `.md-body` scroller, mermaid
 * fences hydrated into diagram boxes, every top-level block stamped with its
 * source lines so a selection can be turned into a line-anchored comment.
 *
 * The diagram toolbars are markup built outside Angular (see md-mermaid.ts),
 * so their actions arrive through ONE delegated click handler on the scroller
 * keyed by `data-act`. "Open full size" clones the SVG into a fixed scrim —
 * cloned as a node, not re-bound through [innerHTML], because the sanitizer
 * would strip mermaid's <style> from it.
 *
 * Selection → note lives in <app-md-review-overlay>, mounted inside the
 * scroller so its popups scroll with the text. Saved comments are painted
 * back as `.md-hl` wraps at the end of every render pass; the overlay's
 * `(changed)` bumps `gen`, which forces a re-render from the html string so
 * an abandoned `.md-sel` wrap disappears the same way the design's does.
 */
@Component({
  selector: "app-markdown-preview",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MdReviewOverlayComponent],
  host: { "(document:keydown.escape)": "closeFull()" },
  template: `
    <div #scroller class="scroll-y md-body" style="flex:1;min-width:0;position:relative" (scroll)="onScroll($event)" (click)="onClick($event)">
      <div #view class="rte-view" [innerHTML]="html()"></div>
      <app-md-review-overlay [agent]="agent()" [path]="path()" [scroller]="scroller" [body]="view"
                             [renderGen]="renderGen()" (changed)="onOverlayChanged()" />
    </div>
    @if (full()) {
      <div role="dialog" aria-modal="true" aria-label="Diagram, full size" (click)="closeFull()"
           style="position:fixed;inset:0;z-index:80;display:grid;place-items:center;background:var(--scrim);backdrop-filter:blur(3px);padding:var(--sp-11)">
        <div #fullBox class="surface md-full" (click)="$event.stopPropagation()"
             style="padding:var(--sp-8);max-width:92vw;max-height:88vh;overflow:auto;box-shadow:var(--shadow)"></div>
        <span style="position:absolute;bottom:var(--sp-7);font-size:var(--fs-badge);color:var(--ink-3)"><span class="kbd">esc</span> close</span>
      </div>
    }
  `,
  styles: [
    `
      /* fill the file view's column; the reading-column look is the shared
         .md-body / .rte-view / .md-box recipes in styles.css */
      :host {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
        min-width: 0;
      }
    `,
  ],
})
export class MarkdownPreviewComponent {
  readonly source = input.required<string>();
  readonly agent = input.required<Agent>();
  readonly path = input.required<string>();

  private ui = inject(UiStore);
  private registry = inject(CommandRegistryService);
  private review = inject(ReviewStore);
  private scroll = inject(ScrollStateService);
  private destroyRef = inject(DestroyRef);
  private readonly view = viewChild.required<ElementRef<HTMLElement>>("view");
  private readonly fullBox = viewChild<ElementRef<HTMLElement>>("fullBox");
  private readonly md = createMarked();

  /** Key the current body is showing — the input effect fires with the NEXT
   *  agent/path already in the signals, so saving must use this, not them. */
  private bodyKey: { agent: string; path: string } | null = null;

  readonly html = computed(() => this.md.parse(this.source()) as string);
  private readonly spans = computed(() => blockSpans(this.source()));
  /** The SVG shown in the full-size scrim, or null when closed. */
  readonly full = signal<SVGElement | null>(null);
  /** Design `gen`: bumped by the overlay to force a re-render from `html`. */
  private readonly gen = signal(0);
  /** Bumped after every render pass; the overlay drops a stale selection on it. */
  readonly renderGen = signal(0);
  /** html string the DOM currently holds — a pass with the same string must
   *  reset [innerHTML] by hand (Angular only rebinds on change). */
  private lastHtml: string | null = null;
  /** Monotonic pass id: an async mermaid step from a superseded pass must not
   *  stamp/highlight the DOM a newer pass already rebuilt. */
  private passId = 0;

  // Stable key: runtime overlay patches re-create the Agent OBJECT many times
  // a second — the id string is memoized so the scroll effect fires only on a
  // real agent switch.
  private readonly agentId = computed(() => this.agent().id);
  /** This file's queued comments (live on the store signal). */
  private readonly comments = computed(() => {
    const path = this.path();
    return this.review.list(this.agentId()).filter((c) => c.file === path);
  });
  /** Re-render key — ids only, so a hover elsewhere never re-paints. */
  private readonly commentsKey = computed(() => this.comments().map((c) => c.id).join(","));

  constructor() {
    // save the outgoing document's offset when the tab shows another file
    effect(() => {
      const id = this.agentId();
      const path = this.path();
      const prev = this.bodyKey;
      if (prev && (prev.agent !== id || prev.path !== path)) this.saveBodyScroll(prev);
      this.bodyKey = { agent: id, path };
    });
    this.destroyRef.onDestroy(() => {
      // covers Raw toggle and the pane switching this leaf to terminal/diff/git
      if (this.bodyKey) this.saveBodyScroll(this.bodyKey);
    });

    // Stamp source lines, hydrate mermaid and colour the code fences once the
    // document is in the DOM. The [innerHTML] binding lands during change
    // detection, so wait two frames before touching the DOM; re-runs on a
    // theme toggle to restyle already-rendered diagrams and re-colour the
    // fences (the html string itself is unchanged then).
    // Also re-runs on the overlay's `gen` and on the comment list (design
    // deps [html, key, gen]) — those keep the same html string, so the DOM is
    // rebuilt from it by hand before stamping, dropping stale wraps.
    effect(() => {
      const html = this.html();
      const theme = this.ui.tweaks().theme;
      this.gen();
      this.commentsKey();
      const pass = ++this.passId;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (pass !== this.passId) return;
          const body = this.view().nativeElement;
          if (this.lastHtml === html) body.innerHTML = html;
          this.lastHtml = html;
          stampBlocks(body, untracked(() => this.spans()));
          // Both hydration steps are lazy chunks, so they are asked for only
          // when the html actually holds the block they own.
          const work: Promise<unknown>[] = [];
          if (html.includes("language-mermaid")) work.push(renderMermaidBlocks(body, theme));
          if (html.includes("md-box fence")) work.push(renderCodeBlocks(body, theme));
          if (work.length === 0) {
            this.finishPass(body);
            return;
          }
          // a hydrated box replaces its placeholder in place — same index, no
          // stamp: re-stamp so the diagram carries its lines too
          void Promise.all(work).then(() => {
            if (pass !== this.passId) return;
            stampBlocks(body, untracked(() => this.spans()));
            this.finishPass(body);
          });
        }),
      );
    });

    // Restore a saved scroll offset once a document is showing. Same two-frame
    // wait — re-applying to an already positioned body is a no-op, so no
    // showing-vs-reloading distinction.
    effect(() => {
      if (!this.html()) return;
      const top = untracked(() => this.scroll.getPlain(this.agent().id, this.path()));
      if (top === undefined) return;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const body = this.view().nativeElement.parentElement;
          if (body) body.scrollTop = top;
        }),
      );
    });

    // the scrim's box exists only while open: clone the SVG in when both are there
    effect(() => {
      const box = this.fullBox()?.nativeElement;
      const svg = this.full();
      if (!box || !svg) return;
      const wrap = document.createElement("div");
      wrap.className = "mmd";
      wrap.appendChild(svg.cloneNode(true));
      box.replaceChildren(wrap);
    });
  }

  /** End of a render pass: paint the saved comments back, then tell the
   *  overlay the text moved. Highlights go last so the wraps never sit under
   *  a mermaid swap or a stamp by index. */
  private finishPass(body: HTMLElement): void {
    untracked(() => this.comments()).forEach((c) => applyHighlight(body, c));
    this.renderGen.update((g) => g + 1);
  }

  /** The overlay closed a composer (cancel/save): rebuild from the html string. */
  onOverlayChanged(): void {
    this.gen.update((g) => g + 1);
  }

  onScroll(e: Event): void {
    const el = e.target as HTMLElement;
    this.scroll.savePlain(this.agent().id, this.path(), el.scrollTop);
  }

  private saveBodyScroll(key: { agent: string; path: string }): void {
    const body = this.view().nativeElement.parentElement;
    if (body) this.scroll.savePlain(key.agent, key.path, body.scrollTop);
  }

  /** Diagram / error box toolbar actions (design `onClick`), delegated. */
  onClick(e: MouseEvent): void {
    // A link inside [innerHTML] is a real <a href>: left alone, the webview
    // navigates to it — which unloads the whole app (the "reload" the user
    // saw). Every link is intercepted and routed by what it points at.
    const link = (e.target as HTMLElement).closest<HTMLAnchorElement>("a[href]");
    if (link) {
      e.preventDefault();
      e.stopPropagation();
      this.followLink(link.getAttribute("href") ?? "");
      return;
    }
    const act = (e.target as HTMLElement).closest<HTMLElement>("[data-act]");
    if (!act) return;
    const box = act.closest<HTMLElement>(".md-box");
    const src = box?.dataset["mmdSrc"] ?? "";
    switch (act.dataset["act"]) {
      case "copy":
        void navigator.clipboard?.writeText(src).catch(() => {});
        this.ui.flash(`copied diagram source · ${src.split("\n").length} lines`);
        break;
      case "full": {
        // guard: an error box (or a diagram that lost its SVG) must not open an
        // empty scrim — leave the click as a no-op instead
        const svg = diagramSvg(box);
        if (svg) this.full.set(svg);
        break;
      }
      case "toggle-src": {
        const pre = box?.querySelector<HTMLElement>("pre.src");
        if (!pre) break;
        pre.hidden = !pre.hidden;
        act.classList.toggle("open", !pre.hidden);
        break;
      }
    }
    e.stopPropagation();
  }

  closeFull(): void {
    this.full.set(null);
  }

  /**
   * `#heading` scrolls within this document; `scheme:` links hand off to the
   * OS browser; anything else is a path relative to THIS file and opens as
   * another file tab in the same worktree — the way a doc's cross-links read.
   */
  private followLink(href: string): void {
    if (!href) return;
    if (href.startsWith("#")) {
      this.scrollToHeading(decodeURIComponent(href.slice(1)));
      return;
    }
    if (/^[a-z][a-z0-9+.-]*:/i.test(href)) {
      import("@tauri-apps/plugin-opener")
        .then((m) => m.openUrl(href))
        .then(() => this.ui.flash("opened in browser"))
        .catch((err: { message?: string }) => this.ui.flash("could not open link: " + (err?.message ?? err)));
      return;
    }
    const target = resolveRelative(fileDir(this.path()), href.replace(/[#?].*$/, ""));
    if (target) this.registry.openFileAt(this.agent().id, target);
  }

  /** marked emits no heading ids (and the sanitizer would strip them), so an
   *  anchor resolves by slug against the rendered headings' text. */
  private scrollToHeading(anchor: string): void {
    const want = slug(anchor);
    const heads = this.view().nativeElement.querySelectorAll<HTMLElement>("h1,h2,h3,h4,h5,h6");
    const hit = Array.from(heads).find((h) => slug(h.textContent ?? "") === want);
    if (hit) hit.scrollIntoView({ block: "start" });
    else this.ui.flash("no heading matches #" + anchor);
  }
}

/**
 * The mermaid drawing inside a diagram box — and only that.
 *
 * `.md-box.diagram` opens with its toolbar, whose buttons carry 12px `<svg>`
 * icons (md-mermaid `svgIcon`); those come FIRST in document order, so a plain
 * `box.querySelector("svg")` hands back the Copy-source icon and "Open full
 * size" showed a tiny glyph in the scrim. The diagram lives in the `.mmd`
 * scroll container, so anchor the lookup there.
 */
export function diagramSvg(box: HTMLElement | null | undefined): SVGElement | null {
  return box?.querySelector<SVGElement>(".box-scroll.mmd svg") ?? null;
}

/** GitHub-style heading slug: lower-case, punctuation dropped, each space →
 *  `-` (one-to-one, so `a & b` → `a--b` exactly as GitHub anchors are copied). */
export function slug(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\w\s-]/g, "")
    .replace(/\s/g, "-");
}

/** Join `href` onto `dir`, collapsing `.` and `..` — pure path math, no fs. */
export function resolveRelative(dir: string, href: string): string {
  if (!href) return "";
  const parts = href.startsWith("/") ? [] : dir.split("/").filter(Boolean);
  for (const seg of href.split("/")) {
    if (!seg || seg === ".") continue;
    if (seg === "..") parts.pop();
    else parts.push(seg);
  }
  return parts.join("/");
}
