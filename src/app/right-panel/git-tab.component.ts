import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal } from "@angular/core";
import { Agent, AgentFile, Commit, Project } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentWorkStore } from "../agents/agent-work.store";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { fileDir, fileName } from "../utils";
import { CommitFeedComponent } from "./commit-feed.component";
import { AgentCommitHistoryComponent } from "./agent-commit-history.component";

@Component({
  selector: "app-git-tab",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CommitFeedComponent, AgentCommitHistoryComponent],
  template: `
    @if (!agent()) {
      <div class="scroll-y" style="flex:1;padding:var(--sp-4) 0">
        <div class="up" style="font-size:var(--fs-2xs);color:var(--ink-3);padding:var(--sp-3) var(--sp-6)">Commit feed · all worktrees</div>
        <app-commit-feed [commits]="projects.commits()" />
      </div>
    } @else {
      @let ag = agent()!;
      <div class="scroll-y" style="flex:1">
        <!-- branch header -->
        <div style="padding:var(--sp-5) var(--sp-6);border-bottom:1px solid var(--hair)">
          <div style="display:flex;align-items:center;gap:var(--sp-3);margin-bottom:var(--sp-2);min-width:0">
            <app-icon name="branch" size="sm" color="var(--accent-2)" />
            <span [title]="ag.branch" style="font-size:var(--fs-sm);color:var(--ink);min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.branch }}</span>
          </div>
          <div class="tnum" style="font-size:var(--fs-xs);color:var(--ink-4);display:flex;gap:var(--sp-4)">
            <span>base {{ ag.base }}</span><span>·</span>
            <span>{{ ag.commits }} ahead</span><span>·</span>
            <span style="color:var(--code-add-ink)">+{{ totAdd() }}</span>
            <span style="color:var(--code-del-ink)">−{{ totDel() }}</span>
          </div>
        </div>

        <!-- changed files (selectable) -->
        <div style="padding:var(--sp-5) var(--sp-6) var(--sp-3);display:flex;align-items:center;gap:var(--sp-3)">
          @if (changes().length) {
            <button (click)="toggleAll()" [title]="allSelected() ? 'Deselect all' : 'Select all'"
              [style.border]="'1px solid ' + (allSelected() ? 'var(--accent)' : 'var(--hair-2)')"
              [style.background]="allSelected() ? 'var(--accent)' : 'transparent'"
              style="flex:none;width:var(--sp-6);height:var(--sp-6);border-radius:4px;display:grid;place-items:center;cursor:pointer;padding:0">
              @if (allSelected()) { <app-icon name="check" size="sm" [px]="10" color="#06070b" /> }
            </button>
          }
          <span class="up" style="font-size:var(--fs-2xs);color:var(--ink-3)">Changes</span>
          <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ink-4)">{{ changes().length }}</span>
          @if (selected().size) { <span class="tnum" style="font-size:var(--fs-2xs);color:var(--accent)">{{ selected().size }} selected</span> }
          @if (changesLoading()) { <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ink-4)">· scanning…</span> }
        </div>

        @if (changesLoading()) {
          <div style="padding:var(--sp-2) var(--sp-6) var(--sp-4);font-size:var(--fs-xs);color:var(--ink-4)">scanning worktree…</div>
        } @else if (changes().length) {
          @for (f of changes(); track f.path) {
            <div
              (click)="toggle(f.path)"
              [style.background]="isSelected(f.path) ? 'var(--panel-2)' : 'transparent'"
              style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-2) var(--sp-6);font-size:var(--fs-sm);cursor:pointer"
            >
              <span
                [style.border]="'1px solid ' + (isSelected(f.path) ? 'var(--accent)' : 'var(--hair-2)')"
                [style.background]="isSelected(f.path) ? 'var(--accent)' : 'transparent'"
                style="flex:none;width:var(--sp-6);height:var(--sp-6);border-radius:3px;display:grid;place-items:center"
              >@if (isSelected(f.path)) { <app-icon name="check" size="sm" [px]="9" color="#06070b" /> }</span>
              <span [style.color]="stateInk(f.state)" style="flex:none;width:12px;text-align:center;font-size:var(--fs-2xs);font-weight:700">{{ f.state }}</span>
              <span [title]="f.path" style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
                <span style="color:var(--ink-4)">{{ fdir(f.path) }}</span>{{ fname(f.path) }}
              </span>
              <span class="tnum" style="flex:none;font-size:var(--fs-2xs);display:flex;gap:var(--sp-2)">
                <span style="color:var(--code-add-ink)">+{{ f.add }}</span>
                @if (f.del > 0) { <span style="color:var(--code-del-ink)">−{{ f.del }}</span> }
              </span>
            </div>
          }
        } @else {
          <div style="padding:var(--sp-2) var(--sp-6) var(--sp-4);font-size:var(--fs-xs);color:var(--ink-4)">clean — no working changes</div>
        }

        <!-- actions -->
        <div style="padding:var(--sp-6);display:grid;gap:var(--sp-3)">
          @if (changes().length) {
            <input
              [value]="commitMsg()"
              (input)="commitMsg.set($any($event.target).value)"
              placeholder="commit message…"
              style="background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:var(--sp-4) var(--sp-5);color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-sm);outline:none"
            />
          }
          <!-- Commit: primary = backend, attached ✨ = AI -->
          <div style="display:flex;gap:var(--sp-2)">
            <button class="btn ghost-hair" [disabled]="changes().length === 0" (click)="commit(ag.id)" style="flex:1;min-width:0;justify-content:flex-start">
              <app-icon name="commit" size="sm" />Commit {{ selected().size ? selected().size + ' selected' : 'all' }}
            </button>
            <button class="btn ghost-hair" [disabled]="changes().length === 0" title="Let the agent commit" (click)="agentActions.aiAction(ag.id, 'commit')" style="flex:none;padding:0 var(--sp-4)">
              <app-icon name="sparkles" size="sm" color="var(--accent)" />
            </button>
          </div>

          <!-- Push: primary = backend, attached ✨ = AI -->
          <div style="display:flex;gap:var(--sp-2)">
            <button class="btn ghost-hair" [disabled]="ag.commits === 0" (click)="agentActions.pushAgent(ag.id)" style="flex:1;min-width:0;justify-content:flex-start">
              <app-icon name="push" size="sm" />Push to origin
            </button>
            <button class="btn ghost-hair" [disabled]="ag.commits === 0" title="Let the agent push" (click)="agentActions.aiAction(ag.id, 'push')" style="flex:none;padding:0 var(--sp-4)">
              <app-icon name="sparkles" size="sm" color="var(--accent)" />
            </button>
          </div>

          <!-- Rebase (AI) -->
          <button class="btn ghost-hair" title="Let the agent rebase (resolves conflicts)" (click)="agentActions.aiAction(ag.id, 'rebase')" style="justify-content:flex-start;min-width:0">
            <app-icon name="sparkles" size="sm" color="var(--accent)" />
            <span style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">Rebase onto {{ project() ? project()!.branch : 'main' }}</span>
          </button>

          <!-- Merge (AI): base → branch -->
          <button class="btn primary" title="Let the agent merge (resolves conflicts)" (click)="agentActions.aiAction(ag.id, 'merge')" style="justify-content:center;min-width:0">
            <app-icon name="sparkles" size="sm" />
            <span
              [title]="'Merge ' + (project() ? project()!.branch : 'main') + ' → ' + ag.branch.replace('agent/', '')"
              style="min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
            >Merge {{ project() ? project()!.branch : 'main' }} → {{ ag.branch.replace('agent/', '') }}</span>
          </button>

          <button class="btn ghost-hair" [disabled]="changes().length === 0" (click)="discard(ag.id)" style="justify-content:flex-start;color:var(--st-blocked)">
            <app-icon name="discard" size="sm" />Discard {{ selected().size ? selected().size + ' selected' : 'all' }}
          </button>
        </div>

        <!-- this branch's commits — expandable per-commit view -->
        <app-agent-commit-history
          [agent]="ag"
          (openCommit)="onOpenCommit($event)"
          (openRange)="onOpenRange($event)"
          (openFileHistory)="onOpenFileHistory($event)"
        />
      </div>
    }
  `,
})
export class GitTabComponent {
  readonly projects = inject(ProjectActionsService);
  readonly agentActions = inject(AgentActionsService);
  readonly work = inject(AgentWorkStore);
  readonly agent = input<Agent | null>(null);
  readonly project = input<Project | undefined>(undefined);

