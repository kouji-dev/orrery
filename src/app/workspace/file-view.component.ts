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
} from "@angular/core";
import { DomSanitizer, SafeResourceUrl } from "@angular/platform-browser";
import { marked } from "marked";
import { Agent, BlameIntern, BlameLine, hydrateBlame } from "../models";
import { AgentsStore } from "../stores/agents.store";
import { EditsStore } from "../stores/edits.store";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName, isMarkdownPath, langId, langTag } from "../utils";
import { BRIDGE, Commands, FileHunk } from "../data-source/bridge";
import { renderMermaidBlocks } from "./md-mermaid";
import { MonacoFileEditorComponent } from "./monaco-file-editor.component";
import { ScrollStateService } from "./scroll-state.service";
import { AnnotateBlameComponent } from "./review/annotate-blame.component";
import { SendReviewButtonComponent } from "./review/send-review.component";
import { KjBadgeComponent, KjButtonComponent, KjTabComponent, KjTabListComponent, KjTabsComponent} from "@kouji-ui/components";

/** Don't try to render megabyte-scale documents in the editor. */
const MAX_CHARS = 1_500_000;

/**
 * Read-only single-file view for a pane's file tab. Content is the
 * working-tree text — fetched through the existing `agent_diff` command whose
 * `.new` side is exactly that — rendered by the shared UnifiedCodeComponent.
 * Markdown gets a Raw / Preview toggle. Annotate overlays per-line blame.
 */
