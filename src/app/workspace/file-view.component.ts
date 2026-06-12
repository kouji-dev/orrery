import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
  inject,
  input,
  OnDestroy,
  signal,
  viewChild,
} from "@angular/core";
import type { EditorView } from "@codemirror/view";
import { marked } from "marked";
import { Agent } from "../models";
import { AgentsStore } from "../stores/agents.store";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName, langId, langTag } from "../utils";
import { buildTheme, CMCore, loadCMCore, loadLangExt } from "./code-lang";

/** Don't try to render megabyte-scale documents in the editor. */
const MAX_CHARS = 1_500_000;

/**
 * Read-only single-file view for a pane's file tab (reincarnation of the old
 * standalone file-viewer, now living INSIDE the pane tree). Content is the
 * working-tree text — fetched through the existing `agent_diff` command whose
 * `.new` side is exactly that — highlighted by the same lazily-loaded
 * CodeMirror core the diff view uses. Markdown gets a Raw / Preview toggle.
 */
@Component({
  selector: "app-file-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <!-- slim toolbar: path · changed-state · (md toggle) · lang · refresh -->
    <div style="display:flex;align-items:center;gap:7px;padding:5px 12px;background:var(--panel);border-bottom:1px solid var(--hair);font-size:11px;flex:none;min-width:0">
      <app-icon name="file" size="sm" [px]="12" color="var(--ink-3)" />
      <span style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [title]="path()">
        <span style="color:var(--ink-4)">{{ fdir(path()) }}</span>{{ fname(path()) }}
      </span>
      <div style="margin-left:auto;display:flex;align-items:center;gap:7px;flex:none">
        @if (isMarkdown()) {
          <div style="display:flex;gap:2px;padding:2px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm)">
            <button class="btn" (click)="preview.set(false)"
              [style.background]="!preview() ? 'var(--panel-3)' : 'transparent'"
              [style.color]="!preview() ? 'var(--ink)' : 'var(--ink-3)'"
              style="padding:2px 7px;border-radius:4px;font-size:10px">Raw</button>
            <button class="btn" (click)="preview.set(true)"
              [style.background]="preview() ? 'var(--panel-3)' : 'transparent'"
              [style.color]="preview() ? 'var(--ink)' : 'var(--ink-3)'"
              style="padding:2px 7px;border-radius:4px;font-size:10px">Preview</button>
          </div>
        }
        @if (tag()) { <span class="chip tnum" style="font-size:9px;padding:0 6px">{{ tag() }}</span> }
        <button class="btn" (click)="reload()" title="Reload from the worktree" style="padding:3px;border-radius:4px">
          <app-icon name="refresh" size="sm" [px]="12" [class.set-spin]="loading()" />
        </button>
      </div>
    </div>

    <!-- body -->
    @if (notice(); as n) {
      <div style="flex:1;display:grid;place-items:center;color:var(--ink-4);font-size:11px;padding:18px;text-align:center">{{ n }}</div>
    } @else if (isMarkdown() && preview()) {
      <div class="scroll-y md-body" style="flex:1;padding:16px 20px" [innerHTML]="mdHtml()"></div>
    } @else {
      <div #host style="flex:1;min-height:0;overflow:auto;font-size:12.5px"></div>
    }
  `,
  styles: [
    `
      :host {
        flex: 1;
        min-height: 0;
        display: flex;
        flex-direction: column;
        background: var(--bg);
      }
      :host ::ng-deep .cm-editor { height: 100%; background-color: var(--bg) !important; }
      :host ::ng-deep .cm-gutters { min-height: 100%; background-color: var(--bg) !important; }
      :host ::ng-deep .cm-content { min-height: 100%; }
      .md-body { font-size: 12.5px; line-height: 1.7; color: var(--ink-2); }
      .md-body ::ng-deep h1, .md-body ::ng-deep h2, .md-body ::ng-deep h3 { color: var(--ink); margin: 14px 0 6px; }
      .md-body ::ng-deep code { background: var(--panel-2); padding: 1px 5px; border-radius: 4px; font-size: 11.5px; }
      .md-body ::ng-deep pre { background: var(--panel-2); padding: 10px 12px; border-radius: 8px; overflow-x: auto; }
      .md-body ::ng-deep a { color: var(--accent-2); }
    `,
  ],
})
export class FileViewComponent implements OnDestroy {
  readonly agent = input.required<Agent>();
  readonly path = input.required<string>();

  private agents = inject(AgentsStore);
  private ui = inject(UiStore);
  private host = viewChild<ElementRef<HTMLElement>>("host");

  readonly loading = signal(false);
  readonly preview = signal(true); // markdown opens rendered; Raw is one click
  private readonly content = signal<string | null>(null);
  private readonly error = signal<string | null>(null);

  readonly fdir = fileDir;
  readonly fname = fileName;
  readonly isMarkdown = computed(() => /\.(md|markdown)$/i.test(this.path()));
  readonly tag = computed(() => langTag(this.path()));
  readonly mdHtml = computed(() => (this.content() ? (marked.parse(this.content()!) as string) : ""));

  /** Block rendering for unloadable / oversized / binary content. */
  readonly notice = computed<string | null>(() => {
    if (this.error()) return this.error();
    const c = this.content();
    if (c === null) return this.loading() ? "loading…" : null;
    if (c.length > MAX_CHARS) return "file too large to display";
    if (c.includes("\u0000")) return "binary file — no preview";
    return null;
  });

  private view?: EditorView;
  private gen = 0; // invalidates in-flight loads AND late lazy-parser arrivals

  constructor() {
    // (re)load when the pane shows a different agent/file
    effect(() => {
      const id = this.agent().id;
      const path = this.path();
      void this.load(id, path);
    });
    // (re)build the editor on content / theme / view-mode changes
    effect(() => {
      const c = this.content();
      const theme = this.ui.tweaks().theme;
      const md = this.isMarkdown() && this.preview();
      const blocked = this.notice() !== null;
      const el = this.host()?.nativeElement;
      if (c === null || md || blocked || !el) return;
      void this.render(el, c, theme);
    });
  }

  ngOnDestroy() {
    this.view?.destroy();
  }

  reload() {
    void this.load(this.agent().id, this.path());
  }

  private async load(id: string, path: string) {
    const g = ++this.gen;
    this.loading.set(true);
    this.error.set(null);
    try {
      // `.new` = current working-tree content (HEAD side unused here)
      const d = await this.agents.diff(id, path);
      if (g !== this.gen) return;
      this.content.set(d.new);
    } catch (e) {
      if (g !== this.gen) return;
      this.error.set("could not read file: " + (e instanceof Error ? e.message : e));
    } finally {
      if (g === this.gen) this.loading.set(false);
    }
  }

  private async render(el: HTMLElement, doc: string, theme: "dark" | "light") {
    const g = this.gen;
    this.view?.destroy();
    this.view = undefined;
    el.textContent = "loading…";
    let cm: CMCore;
    try {
      cm = await loadCMCore();
    } catch {
      // CDN unavailable — plain text beats nothing
      if (g === this.gen) el.textContent = doc;
      return;
    }
    if (g !== this.gen || el !== this.host()?.nativeElement) return;
    el.textContent = "";
    try {
      const { EditorState, Compartment } = cm.state;
      const { EditorView, lineNumbers } = cm.view;
      const langComp = new Compartment();
      const view = new EditorView({
        state: EditorState.create({
          doc,
          extensions: [
            lineNumbers(),
            buildTheme(cm, theme),
            EditorView.editable.of(false),
            EditorState.readOnly.of(true),
            EditorView.lineWrapping,
            langComp.of([]),
          ],
        }),
        parent: el,
      });
      this.view = view;
      const lang = langId(this.path());
      if (lang) {
        void loadLangExt(lang).then((ext) => {
          if (g !== this.gen || this.view !== view) return; // stale
          view.dispatch({ effects: langComp.reconfigure(ext) });
        });
      }
    } catch {
      el.textContent = doc;
    }
  }
}
