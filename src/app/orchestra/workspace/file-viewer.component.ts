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
import { Agent } from "../models";
import { IconComponent } from "../shared/icon.component";
import { AgentsStore } from "../stores/agents.store";
import { fileDir, fileName, langTag } from "../utils";
import { CodeViewComponent } from "./code-view.component";

/**
 * Read-only viewer for a single file in an agent's worktree — shown in the
 * closable "file" tab. Content is the working-tree text (loaded via the diff
 * command's `.new`), syntax-highlighted by CodeMirror. Markdown files get a
 * Raw / Preview toggle (preview rendered with `marked`, sanitized by Angular's
 * [innerHTML]).
 */
@Component({
  selector: "app-file-viewer",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CodeViewComponent],
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--bg)">
      <!-- header: dir/name · changed chip · [md raw/preview] · lang · line count -->
      <div style="display:flex;align-items:center;gap:8px;padding:8px 14px;background:var(--panel);border-bottom:1px solid var(--hair);font-size:11.5px;flex:none">
        <app-icon name="file" size="sm" [color]="changed() ? changedInk() : 'var(--ink-3)'" />
        <span style="color:var(--ink-4)">{{ fdir(path()) }}</span>
        <span style="margin-left:-6px">{{ fname(path()) }}</span>
        @if (changed(); as c) {
          <span class="chip tnum" [style.color]="changedInk()" style="font-size:9px;padding:1px 6px">
            {{ c.state === 'A' ? 'added' : c.state === 'D' ? 'deleted' : c.state === 'R' ? 'renamed' : 'modified' }} +{{ c.add }}{{ c.del ? ' −' + c.del : '' }}
          </span>
        }

        <div style="margin-left:auto;display:flex;align-items:center;gap:8px">
          @if (isMarkdown()) {
            <!-- raw / rendered-preview toggle (markdown only) -->
            <div style="display:flex;gap:2px;padding:2px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm)">
              <button class="btn" (click)="preview.set(false)" title="Source"
                [style.background]="!preview() ? 'var(--panel-3)' : 'transparent'"
                [style.color]="!preview() ? 'var(--ink)' : 'var(--ink-3)'"
                style="padding:3px 8px;border-radius:4px;font-size:10px">Raw</button>
              <button class="btn" (click)="preview.set(true)" title="Rendered preview"
                [style.background]="preview() ? 'var(--panel-3)' : 'transparent'"
                [style.color]="preview() ? 'var(--ink)' : 'var(--ink-3)'"
                style="padding:3px 8px;border-radius:4px;font-size:10px">Preview</button>
            </div>
          }
          @if (displayLang()) { <span class="chip tnum" style="font-size:9.5px">{{ displayLang() }}</span> }
          <span class="tnum" style="font-size:9.5px;color:var(--ink-4)">
            @if (loading()) { loading… } @else { {{ lineCount() }} lines }
          </span>
        </div>
      </div>

      <!-- body -->
      @if (loading()) {
        <div style="padding:14px;font-size:11px;color:var(--ink-4)">loading…</div>
      } @else if (isMarkdown() && preview()) {
        <div class="scroll-y md-preview" style="flex:1;min-height:0" [innerHTML]="previewHtml()"></div>
      } @else {
        <app-code-view style="flex:1;min-height:0" [text]="content()" [lang]="rawLang()" />
      }
    </div>
  `,
  styles: [
    `
      .md-preview {
        padding: 18px 24px;
        font-size: 13.5px;
        line-height: 1.65;
        color: var(--ink-2);
        max-width: 860px;
      }
      .md-preview h1,
      .md-preview h2,
      .md-preview h3 {
        color: var(--ink);
        font-weight: 600;
        margin: 1.2em 0 0.5em;
        line-height: 1.3;
      }
      .md-preview h1 {
        font-size: 1.5em;
        border-bottom: 1px solid var(--hair);
        padding-bottom: 0.3em;
      }
      .md-preview h2 {
        font-size: 1.25em;
      }
      .md-preview h3 {
        font-size: 1.1em;
      }
      .md-preview p {
        margin: 0.6em 0;
      }
      .md-preview a {
        color: var(--accent-2);
      }
      .md-preview code {
        font-family: var(--font-mono);
        font-size: 0.88em;
        background: var(--panel-2);
        padding: 1px 5px;
        border-radius: 4px;
      }
      .md-preview pre {
        background: var(--panel-2);
        border: 1px solid var(--hair);
        border-radius: var(--r-md);
        padding: 11px 13px;
        overflow-x: auto;
      }
      .md-preview pre code {
        background: none;
        padding: 0;
      }
      .md-preview ul,
      .md-preview ol {
        padding-left: 1.4em;
        margin: 0.6em 0;
      }
      .md-preview blockquote {
        border-left: 3px solid var(--hair-2);
        margin: 0.6em 0;
        padding-left: 0.9em;
        color: var(--ink-3);
      }
      .md-preview table {
        border-collapse: collapse;
        margin: 0.6em 0;
      }
      .md-preview th,
      .md-preview td {
        border: 1px solid var(--hair);
        padding: 5px 9px;
      }
      .md-preview img {
        max-width: 100%;
      }
    `,
  ],
})
export class FileViewerComponent {
  private agents = inject(AgentsStore);
  readonly agent = input.required<Agent>();
  readonly path = input.required<string>();

  readonly fname = fileName;
  readonly fdir = fileDir;

  readonly content = signal<string>("");
  readonly rawLang = signal<string>(""); // a FileDiff.lang tag, for CodeMirror
  readonly loading = signal(false);
  readonly preview = signal(false); // markdown: false = raw source, true = rendered
  private gen = 0;

  readonly isMarkdown = computed(() => this.rawLang() === "markdown");
  readonly displayLang = computed(() => (this.content() ? langTag(this.path()) : ""));
  readonly lineCount = computed(() => {
    const t = this.content();
    return t ? t.replace(/\n$/, "").split("\n").length : 0;
  });
  readonly previewHtml = computed(() => marked.parse(this.content(), { async: false }) as string);

  // the changed-file entry for this path (drives the state chip), if any
  readonly changed = computed(() =>
    (this.agent().git_changes?.files ?? []).find((f) => f.path === this.path()),
  );
  changedInk(): string {
    const c = this.changed();
    return c?.state === "A"
      ? "var(--code-add-ink)"
      : c?.state === "D"
        ? "var(--code-del-ink)"
        : c?.state === "R"
          ? "var(--accent)"
          : "var(--accent-2)";
  }

  constructor() {
    // (re)load the file content whenever the agent or path changes; superseded on
    // rapid switches via a generation counter. Default back to raw source view.
    effect(() => {
      const ag = this.agent();
      const p = this.path();
      const g = ++this.gen;
      this.preview.set(false);
      this.loading.set(true);
      void this.agents
        .diff(ag.id, p)
        .then((d) => {
          if (this.gen !== g) return;
          this.content.set(d.new ?? "");
          this.rawLang.set(d.lang ?? "");
          this.loading.set(false);
        })
        .catch(() => {
          if (this.gen !== g) return;
          this.content.set("");
          this.rawLang.set("");
          this.loading.set(false);
        });
    });
  }
}