@Component({
  selector: "app-file-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, MonacoFileEditorComponent, AnnotateBlameComponent, SendReviewButtonComponent, KjButtonComponent, KjBadgeComponent, KjTabsComponent, KjTabListComponent, KjTabComponent],
  template: `
    <!-- slim toolbar: path · changed-state · (md toggle) · annotate · lang · refresh -->
    <div class="pane-head" style="gap:var(--sp-3);padding-block:var(--sp-2);background:var(--panel);min-width:0">
      <app-icon size="md" name="file" color="var(--ink-3)" />
      <span class="trunc" [title]="path()">
        <span style="color:var(--ink-4)">{{ fdir(path()) }}</span>{{ fname(path()) }}
      </span>
      <div style="margin-left:auto;display:flex;align-items:center;gap:var(--sp-3);flex:none">
        @if (isMarkdown()) {
          <kj-tabs variant="pills" class="tabs-xs"
                   [value]="preview() ? 'preview' : 'raw'" (valueChange)="preview.set($event === 'preview')">
            <kj-tab-list aria-label="Markdown display mode">
              <kj-tab value="raw">Raw</kj-tab>
              <kj-tab value="preview">Preview</kj-tab>
            </kj-tab-list>
          </kj-tabs>
        }
        @if (kind() === 'text') {
        <kj-button kjVariant="outline" [kjPressed]="annotate()" (click)="annotate.set(!annotate())" title="Annotate — show who last changed each line">
          <app-icon size="md" name="git" [color]="annotate() ? 'var(--ui-ink)' : null" />
          Annotate
        </kj-button>
        }
        @if (tag()) { <kj-badge class="tnum" style="font-size:var(--fs-meta);padding:0 var(--sp-3)">{{ tag() }}</kj-badge> }
        <kj-button kjSize="icon" kjVariant="ghost" (click)="reload()" title="Reload from the worktree">
          <app-icon size="md" name="refresh" [class.set-spin]="loading()" />
        </kj-button>
        <app-send-review-button [agent]="agent().id" [agentName]="agent().name" />
      </div>
    </div>

    <!-- external-change conflict banner (B1.1): the file changed on disk while
         this buffer holds unsaved edits -->
    @if (conflict() !== null) {
      <div class="ec-banner">
        <app-icon size="md" name="refresh" color="var(--code-del-ink)" />
        <p>Changed on disk — an agent or another program modified this file.</p>
        <kj-button kjVariant="outline" style="margin-left:auto" (click)="acceptDisk()" title="Discard my edits and load the disk version">Reload</kj-button>
        <kj-button kjVariant="outline" (click)="keepMine()" title="Keep my edits — saving will overwrite the disk version">Keep mine</kj-button>
      </div>
    }

    <!-- body -->
    @if (kind() !== 'text') {
      <!-- B1.4: image / PDF preview (binary read, no text pipeline) -->
      @if (mediaError(); as me) {
        <div class="pane-empty pad" style="text-align:center">{{ me }}</div>
      } @else if (kind() === 'image' && mediaDataUrl()) {
        <div class="scroll-y media-body" (scroll)="onBodyScroll($event)">
          <img [src]="mediaDataUrl()" [alt]="fname(path())" />
        </div>
      } @else if (kind() === 'pdf' && mediaSafeUrl()) {
        <embed [src]="mediaSafeUrl()" type="application/pdf" style="flex:1;width:100%;min-height:0" />
      } @else {
        <div class="pane-empty">loading…</div>
      }
    } @else if (notice(); as n) {
      <div class="pane-empty pad" style="text-align:center">{{ n }}</div>
    } @else if (isMarkdown() && preview()) {
      <div class="scroll-y rte-view md-body" (scroll)="onBodyScroll($event)" style="flex:1;padding:var(--sp-7) var(--sp-8)" [innerHTML]="mdHtml()"></div>
    } @else if (annotate()) {
      <app-annotate-blame [lines]="blame()" (openCommit)="onOpenCommit($event)" />
    } @else {
      <app-monaco-file-editor [agent]="agent().id" [file]="path()" [newText]="content() ?? ''" [lang]="lid()" [syncGen]="syncGen()" [hunks]="hunks()" (revertHunk)="onRevertHunk($event)" />
    }
  `,
  styles: [
    `
      /* the flex/column/min-height fill comes from the shared app-file-view
         host rule in styles.css; only the ground is this component's own */
      :host {
        background: var(--bg);
      }
      /* Heading / code / pre / link styling is the shared .rte-view recipe.
         Only the mermaid block, which rte-view knows nothing about, stays. */
      .md-body ::ng-deep .mmd { margin: var(--sp-5) 0; overflow-x: auto; }
      .md-body ::ng-deep .mmd svg { max-width: 100%; height: auto; }
      .ec-banner {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        padding: var(--sp-2) var(--sp-6);
        background: color-mix(in oklch, var(--code-del-ink), transparent 90%);
        border-bottom: 1px solid color-mix(in oklch, var(--code-del-ink), transparent 70%);
        color: var(--ink-2);
        flex: none;
      }
      .media-body {
        flex: 1;
        display: grid;
        place-items: center;
        padding: var(--sp-7);
        background: var(--bg);
      }
      .media-body img {
        max-width: 100%;
        max-height: 100%;
        object-fit: contain;
        border-radius: var(--r-sm);
        box-shadow: var(--shadow);
      }
    `,
  ],
})
export class FileViewComponent {
  readonly agent = input.required<Agent>();
  readonly path = input.required<string>();

  private agents = inject(AgentsStore);
  private ui = inject(UiStore);
  private bridge = inject(BRIDGE);
  private edits = inject(EditsStore);
  private destroyRef = inject(DestroyRef);
  private host = inject<ElementRef<HTMLElement>>(ElementRef);
  private scroll = inject(ScrollStateService);
  /** Key the current body is showing — the reload effect fires with the NEXT
   *  agent/path already in the signals, so saving must use this, not them. */
  private bodyKey: { agent: string; path: string } | null = null;

  readonly loading = signal(false);
  readonly preview = signal(true); // markdown opens rendered; Raw is one click
  readonly annotate = signal(false);
  readonly content = signal<string | null>(null);
  private readonly error = signal<string | null>(null);
  readonly blame = signal<BlameLine[]>([]);
  /** Disk text when it diverged under a dirty buffer (drives the banner). */
  readonly conflict = signal<string | null>(null);
  /** B4.3: changed regions vs HEAD — the editor's gutter change markers. */
  readonly hunks = signal<FileHunk[]>([]);
  /** Bumped after a conflict resolution so the editor re-syncs its model. */
  readonly syncGen = signal(0);

  // ----- B1.4: image / PDF preview -----
  private sanitizer = inject(DomSanitizer);
  readonly mediaDataUrl = signal<string | null>(null); // images (data: is img-safe)
  readonly mediaSafeUrl = signal<SafeResourceUrl | null>(null); // pdf (blob: needs trust)
  readonly mediaError = signal<string | null>(null);
  private objectUrl: string | null = null;

