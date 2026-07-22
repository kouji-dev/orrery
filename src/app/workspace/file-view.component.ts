import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
import { marked } from "marked";
import { Agent, BlameLine } from "../models";
import { AgentsStore } from "../stores/agents.store";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName, langId, langTag } from "../utils";
import { BRIDGE, Commands } from "../data-source/bridge";
import { UnifiedCodeComponent } from "./review/unified-code.component";
import { AnnotateBlameComponent } from "./review/annotate-blame.component";
import { SendReviewButtonComponent } from "./review/send-review.component";

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
  imports: [IconComponent, UnifiedCodeComponent, AnnotateBlameComponent, SendReviewButtonComponent],
  template: `
    <!-- slim toolbar: path · changed-state · (md toggle) · annotate · lang · refresh -->
    <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-6);background:var(--panel);border-bottom:1px solid var(--hair);font-size:var(--fs-sm);flex:none;min-width:0">
      <app-icon name="file" size="sm" [px]="12" color="var(--ink-3)" />
      <span style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [title]="path()">
        <span style="color:var(--ink-4)">{{ fdir(path()) }}</span>{{ fname(path()) }}
      </span>
      <div style="margin-left:auto;display:flex;align-items:center;gap:var(--sp-3);flex:none">
        @if (isMarkdown()) {
          <div style="display:flex;gap:var(--sp-1);padding:var(--sp-1);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm)">
            <button class="btn" (click)="preview.set(false)"
              [style.background]="!preview() ? 'var(--panel-3)' : 'transparent'"
              [style.color]="!preview() ? 'var(--ink)' : 'var(--ink-3)'"
              style="padding:var(--sp-1) var(--sp-3);border-radius:4px;font-size:var(--fs-xs)">Raw</button>
            <button class="btn" (click)="preview.set(true)"
              [style.background]="preview() ? 'var(--panel-3)' : 'transparent'"
              [style.color]="preview() ? 'var(--ink)' : 'var(--ink-3)'"
              style="padding:var(--sp-1) var(--sp-3);border-radius:4px;font-size:var(--fs-xs)">Preview</button>
          </div>
        }
        <button
          class="btn"
          [class.ghost-hair]="!annotate()"
          (click)="annotate.set(!annotate())"
          title="Annotate — show who last changed each line"
          [style.color]="annotate() ? 'var(--ink)' : 'var(--ink-3)'"
          [style.background]="annotate() ? 'color-mix(in oklch, var(--accent), transparent 86%)' : 'transparent'"
          [style.border]="'1px solid ' + (annotate() ? 'color-mix(in oklch, var(--accent), transparent 60%)' : 'var(--hair)')"
          style="padding:var(--sp-1) var(--sp-4);gap:var(--sp-2);border-radius:var(--r-sm);font-size:var(--fs-xs)"
        >
          <app-icon name="git" size="sm" [px]="12" [color]="annotate() ? 'var(--accent)' : null" />
          Annotate
        </button>
        @if (tag()) { <span class="chip tnum" style="font-size:var(--fs-2xs);padding:0 var(--sp-3)">{{ tag() }}</span> }
        <button class="btn" (click)="reload()" title="Reload from the worktree" style="padding:var(--sp-1);border-radius:4px">
          <app-icon name="refresh" size="sm" [px]="12" [class.set-spin]="loading()" />
        </button>
        <app-send-review-button [agent]="agent().id" [agentName]="agent().name" />
      </div>
    </div>

    <!-- body -->
    @if (notice(); as n) {
      <div style="flex:1;display:grid;place-items:center;color:var(--ink-4);font-size:var(--fs-sm);padding:var(--sp-7);text-align:center">{{ n }}</div>
    } @else if (isMarkdown() && preview()) {
      <div class="scroll-y md-body" style="flex:1;padding:var(--sp-7) var(--sp-8)" [innerHTML]="mdHtml()"></div>
    } @else if (annotate()) {
      <app-annotate-blame [lines]="blame()" (openCommit)="onOpenCommit($event)" />
    } @else {
      <app-unified-code [agent]="agent().id" [file]="path()" view="file" [newText]="content() ?? ''" [lang]="lid()" />
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
      .md-body { font-size: var(--fs-ui); line-height: 1.7; color: var(--ink-2); }
      .md-body ::ng-deep h1, .md-body ::ng-deep h2, .md-body ::ng-deep h3 { color: var(--ink); margin: var(--sp-6) 0 var(--sp-3); }
      .md-body ::ng-deep code { background: var(--panel-2); padding: 1px var(--sp-2); border-radius: 4px; font-size: var(--fs-sm); }
      .md-body ::ng-deep pre { background: var(--panel-2); padding: var(--sp-5) var(--sp-6); border-radius: 8px; overflow-x: auto; }
      .md-body ::ng-deep a { color: var(--accent-2); }
    `,
  ],
})
export class FileViewComponent {
  readonly agent = input.required<Agent>();
  readonly path = input.required<string>();

  private agents = inject(AgentsStore);
  private ui = inject(UiStore);
  private bridge = inject(BRIDGE);

  readonly loading = signal(false);
  readonly preview = signal(true); // markdown opens rendered; Raw is one click
  readonly annotate = signal(false);
  readonly content = signal<string | null>(null);
  private readonly error = signal<string | null>(null);
  readonly blame = signal<BlameLine[]>([]);

  readonly fdir = fileDir;
  readonly fname = fileName;
  readonly isMarkdown = computed(() => /\.(md|markdown)$/i.test(this.path()));
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
      void this.load(id, path);
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
        .invoke<{ old: BlameLine[]; new: BlameLine[] }>(Commands.AgentWorkingBlame, { id, path })
        .then((r) => {
          if (this.blameGen !== g) return;
          this.blame.set(r.new ?? []);
        })
        .catch(() => {
          if (this.blameGen !== g) return;
          this.blame.set([]);
        });
    });
  }

  reload() {
    void this.load(this.agent().id, this.path());
  }

  onOpenCommit(sha: string) {
    this.ui.setGitView(this.agent().id, { kind: "commit", sha });
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
}
