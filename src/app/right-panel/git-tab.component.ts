import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal } from "@angular/core";
import { Agent, AgentFile, Commit, Project } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentWorkStore } from "../agents/agent-work.store";
import { ConflictStore } from "../agents/conflict.store";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { GitActionButtonComponent } from "../shared/git/git-action-button.component";
import { EstimateInput } from "../cost/estimate.service";
import { fileDir, fileName } from "../utils";
import { CommitFeedComponent } from "./commit-feed.component";
import { AgentCommitHistoryComponent } from "./agent-commit-history.component";
import { UiStore } from "../ui/ui.store";

@Component({
  selector: "app-git-tab",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CommitFeedComponent, AgentCommitHistoryComponent, GitActionButtonComponent],
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
            <app-icon name="branch" size="sm" color="var(--ink-3)" />
            <span [title]="ag.branch" style="font-size:var(--fs-sm);color:var(--ink);min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.branch }}</span>
          </div>
          <div class="tnum" style="font-size:var(--fs-xs);color:var(--ink-4);display:flex;gap:var(--sp-4)">
            <span>base {{ ag.base }}</span><span>·</span>
            <span>{{ ag.commits }} ahead</span><span>·</span>
            <span style="color:var(--code-add-ink)">+{{ totAdd() }}</span>
            <span style="color:var(--code-del-ink)">−{{ totDel() }}</span>
          </div>
        </div>

        <!-- in-progress merge conflicts (design agent-git.jsx AgentConflictCard):
             the session opened by a conflicted native merge — jump back into the
             3-way resolve view from here -->
        @if (conflictSession(); as sess) {
          <div style="margin:var(--sp-5) var(--sp-6);border:1px solid color-mix(in oklch, var(--st-blocked), transparent 55%);border-radius:var(--r-md);overflow:hidden;background:color-mix(in oklch, var(--st-blocked), transparent 92%)">
            <div style="padding:var(--sp-5)">
              <div style="display:flex;align-items:center;gap:var(--sp-3);margin-bottom:var(--sp-3)">
                <app-icon name="merge" size="sm" color="var(--st-blocked)" />
                <span style="font-size:var(--fs-sm);font-weight:600;color:var(--ink)">Merge conflicts</span>
                <span class="chip tnum" style="margin-left:auto;font-size:var(--fs-3xs);padding:0 var(--sp-3);color:var(--st-blocked);border-color:color-mix(in oklch, var(--st-blocked), transparent 55%)">{{ sess.files.length }} files</span>
              </div>
              <div style="font-size:var(--fs-xs);color:var(--ink-3);line-height:1.5;margin-bottom:var(--sp-4);text-wrap:pretty">
                Merge {{ sess.theirs }} → {{ sess.ours }} stopped —
                <b style="color:var(--st-blocked)">{{ unresolvedCount() }} unresolved file{{ unresolvedCount() === 1 ? "" : "s" }}</b>
                need resolving before this branch can land.
              </div>
              <button class="btn ghost-hair" (click)="openConflicts(ag.id)"
                style="width:100%;justify-content:center;border-color:color-mix(in oklch, var(--st-blocked), transparent 65%);color:var(--st-blocked)">
                <app-icon name="diff" size="sm" />Resolve in the Diff tab →
              </button>
            </div>
          </div>
        }

        <!-- changed files (selectable) -->
        <div style="padding:var(--sp-5) var(--sp-6) var(--sp-3);display:flex;align-items:center;gap:var(--sp-3)">
          @if (changes().length) {
            <button (click)="toggleAll()" [title]="allSelected() ? 'Deselect all' : 'Select all'"
              [style.border]="'1px solid ' + (allSelected() ? 'var(--ui-focus)' : 'var(--hair-2)')"
              [style.background]="allSelected() ? 'var(--ui-fill)' : 'transparent'"
              style="flex:none;width:var(--sp-6);height:var(--sp-6);border-radius:4px;display:grid;place-items:center;cursor:pointer;padding:0">
              @if (allSelected()) { <app-icon name="check" size="sm" [px]="10" color="var(--ui-on-fill)" /> }
            </button>
          }
          <span class="up" style="font-size:var(--fs-2xs);color:var(--ink-3)">Changes</span>
          <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ink-4)">{{ changes().length }}</span>
          @if (selected().size) { <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ui-ink)">{{ selected().size }} selected</span> }
          @if (changesLoading()) { <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ink-4)">· scanning…</span> }
        </div>

        <!-- capped + scrollable: however long the list, the commit input and
             the git action buttons below stay on screen -->
        <div class="scroll-y gt-changes">
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
                [style.border]="'1px solid ' + (isSelected(f.path) ? 'var(--ui-focus)' : 'var(--hair-2)')"
                [style.background]="isSelected(f.path) ? 'var(--ui-fill)' : 'transparent'"
                style="flex:none;width:var(--sp-6);height:var(--sp-6);border-radius:3px;display:grid;place-items:center"
              >@if (isSelected(f.path)) { <app-icon name="check" size="sm" [px]="9" color="var(--ui-on-fill)" /> }</span>
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
        </div>

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
          <!-- Commit: dual-path split button — primary = native backend commit,
               dropdown = AI variant with its estimate ON the row (A4.1) -->
          <app-git-action-button
            [label]="'Commit ' + (selected().size ? selected().size + ' selected' : 'all')"
            icon="commit"
            [disabled]="changes().length === 0"
            [estimateInput]="commitEstimate()"
            [variants]="[{ id: 'commit', label: 'Commit with AI (write the message)', icon: 'sparkles' }]"
            (native)="commit(ag.id)"
            (ai)="agentActions.aiAction(ag.id, 'commit')"
          />

          <!-- Push: native default; AI variant kept (existing PTY path) -->
          <app-git-action-button
            label="Push to origin"
            icon="push"
            [disabled]="ag.commits === 0"
            [estimateInput]="pushEstimate()"
            [variants]="[{ id: 'push', label: 'Push with AI (agent runs it)', icon: 'sparkles' }]"
            (native)="agentActions.pushAgent(ag.id)"
            (ai)="agentActions.aiAction(ag.id, 'push')"
          />

          <!-- Rebase: no native path yet (A3.4) → AI-only control, estimate
               shown inline on the button rather than hover-only -->
          <app-git-action-button
            [label]="'Rebase onto ' + baseBranch()"
            icon="sparkles"
            [aiOnly]="true"
            [estimateInput]="rebaseEstimate()"
            [variants]="[
              { id: 'rebase', label: 'Rebase with AI', icon: 'sparkles' },
              { id: 'rebase-v', label: 'Rebase with AI (verbose)', verbose: true, icon: 'sparkles' },
            ]"
            (ai)="agentActions.aiAction(ag.id, 'rebase')"
          />

          <!-- Merge: primary = NATIVE merge (A3.5 — conflicts open the 3-way
               view); dropdown = the agent drives the merge in its PTY -->
          <app-git-action-button
            [label]="'Merge ' + baseBranch() + ' → ' + ag.branch.replace('agent/', '')"
            icon="branch"
            kind="primary"
            [title]="'Merge ' + baseBranch() + ' into ' + ag.branch + ' · native'"
            [estimateInput]="mergeEstimate()"
            [variants]="[{ id: 'merge', label: 'Merge with AI (agent resolves conflicts)', icon: 'sparkles' }]"
            (native)="agentActions.mergeAgent(ag.id, baseBranch())"
            (ai)="agentActions.aiAction(ag.id, 'merge')"
          />

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
  styles: [
    `
      /* ~8 file rows, then scroll — token-derived so density scales it */
      .gt-changes {
        max-height: calc(var(--sp-9) * 8);
        overflow-y: auto;
        flex: none;
      }
    `,
  ],
})
export class GitTabComponent {
  readonly projects = inject(ProjectActionsService);
  readonly agentActions = inject(AgentActionsService);
  readonly work = inject(AgentWorkStore);
  readonly conflicts = inject(ConflictStore);
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