  /** How this path renders: normal text pipeline, or a binary preview. */
  readonly kind = computed<"text" | "image" | "pdf">(() => {
    const p = this.path().toLowerCase();
    if (/\.(png|jpe?g|gif|webp|bmp|ico|avif|svg)$/.test(p)) return "image";
    if (/\.pdf$/.test(p)) return "pdf";
    return "text";
  });

  readonly fdir = fileDir;
  readonly fname = fileName;
  readonly isMarkdown = computed(() => isMarkdownPath(this.path()));
  readonly tag = computed(() => langTag(this.path()));
  readonly mdHtml = computed(() => (this.content() ? (marked.parse(this.content()!) as string) : ""));

  readonly lid = computed(() => langId(this.path()));

  /** Block rendering for unloadable / oversized / binary content. */
  readonly notice = computed<string | null>(() => {
    if (this.error()) return this.error();
    const c = this.content();
    if (c === null) return this.loading() ? "loading…" : null;
    if (c.length > MAX_CHARS) return "file too large to display";
    if (c.includes("\u0000")) return "binary file — no preview";
    return null;
  });

  private gen = 0;
  private blameGen = 0;

  constructor() {
    // (re)load when the pane shows a different agent/file
    effect(() => {
      const id = this.agent().id;
      const path = this.path();
      const prev = this.bodyKey;
      if (prev && (prev.agent !== id || prev.path !== path)) this.saveBodyScroll(prev);
      this.bodyKey = { agent: id, path };
      void this.load(id, path);
    });

    // Re-read on watcher pushes for this agent — a save's own echo lands as a
    // clean no-op (EditsStore baseText == disk); a real external change either
    // swaps silently (clean buffer) or raises the conflict banner (dirty).
    let unsub: (() => void) | null = null;
    void this.agents
      .onScan((p) => {
        if (p.id !== this.agent().id) return;
        void this.load(p.id, this.path());
      })
      .then((u) => (unsub = u));
    this.destroyRef.onDestroy(() => {
      // covers the pane switching this leaf to terminal/diff/git
      if (this.bodyKey) this.saveBodyScroll(this.bodyKey);
      unsub?.();
      this.dropObjectUrl();
    });

    // Load blame when annotate is on (or file/agent changes while on).
    effect(() => {
      const on = this.annotate();
      const id = this.agent().id;
      const path = this.path();
      if (!on) {
        this.blame.set([]);
        return;
      }
      const g = ++this.blameGen;
      void this.bridge
        .invoke<{ old: BlameIntern; new: BlameIntern }>(Commands.AgentWorkingBlame, { id, path })
        .then((r) => {
          if (this.blameGen !== g) return;
          this.blame.set(hydrateBlame(r.new));
        })
        .catch(() => {
          if (this.blameGen !== g) return;
          this.blame.set([]);
        });
    });

    // Mermaid fences in the rendered markdown preview. The [innerHTML] binding
    // lands during change detection, so wait two frames before touching the
    // DOM; re-runs on a theme toggle to restyle already-rendered diagrams.
    effect(() => {
      const html = this.mdHtml();
      const showing = this.isMarkdown() && this.preview() && this.kind() === "text" && this.notice() === null;
      const theme = this.ui.tweaks().theme;
      if (!showing || !html.includes("language-mermaid")) return;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const body = this.host.nativeElement.querySelector<HTMLElement>(".md-body");
          if (body) void renderMermaidBlocks(body, theme);
        }),
      );
    });

    // Restore a saved scroll offset once a plain body (md preview / media) is
    // showing. Same two-frame wait as the mermaid effect — the [innerHTML] /
    // [src] bindings land during change detection. Re-applying to an already
    // positioned body is a no-op, so no showing-vs-reloading distinction.
    effect(() => {
      const showingMd = this.isMarkdown() && this.preview() && this.kind() === "text" && this.notice() === null && this.content();
      const showingMedia = this.kind() === "image" && this.mediaDataUrl();
      if (!showingMd && !showingMedia) return;
      const top = untracked(() => this.scroll.getPlain(this.agent().id, this.path()));
      if (top === undefined) return;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const body = this.host.nativeElement.querySelector<HTMLElement>(".scroll-y");
          if (body) body.scrollTop = top;
        }),
      );
    });
  }

  onBodyScroll(e: Event): void {
    const el = e.target as HTMLElement;
    this.scroll.savePlain(this.agent().id, this.path(), el.scrollTop);
  }

  private saveBodyScroll(key: { agent: string; path: string }): void {
    const body = this.host.nativeElement.querySelector<HTMLElement>(".scroll-y");
    if (body) this.scroll.savePlain(key.agent, key.path, body.scrollTop);
  }

  reload() {
    void this.load(this.agent().id, this.path());
  }

  /** Conflict banner: adopt the disk version, dropping unsaved edits. */
  acceptDisk() {
    const disk = this.conflict();
    if (disk === null) return;
    this.edits.discard(this.agent().id, this.path(), disk);
    this.conflict.set(null);
    this.syncGen.update((n) => n + 1);
  }

  /** Conflict banner: keep the buffer — the next save overwrites the disk. */
  keepMine() {
    this.conflict.set(null);
  }

  /** B4.3 gutter marker revert — the watcher's rescan refreshes everything. */
  onRevertHunk(h: FileHunk) {
    if (this.edits.isDirty(this.agent().id, this.path())) {
      this.ui.flash("Save or discard your edits before reverting a hunk");
      return;
    }
    void this.bridge
      .invoke(Commands.AgentHunkRevert, { id: this.agent().id, path: this.path(), newStart: h.newStart })
      .catch((e) => this.ui.flash(e instanceof Error ? e.message : String(e)));
  }

  onOpenCommit(sha: string) {
    this.ui.setGitView(this.agent().id, { kind: "commit", sha });
  }

  private async load(id: string, path: string) {
    if (this.kind() !== "text") {
      await this.loadMedia(id, path);
      return;
    }
    const g = ++this.gen;
    this.loading.set(true);
    this.error.set(null);
    try {
      // `.new` = current working-tree content (HEAD side unused here)
      const d = await this.agents.diff(id, path);
      if (g !== this.gen) return;
      this.content.set(d.new);
      const buf = this.edits.get(id, path);
      this.conflict.set(buf?.dirty && d.new !== buf.baseText ? d.new : null);
      // change markers ride along with every content (re)load
      void this.bridge
        .invoke<FileHunk[]>(Commands.AgentFileHunks, { id, path })
        .then((h) => {
          if (g === this.gen) this.hunks.set(h);
        })
        .catch(() => {
          if (g === this.gen) this.hunks.set([]);
        });
    } catch (e) {
      if (g !== this.gen) return;
      this.error.set("could not read file: " + (e instanceof Error ? e.message : e));
    } finally {
      if (g === this.gen) this.loading.set(false);
    }
  }

  /** Binary read → data URL (images) or trusted blob URL (pdf). */
  private async loadMedia(id: string, path: string) {
    const g = ++this.gen;
    this.loading.set(true);
    this.mediaError.set(null);
    try {
      const f = await this.bridge.invoke<{ base64: string; mime: string; len: number }>(
        Commands.FileReadBinary,
        { id, path },
      );
      if (g !== this.gen) return;
      this.dropObjectUrl();
      if (this.kind() === "image") {
        this.mediaDataUrl.set(`data:${f.mime};base64,${f.base64}`);
        this.mediaSafeUrl.set(null);
      } else {
        const bytes = Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0));
        this.objectUrl = URL.createObjectURL(new Blob([bytes], { type: f.mime }));
        this.mediaSafeUrl.set(this.sanitizer.bypassSecurityTrustResourceUrl(this.objectUrl));
        this.mediaDataUrl.set(null);
      }
    } catch (e) {
      if (g !== this.gen) return;
      this.mediaError.set("could not preview file: " + (e instanceof Error ? e.message : e));
      this.mediaDataUrl.set(null);
      this.mediaSafeUrl.set(null);
    } finally {
      if (g === this.gen) this.loading.set(false);
    }
  }

  private dropObjectUrl() {
    if (this.objectUrl) {
      URL.revokeObjectURL(this.objectUrl);
      this.objectUrl = null;
    }
  }
}
