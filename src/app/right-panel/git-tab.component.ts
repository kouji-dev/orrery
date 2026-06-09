import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal } from "@angular/core";
import { Agent, AgentFile, Commit, Project } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentWorkStore } from "../agents/agent-work.store";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { fileDir, fileName } from "../utils";
import { CommitFeedComponent } from "./commit-feed.component";

@Component({
  selector: "app-git-tab",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CommitFeedComponent],
  template: `
    @if (!agent()) {
      <div class="scroll-y" style="flex:1;padding:8px 0">
        <div class="up" style="font-size:9px;color:var(--ink-3);padding:6px 14px">Commit feed · all worktrees</div>
        <app-commit-feed [commits]="projects.commits()" />
      </div>
    } @else {
      @let ag = agent()!;
      <div class="scroll-y" style="flex:1">
        <!-- branch header -->
        <div style="padding:10px 14px;border-bottom:1px solid var(--hair)">
          <div style="display:flex;align-items:center;gap:7px;margin-bottom:4px;min-width:0">
            <app-icon name="branch" size="sm" color="var(--accent-2)" />
            <span [title]="ag.branch" style="font-size:11.5px;color:var(--ink);min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.branch }}</span>
          </div>
          <div class="tnum" style="font-size:10px;color:var(--ink-4);display:flex;gap:8px">
            <span>base {{ ag.base }}</span><span>·</span>
            <span>{{ ag.commits }} ahead</span><span>·</span>
            <span style="color:var(--code-add-ink)">+{{ totAdd() }}</span>
            <span style="color:var(--code-del-ink)">−{{ totDel() }}</span>
          </div>
        </div>

        <!-- changed files (selectable) -->
        <div style="padding:10px 14px 6px;display:flex;align-items:center;gap:7px">
          @if (changes().length) {
            <button (click)="toggleAll()" [title]="allSelected() ? 'Deselect all' : 'Select all'"
              [style.border]="'1px solid ' + (allSelected() ? 'var(--accent)' : 'var(--hair-2)')"
              [style.background]="allSelected() ? 'var(--accent)' : 'transparent'"
              style="flex:none;width:14px;height:14px;border-radius:4px;display:grid;place-items:center;cursor:pointer;padding:0">
              @if (allSelected()) { <app-icon name="check" size="sm" [px]="10" color="#06070b" /> }
            </button>
          }
          <span class="up" style="font-size:9px;color:var(--ink-3)">Changes</span>
          <span class="tnum" style="font-size:9px;color:var(--ink-4)">{{ changes().length }}</span>
          @if (selected().size) { <span class="tnum" style="font-size:9px;color:var(--accent)">{{ selected().size }} selected</span> }
          @if (changesLoading()) { <span class="tnum" style="font-size:9px;color:var(--ink-4)">· scanning…</span> }
        </div>

        @if (changesLoading()) {
          <div style="padding:4px 14px 8px;font-size:10.5px;color:var(--ink-4)">scanning worktree…</div>
        } @else if (changes().length) {
          @for (f of changes(); track f.path) {
            <div
              (click)="toggle(f.path)"
              [style.background]="isSelected(f.path) ? 'var(--panel-2)' : 'transparent'"
              style="display:flex;align-items:center;gap:8px;padding:4px 14px;font-size:11px;cursor:pointer"
            >
              <span
                [style.border]="'1px solid ' + (isSelected(f.path) ? 'var(--accent)' : 'var(--hair-2)')"
                [style.background]="isSelected(f.path) ? 'var(--accent)' : 'transparent'"
                style="flex:none;width:13px;height:13px;border-radius:3px;display:grid;place-items:center"
              >@if (isSelected(f.path)) { <app-icon name="check" size="sm" [px]="9" color="#06070b" /> }</span>
              <span [style.color]="stateInk(f.state)" style="flex:none;width:12px;text-align:center;font-size:9px;font-weight:700">{{ f.state }}</span>
              <span [title]="f.path" style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
                <span style="color:var(--ink-4)">{{ fdir(f.path) }}</span>{{ fname(f.path) }}
              </span>
              <span class="tnum" style="flex:none;font-size:9.5px;display:flex;gap:4px">
                <span style="color:var(--code-add-ink)">+{{ f.add }}</span>
                @if (f.del > 0) { <span style="color:var(--code-del-ink)">−{{ f.del }}</span> }
              </span>
            </div>
          }
        } @else {
          <div style="padding:4px 14px 8px;font-size:10.5px;color:var(--ink-4)">clean — no working changes</div>
        }

        <!-- actions -->
        <div style="padding:12px;display:grid;gap:7px">
          @if (changes().length) {
            <input
              [value]="commitMsg()"
              (input)="commitMsg.set($any($event.target).value)"
              placeholder="commit message…"
              style="background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:8px 10px;color:var(--ink);font-family:var(--font-mono);font-size:11.5px;outline:none"
            />
          }
          <!-- Commit: primary = backend, attached ✨ = AI -->
          <div style="display:flex;gap:4px">
            <button class="btn ghost-hair" [disabled]="changes().length === 0" (click)="commit(ag.id)" style="flex:1;min-width:0;justify-content:flex-start">
              <app-icon name="commit" size="sm" />Commit {{ selected().size ? selected().size + ' selected' : 'all' }}
            </button>
            <button class="btn ghost-hair" [disabled]="changes().length === 0" title="Let the agent commit" (click)="agentActions.aiAction(ag.id, 'commit')" style="flex:none;padding:0 9px">
              <app-icon name="sparkles" size="sm" color="var(--accent)" />
            </button>
          </div>

          <!-- Push: primary = backend, attached ✨ = AI -->
          <div style="display:flex;gap:4px">
            <button class="btn ghost-hair" [disabled]="ag.commits === 0" (click)="agentActions.pushAgent(ag.id)" style="flex:1;min-width:0;justify-content:flex-start">
              <app-icon name="push" size="sm" />Push to origin
            </button>
            <button class="btn ghost-hair" [disabled]="ag.commits === 0" title="Let the agent push" (click)="agentActions.aiAction(ag.id, 'push')" style="flex:none;padding:0 9px">
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

        <!-- this branch's commits (lazy first page + Load more) -->
        <div class="up" style="font-size:9px;color:var(--ink-3);padding:4px 14px">Commits on this branch</div>
        <app-commit-feed [commits]="agentCommits()" [compact]="true" />
        @if (commitsEntry()?.status === 'loading' && !agentCommits().length) {
          <div style="padding:2px 14px 14px;font-size:10.5px;color:var(--ink-4)">loading commits…</div>
        } @else if (!agentCommits().length) {
          <div style="padding:2px 14px 14px;font-size:10.5px;color:var(--ink-4)">no commits yet</div>
        }
        @if (commitsEntry()?.hasMore) {
          <button class="btn ghost-hair" style="margin:4px 14px 14px;justify-content:center"
            [disabled]="commitsEntry()?.status === 'loading'" (click)="work.loadMoreCommits(ag.id)">
            {{ commitsEntry()?.status === 'loading' ? 'loading…' : 'Load more' }}
          </button>
        }
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
}