  // ---- in-progress merge conflict session (A3.5 / B4.2) ----
  readonly conflictSession = computed(() => {
    const ag = this.agent();
    const sess = ag ? this.conflicts.sessionFor(ag.id).data : null;
    return sess && sess.files.length ? sess : null;
  });
  readonly unresolvedCount = computed(
    () => this.conflictSession()?.files.filter((f) => !f.resolved).length ?? 0,
  );
  openConflicts(id: string): void {
    this.ui.setGitView(id, { kind: "conflict" });
  }

  // ---- AI cost estimate inputs (A4.3) ----
  readonly baseBranch = computed(() => this.project()?.branch ?? "main");
  // Why *40: the backend status carries line counts, not bytes — 40 bytes is a
  // conservative average source-line length, good enough for a cost ENVELOPE.
  private readonly diffBytes = computed(() => (this.totAdd() + this.totDel()) * 40);
  readonly commitEstimate = computed<EstimateInput>(() => ({
    op: "commit",
    files: this.changes().length,
    diffBytes: this.diffBytes(),
    model: this.agent()?.model,
  }));
  readonly pushEstimate = computed<EstimateInput>(() => ({
    op: "push",
    model: this.agent()?.model,
  }));
  readonly rebaseEstimate = computed<EstimateInput>(() => ({
    op: "rebase",
    files: this.changes().length,
    diffBytes: this.diffBytes(),
    model: this.agent()?.model,
  }));
  readonly mergeEstimate = computed<EstimateInput>(() => ({
    op: "merge",
    files: this.changes().length,
    diffBytes: this.diffBytes(),
    model: this.agent()?.model,
  }));
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
    return state === "A" ? "var(--vcs-added)" : state === "D" ? "var(--vcs-deleted)" : "var(--vcs-modified)";
  }

  // ---- AgentCommitHistory output handlers (no-op: wired to diff tab later) ----
  readonly ui = inject(UiStore);

  onOpenCommit(event: { sha: string; path?: string }): void {
    const ag = this.agent();
    if (ag) this.ui.setGitView(ag.id, { kind: "commit", sha: event.sha, path: event.path });
  }
  onOpenRange(shas: string[]): void {
    const ag = this.agent();
    if (ag) this.ui.setGitView(ag.id, { kind: "range", shas });
  }
  onOpenFileHistory(path: string): void {
    const ag = this.agent();
    if (ag) this.ui.setGitView(ag.id, { kind: "filehistory", path });
  }
}
