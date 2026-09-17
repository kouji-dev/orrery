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
import { Agent, BlameIntern, BlameLine, hydrateBlame, VirtualDoc } from "../models";
import { AgentsStore } from "../stores/agents.store";
import { EditsStore } from "../stores/edits.store";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName, isMarkdownPath, langId, langTag } from "../utils";
import { BRIDGE, Commands, FileHunk } from "../data-source/bridge";
import { MarkdownPreviewComponent } from "./markdown/markdown-preview.component";
import { MonacoFileEditorComponent } from "./monaco-file-editor.component";
import { ScrollStateService } from "./scroll-state.service";
import { SendReviewButtonComponent } from "./review/send-review.component";
import { locationDetail } from "./nav-providers";
import { isVirtualUri, libCrumbs, virtualFileName, virtualLang, VirtualDocService } from "./virtual-doc";
import { KjBadgeComponent, KjButtonComponent, KjTabComponent, KjTabListComponent, KjTabsComponent} from "@kouji-ui/components";

/** Don't try to render megabyte-scale documents in the editor. */
const MAX_CHARS = 1_500_000;

/**
 * Read-only single-file view for a pane's file tab. Content is the
 * working-tree text — fetched through the existing `agent_diff` command whose
 * `.new` side is exactly that — rendered by the shared UnifiedCodeComponent.
 * Markdown gets a Raw / Preview toggle (the preview itself is
 * MarkdownPreviewComponent). Annotate overlays per-line blame.
 */
