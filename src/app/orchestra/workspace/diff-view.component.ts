import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal } from "@angular/core";
import { Agent, AgentFile, FileDiff } from "../models";
import { IconComponent } from "../shared/icon.component";
import { AgentsStore } from "../stores/agents.store";
import { fileDir, fileName, mix } from "../utils";
import { CodeDiffComponent } from "./code-diff.component";

@Component({
  selector: "app-diff-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CodeDiffComponent],
  template: `
    <div style="flex:1;display:grid;grid-template-columns:236px 1fr;min-height:0">
      <!-- file list -->
      <div class="scroll-y" style="border-right:1px solid var(--hair);background:var(--panel);padding:6px 0">
        <div class="up" style="font-size:9px;color:var(--ink-3);padding:6px 14px 4px">Changed files · {{ changes().length }}</div>
        @for (f of changes(); track f.path; let i = $index) {
          <div
            class="diff-file"
            [class.sel]="sel() === i"
            (click)="sel.set(i)"
            style="display:flex;align-items:center;gap:8px;padding:6px 12px;cursor:pointer;margin:1px 6px;border-radius:var(--r-sm)"
          >
            <span
              [style.color]="stateInk(f.state)"
              [style.background]="stateBg(f.state)"
              style="flex:none;width:14px;height:14px;border-radius:3px;display:grid;place-items:center;font-size:9px;font-weight:700"
            >{{ f.state }}</span>
            <div style="flex:1;min-width:0">
              <div style="font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ fname(f.path) }}</div>
              @if (fdir(f.path)) {
                <div style="font-size:9.5px;color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ fdir(f.path) }}</div>
              }
            </div>
            <span class="tnum" style="font-size:9.5px;display:flex;gap:4px;flex:none">
              <span style="color:var(--code-add-ink)">+{{ f.add }}</span>
              @if (f.del > 0) { <span style="color:var(--code-del-ink)">−{{ f.del }}</span> }
            </span>
          </div>
        } @empty {
          <div style="padding:10px 14px;font-size:10.5px;color:var(--ink-4)">no changes</div>
        }
      </div>

      <!-- diff body -->
      <div style="display:flex;flex-direction:column;min-height:0;background:var(--bg)">
        <div style="display:flex;align-items:center;gap:8px;padding:8px 14px;background:var(--panel);border-bottom:1px solid var(--hair);font-size:11.5px">
          <app-icon name="file" size="sm" color="var(--ink-3)" />
          <span>{{ current()?.path ?? '—' }}</span>
          @if (loading()) { <span class="chip tnum" style="margin-left:auto;font-size:9.5px">loading…</span> }
        </div>
        @if (current() && diff(); as d) {
          <app-code-diff style="flex:1;min-height:0" [oldText]="d.old" [newText]="d.new" [lang]="d.lang" />
        } @else if (!current()) {
          <div style="flex:1;display:grid;place-items:center;color:var(--ink-4);font-size:12px">no changed files</div>
        } @else if (!loading()) {
          <div style="flex:1;display:grid;place-items:center;color:var(--ink-4);font-size:12px">no diff</div>
        }
      </div>
    </div>
  `,
  styles: [
    `
      .diff-file:hover:not(.sel) {
        background: var(--panel-2);
      }
      .diff-file.sel {
        background: var(--panel-3);
      }
    `,
  ],
})
export class DiffViewComponent {
  private agents = inject(AgentsStore);
  readonly agent = input.required<Agent>();
  readonly sel = signal(0);

  readonly fname = fileName;
  readonly fdir = fileDir;

  readonly changes = computed(() => this.agent().git_changes?.files ?? []);
  readonly current = computed<AgentFile | undefined>(() => this.changes()[this.sel()]);
  readonly diff = signal<FileDiff | null>(null);
  readonly loading = signal(false);
  private gen = 0;
  private lastId: string | null = null;

  constructor() {
    // reset selection to the first file when switching agents
    effect(() => {
      const id = this.agent().id;
      if (id !== this.lastId) {
        this.lastId = id;
        this.sel.set(0);
      }
    });
    // load the diff for the selected file (superseded on rapid changes)
    effect(() => {
      const ag = this.agent();
      const f = this.current();
      if (!f) {
        this.diff.set(null);
        return;
      }
      const g = ++this.gen;
      this.loading.set(true);
      void this.agents
        .diff(ag.id, f.path)
        .then((d) => {
          if (this.gen === g) {
            this.diff.set(d);
            this.loading.set(false);
          }
        })
        .catch(() => {
          if (this.gen === g) {
            this.diff.set(null);
            this.loading.set(false);
          }
        });
    });
  }

  stateInk(state: string): string {
    return state === "A" ? "var(--code-add-ink)" : state === "D" ? "var(--code-del-ink)" : "var(--accent-2)";
  }
  stateBg(state: string): string {
    return state === "A" ? "var(--code-add-bg)" : state === "D" ? "var(--code-del-bg)" : mix("var(--accent-2)", 86);
  }
}