  readonly fname = fileName;
  readonly fdir = fileDir;

  readonly changes = computed<AgentFile[]>(() => {
    const ag = this.agent();
    return ag ? this.work.changesFor(ag.id).data : [];
  });
  readonly changesLoading = computed(() => {
    const ag = this.agent();
    return ag ? this.work.changesFor(ag.id).status === "loading" : false;
  });
  readonly totAdd = computed(() => this.changes().reduce((s, f) => s + f.add, 0));
  readonly totDel = computed(() => this.changes().reduce((s, f) => s + f.del, 0));
  // this branch's commits, read lazily from the agent's worktree (first page on
  // agent open, paged onward via the Load more button).
  readonly commitsEntry = computed(() => {
    const ag = this.agent();
    return ag ? this.work.commitsFor(ag.id) : null;
  });
  readonly agentCommits = computed<Commit[]>(() => this.commitsEntry()?.data ?? []);

  readonly selected = signal<Set<string>>(new Set());
  readonly commitMsg = signal("");
  readonly allSelected = computed(() => {
    const ch = this.changes();
    return ch.length > 0 && ch.every((f) => this.selected().has(f.path));
  });

  private lastId: string | null = null;
  constructor() {
    // clear the selection when switching agents
    effect(() => {
      const id = this.agent()?.id ?? null;
      if (id !== this.lastId) {
        this.lastId = id;
        this.selected.set(new Set());
        this.commitMsg.set("");
      }
    });
  }