@Component({
  selector: "app-file-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, MarkdownPreviewComponent, MonacoFileEditorComponent, SendReviewButtonComponent, KjButtonComponent, KjBadgeComponent, KjTabsComponent, KjTabListComponent, KjTabComponent],
  template: `
    @if (kind() === 'virtual') {
      <!-- M3 virtual read-only doc (design LibDocToolbar + LibBanner): the
           payload's title, the read-only badge, the language — no Annotate,
           no reload, no review. M4: a library entry shows its crumbs
           ("JDK 21 · java.base · java.util") before the file name. -->
      <div class="lib-bar" data-testid="lib-bar">
        <app-icon name="box" size="sm" class="ch" />
        @for (c of vcrumbs().crumbs; track $index) {
          <span class="c">{{ c }}</span><app-icon name="chevron" size="sm" class="ch" />
        }
        <span class="c on trunc" [title]="vtitle()">{{ vcrumbs().name }}</span>
        <div class="r">
          <kj-badge class="lib-badge" variant="outline">read-only · library</kj-badge>
          @if (vlang()) { <kj-badge class="tnum" style="font-size:var(--fs-meta);padding:0 var(--sp-3)">{{ vlang() }}</kj-badge> }
        </div>
      </div>
    } @else {
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
    }

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
    @if (kind() === 'virtual') {
      @if (notice(); as n) {
        <div class="pane-empty pad" style="text-align:center">{{ n }}</div>
      } @else {
        <div class="lib-banner" data-testid="lib-banner">
          <app-icon name="lock" size="sm" />This file comes from a library source and cannot be edited.<span class="mono">{{ path() }}</span>
        </div>
        <app-monaco-file-editor [agent]="agent().id" [file]="path()" [newText]="content() ?? ''" [lang]="vlang()" [readOnly]="true" />
      }
    } @else if (kind() !== 'text') {
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
    } @else if (isMarkdown() && preview() && !annotate()) {
      <app-markdown-preview [source]="content()!" [agent]="agent()" [path]="path()" />
    } @else {
      <!-- Annotate outranks the markdown preview (Preview is the default for
           .md, so a lower branch made the pill a silent no-op there), and it
           annotates the EDITOR rather than replacing it: blame rides in as an
           injected column, so the file itself — highlighting, folding, find,
           scrolling — is never taken away. The strip only reports the states
           where there is no column to show. -->
      @if (annotate()) {
        @if (blameLoading()) {
          <div class="blame-strip">annotating…</div>
        } @else if (blameError(); as be) {
          <div class="blame-strip err">blame failed: {{ be }}</div>
        } @else if (blame().length === 0) {
          <div class="blame-strip">no history for this file yet</div>
        }
      }
      <app-monaco-file-editor [agent]="agent().id" [file]="path()" [newText]="content() ?? ''" [lang]="lid()" [syncGen]="syncGen()" [hunks]="hunks()" [blame]="blame()" (revertHunk)="onRevertHunk($event)" (openCommit)="onOpenCommit($event)" />
    }
  `,
  styles: [
    `
      /* the flex/column/min-height fill comes from the shared app-file-view
         host rule in styles.css; only the ground is this component's own */
      :host {
        background: var(--bg);
      }
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
      /* Annotate status line — sits above the editor, never instead of it */
      .blame-strip {
        flex: none;
        padding: var(--sp-2) var(--sp-6);
        background: var(--panel-2);
        border-bottom: 1px solid var(--hair);
        color: var(--ink-3);
        font-size: var(--fs-meta);
      }
      .blame-strip.err {
        color: var(--code-del-ink);
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
  /** Blame request in flight — the pane says so instead of sitting blank. */
  readonly blameLoading = signal(false);
  /** Backend error message from the last blame request; null when it succeeded. */
  readonly blameError = signal<string | null>(null);
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
  /** id:path:mime:len:bytes of the media currently shown — scan-echo reloads
   *  with identical content skip the src swap (see loadMedia). */
  private lastMediaSig: string | null = null;

  /** How this path renders: normal text pipeline, a binary preview, or (M3)
   *  a virtual read-only document behind a non-worktree uri. */
  readonly kind = computed<"text" | "image" | "pdf" | "virtual">(() => {
    if (isVirtualUri(this.path())) return "virtual";
    const p = this.path().toLowerCase();
    if (/\.(png|jpe?g|gif|webp|bmp|ico|avif|svg)$/.test(p)) return "image";
    if (/\.pdf$/.test(p)) return "pdf";
    return "text";
  });

  readonly fdir = fileDir;
  readonly fname = fileName;
  readonly isMarkdown = computed(() => isMarkdownPath(this.path()));
  readonly tag = computed(() => langTag(this.path()));

  readonly lid = computed(() => langId(this.path()));

  // ----- M3: virtual read-only doc -----
  private virtual = inject(VirtualDocService);
  /** The last `nav_virtual_read` payload (title + language) for this uri. */
  readonly vdoc = signal<VirtualDoc | null>(null);
  readonly vname = computed(() => virtualFileName(this.path()));
  readonly vlang = computed(() => this.vdoc()?.language || virtualLang(this.path()));
  /** M4: design LibDocToolbar crumbs + name (see `libCrumbs`). */
  readonly vcrumbs = computed(() => libCrumbs(this.path(), this.vdoc()?.title || this.vname()));
  /** Tooltip of the name: the payload's full title, then the hit's
   *  fully-qualified name when the navigation answer carried one. */
  readonly vtitle = computed(() => {
    const detail = locationDetail(this.path());
    const title = this.vdoc()?.title || this.path();
    return detail && detail !== title ? `${title}\n${detail}` : title;
  });

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

  // Stable key: runtime overlay patches re-create the Agent OBJECT many times
  // a second while it runs — effects reading agent() directly refire (and
  // refetched media previews swap their src = visible flashing). The id string
  // is memoized, so these effects fire only on a real agent switch.
  private readonly agentId = computed(() => this.agent().id);

  constructor() {
    // (re)load when the pane shows a different agent/file
    effect(() => {
      const id = this.agentId();
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
      const id = this.agentId();
      const path = this.path();
      if (!on) {
        this.blame.set([]);
        this.blameLoading.set(false);
        this.blameError.set(null);
        return;
      }
      const g = ++this.blameGen;
      this.blameLoading.set(true);
      this.blameError.set(null);
      void this.bridge
        .invoke<{ old: BlameIntern; new: BlameIntern }>(Commands.AgentWorkingBlame, { id, path })
        .then((r) => {
          if (this.blameGen !== g) return;
          this.blame.set(hydrateBlame(r.new));
          this.blameLoading.set(false);
        })
        .catch((e) => {
          if (this.blameGen !== g) return;
          // surface the backend's message — swallowing it left a blank pane
          // and nothing in the log to explain why
          this.blame.set([]);
          this.blameError.set(e instanceof Error ? e.message : String(e));
          this.blameLoading.set(false);
        });
    });

    // Restore a saved scroll offset once a media body is showing (the markdown
    // preview restores its own). Two-frame wait — the [src] binding lands
    // during change detection. Re-applying to an already positioned body is a
    // no-op, so no showing-vs-reloading distinction.
    effect(() => {
      if (this.kind() !== "image" || !this.mediaDataUrl()) return;
      const top = untracked(() => this.scroll.getPlain(this.agent().id, this.path()));
      if (top === undefined) return;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          const body = this.host.nativeElement.querySelector<HTMLElement>(".media-body");
          if (body) body.scrollTop = top;
        }),
      );
    });
  }

  onBodyScroll(e: Event): void {
    const el = e.target as HTMLElement;
    this.scroll.savePlain(this.agent().id, this.path(), el.scrollTop);
  }

  /** Media body only — the markdown preview saves its own scroller. */
  private saveBodyScroll(key: { agent: string; path: string }): void {
    const body = this.host.nativeElement.querySelector<HTMLElement>(".media-body");
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
    if (this.kind() === "virtual") {
      await this.loadVirtual(path);
      return;
    }
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

  /** M3: one memoized `nav_virtual_read` — no buffer, no hunks, no conflict. */
  private async loadVirtual(uri: string) {
    const g = ++this.gen;
    this.loading.set(true);
    this.error.set(null);
    this.hunks.set([]);
    this.conflict.set(null);
    try {
      const d = await this.virtual.read(uri);
      if (g !== this.gen) return;
      this.vdoc.set(d);
      this.content.set(d.text);
    } catch (e) {
      if (g !== this.gen) return;
      this.error.set("could not read document: " + (e instanceof Error ? e.message : e));
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
      // identical bytes (a watcher-scan echo) keep the current URL — swapping
      // the img/embed src for the same content flashes the preview
      const sig = `${id}:${path}:${f.mime}:${f.base64.length}:${f.base64}`;
      if (sig === this.lastMediaSig && (this.mediaDataUrl() || this.mediaSafeUrl())) return;
      this.lastMediaSig = sig;
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