  isSelected(path: string): boolean {
    return this.selected().has(path);
  }
  toggle(path: string) {
    this.selected.update((s) => {
      const n = new Set(s);
      if (n.has(path)) n.delete(path);
      else n.add(path);
      return n;
    });
  }
  toggleAll() {
    this.selected.set(this.allSelected() ? new Set() : new Set(this.changes().map((f) => f.path)));
  }
  // the paths to act on: the selection, or [] (= all) when nothing is selected
  private targetPaths(): string[] {
    const sel = this.selected();
    return sel.size ? this.changes().filter((f) => sel.has(f.path)).map((f) => f.path) : [];
  }

  commit(id: string) {
    const msg = this.commitMsg().trim() || "wip: " + (this.agent()?.name ?? "");
    this.agentActions.commitAgent(id, this.targetPaths(), msg);
    this.commitMsg.set("");
    this.selected.set(new Set());
  }
  discard(id: string) {
    this.agentActions.discardAgent(id, this.targetPaths());
    this.selected.set(new Set());
  }

  stateInk(state: string): string {
    return state === "A" ? "var(--code-add-ink)" : state === "D" ? "var(--code-del-ink)" : "var(--accent-2)";
  }

  // ---- AgentCommitHistory output handlers (no-op: wired to diff tab later) ----
  onOpenCommit(_event: { sha: string; path?: string }): void { /* TODO: route to diff tab */ }
  onOpenRange(_shas: string[]): void { /* TODO: route to range-diff tab */ }
  onOpenFileHistory(_path: string): void { /* TODO: route to file-history tab */ }
}
