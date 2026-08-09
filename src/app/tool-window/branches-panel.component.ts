import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
import { AgentActionsService } from "../agents/agent-actions.service";
import { BranchesStore } from "../agents/branches.store";
import { BranchInfo } from "../data-source/bridge";
import { Agent, Project } from "../models";
import { EstimateInput } from "../cost/estimate.service";
import { GitActionButtonComponent } from "../shared/git/git-action-button.component";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";

/**
 * Bottom-dock "Branches" panel — LIVE since A3.2: fetch/pull (CLI shell-out so
 * the OS credential helper auths), checkout into the scoped agent's worktree,
 * new branch / rename / delete / upstream via git2 with worktree-occupancy
 * pre-checks, "Merge in" through the existing native-merge conflict session.
 */
@Component({
  selector: "app-branches-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, GitActionButtonComponent],
  template: `
    @if (!project()) {
      <div style="padding:var(--sp-8);font-size:var(--fs-sm);color:var(--ink-4)">
        select a project to see its branches &amp; remotes
      </div>
    } @else {
      @let p = project()!;
      <div style="display:grid;grid-template-columns:300px 1fr;min-height:0;flex:1">
        <!-- remotes -->
        <div style="display:flex;flex-direction:column;min-height:0;border-right:1px solid var(--hair)">
          <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
            <span class="up" style="font-size:var(--fs-3xs);color:var(--ink-3)">Remotes</span>
          </div>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            @for (r of store.remotes(); track r.name) {
              <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair)">
                <app-icon name="globe" size="sm" color="var(--ink-4)" />
                <div style="flex:1;min-width:0">
                  <div style="font-size:var(--fs-sm);color:var(--ink-2)">{{ r.name }}</div>
                  <div style="font-size:var(--fs-2xs);color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [title]="r.url">{{ r.url }}</div>
                </div>
                <button class="btn br-op" [disabled]="store.busy()" (click)="store.fetch(p.id, r.name)" title="git fetch --prune {{ r.name }}">
                  <app-icon name="refresh" size="sm" [px]="11" />Fetch
                </button>
              </div>
            }
            @if (!store.remotes().length) {
              <div style="padding:var(--sp-6);font-size:var(--fs-sm);color:var(--ink-4)">no remotes configured</div>
            }
          </div>
        </div>

        <!-- branches -->
        <div style="display:flex;flex-direction:column;min-height:0">
          <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
            <span class="up" style="font-size:var(--fs-3xs);color:var(--ink-3)">Branches · {{ p.name }}</span>
            @if (store.busy()) {
              <span class="dot running" style="background:var(--st-running)"></span>
            }
            <div style="margin-left:auto;display:flex;gap:var(--sp-3);align-items:center">
              <app-git-action-button label="Fetch" icon="refresh" [small]="true" [disabled]="store.busy()" title="git fetch --all --prune" [estimateInput]="branchEstimate()" (native)="store.fetch(p.id)" />
              <app-git-action-button label="Pull" icon="stage" [small]="true" [disabled]="store.busy()" [title]="pullTitle()" [estimateInput]="branchEstimate()" (native)="pull(p.id)" />
              <app-git-action-button
                label="New branch"
                icon="plus"
                kind="primary"
                [small]="true"
                [disabled]="store.busy() || !newName().trim()"
                title="create the branch named below"
                [estimateInput]="branchEstimate()"
                (native)="create(p.id)"
              />
            </div>
          </div>
          <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair);flex:none;background:var(--panel-2)">
            <input
              [value]="newName()"
              (input)="newName.set($any($event.target).value)"
              (keydown.enter)="create(p.id)"
              placeholder="new branch name…"
              spellcheck="false"
              style="flex:1;background:var(--panel);border:1px solid var(--hair);border-radius:var(--r-sm);padding:var(--sp-2) var(--sp-4);color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-sm);outline:none"
            />
            <span style="font-size:var(--fs-xs);color:var(--ink-4)">from</span>
            <select class="osel" [value]="newFrom() ?? current()" (change)="newFrom.set($any($event.target).value)" style="width:160px;flex:none;padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)">
              @for (b of names(); track b) {
                <option [value]="b">{{ b }}</option>
              }
            </select>
          </div>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            @for (b of rows(); track b.name) {
              @let held = b.checkedOutIn !== undefined;
              <div
                style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair)"
                [style.background]="b.current ? 'color-mix(in oklch, var(--accent), transparent 92%)' : 'transparent'"
              >
                <app-icon name="branch" size="sm" [color]="b.current ? 'var(--accent)' : 'var(--ink-4)'" />
                <span style="font-size:var(--fs-sm)" [style.color]="b.current ? 'var(--ink)' : 'var(--ink-2)'">{{ b.name }}</span>
                @if (b.current) {
                  <span class="chip" style="font-size:var(--fs-3xs);padding:0 var(--sp-2);color:var(--accent);border-color:color-mix(in oklch, var(--accent), transparent 58%)">HEAD</span>
                } @else if (held) {
                  <span class="chip" style="font-size:var(--fs-3xs);padding:0 var(--sp-2);color:var(--ink-4)" [title]="'checked out in ' + (b.checkedOutIn || 'the project checkout')">in use</span>
                }
                @if (b.upstream) {
                  <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ink-4)" [title]="'upstream ' + b.upstream">
                    {{ b.upstream }}@if (b.ahead || b.behind) { · ↑{{ b.ahead }} ↓{{ b.behind }} }
                  </span>
                }
                @if (renameFor() === b.name) {
                  <input
                    class="br-rename"
                    [value]="renameTo()"
                    (input)="renameTo.set($any($event.target).value)"
                    (keydown.enter)="doRename(p.id, b.name)"
                    (keydown.escape)="renameFor.set(null)"
                    spellcheck="false"
                  />
                  <button class="btn br-op" [disabled]="!renameTo().trim()" (click)="doRename(p.id, b.name)">OK</button>
                  <button class="btn br-op" (click)="renameFor.set(null)">Cancel</button>
                } @else if (deleteFor() === b.name) {
                  <span style="font-size:var(--fs-xs);color:var(--ink-3)">delete {{ b.name }}?</span>
                  <button class="btn br-op danger" [disabled]="store.busy()" (click)="doDelete(p.id, b.name, false)">Delete</button>
                  <button class="btn br-op danger" [disabled]="store.busy()" title="delete even if unmerged" (click)="doDelete(p.id, b.name, true)">Force</button>
                  <button class="btn br-op" (click)="deleteFor.set(null)">Cancel</button>
                } @else {
                  <div style="margin-left:auto;display:flex;gap:var(--sp-1)">
                    @if (!b.current) {
                      <button class="btn br-op" [disabled]="store.busy() || held || !agent()" [title]="checkoutTitle(b)" (click)="checkout(p.id, b.name)">
                        <app-icon name="enter" size="sm" [px]="11" />Checkout
                      </button>
                    }
                    <button class="btn br-op" [disabled]="store.busy()" [title]="upstreamTitle(b)" (click)="toggleUpstream(p.id, b)">
                      <app-icon name="link" size="sm" [px]="11" />Upstream
                    </button>
                    @if (!b.current && agent()) {
                      <button class="btn br-op" [disabled]="store.busy()" [title]="'merge ' + b.name + ' into ' + agent()!.branch + ' · native (conflicts open the resolver)'" (click)="mergeIn(b.name)">
                        <app-icon name="merge" size="sm" [px]="11" />Merge in
                      </button>
                    }
                    <button class="btn br-op" [disabled]="store.busy() || held" [title]="held ? 'in use — cannot rename' : 'rename branch'" (click)="startRename(b.name)">
                      <app-icon name="rename" size="sm" [px]="11" />Rename
                    </button>
                    @if (!b.current) {
                      <button class="btn br-op danger" [disabled]="store.busy() || held" [title]="held ? 'in use — cannot delete' : 'delete branch'" (click)="deleteFor.set(b.name)">
                        <app-icon name="trash" size="sm" [px]="11" />Delete
                      </button>
                    }
                  </div>
                }
              </div>
            }
            @if (!rows().length) {
              <div style="padding:var(--sp-6);font-size:var(--fs-sm);color:var(--ink-4)">no branches detected — is this a git repo?</div>
            }
          </div>
        </div>
      </div>
    }
  `,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
        min-width: 0;
      }
      .br-op {
        padding: var(--sp-1) var(--sp-3);
        font-size: var(--fs-xs);
        color: var(--ink-3);
      }
      .br-op:disabled {
        opacity: 0.45;
        cursor: default;
      }
      .br-op.danger {
        color: var(--st-blocked);
      }
      .br-rename {
        margin-left: auto;
        width: 180px;
        background: var(--panel);
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
        padding: var(--sp-1) var(--sp-3);
        color: var(--ink);
        font-family: var(--font-mono);
        font-size: var(--fs-sm);
        outline: none;
      }
    `,
  ],
})
export class BranchesPanelComponent {
  readonly agent = input<Agent | null>(null);
  readonly project = input<Project | undefined>(undefined);
  /** The scoped project's agents — each worktree contributes its branch. */
  readonly agents = input<Agent[]>([]);

  readonly store = inject(BranchesStore);
  private agentActions = inject(AgentActionsService);
  private ui = inject(UiStore);

  readonly newName = signal("");
  readonly newFrom = signal<string | null>(null);
  readonly renameFor = signal<string | null>(null);
  readonly renameTo = signal("");
  readonly deleteFor = signal<string | null>(null);

  /** The checked-out branch of the scoped worktree (or the project checkout). */
  readonly current = computed(() => this.agent()?.branch ?? this.project()?.branch ?? "main");

  /** Fallback list while the native detail hasn't loaded (or repo-less dirs). */
  private readonly fallbackNames = computed<string[]>(() => {
    const p = this.project();
    const base = p?.branches ?? (p?.branch ? [p.branch] : []);
    const agentBranches = this.agents().map((a) => a.branch);
    return [...base, ...agentBranches].filter((b, i, arr) => b && arr.indexOf(b) === i);
  });

  /** Live rows from the backend; falls back to bare detected names. */
  readonly rows = computed<BranchInfo[]>(() => {
    const live = this.store.branches();
    if (live.length && this.store.loadedFor() === this.project()?.id) return live;
    return this.fallbackNames().map((name) => ({
      name,
      current: name === this.current(),
      ahead: 0,
      behind: 0,
    }));
  });

  readonly names = computed(() => this.rows().map((b) => b.name));

  readonly branchEstimate = computed<EstimateInput>(() => ({
    op: "branch",
    model: this.agent()?.model,
  }));

  constructor() {
    effect(() => {
      const p = this.project();
      if (p && this.store.loadedFor() !== p.id) void this.store.load(p.id);
    });
  }

  pullTitle(): string {
    const ag = this.agent();
    return ag
      ? `git pull --ff-only in ${ag.name}'s worktree`
      : "git pull --ff-only in the project checkout";
  }

  pull(projectId: string): void {
    const ag = this.agent();
    if (ag) void this.store.pullAgent(projectId, ag.id);
    else void this.store.pull(projectId);
  }

  create(projectId: string): void {
    const name = this.newName().trim();
    if (!name) return;
    void this.store.create(projectId, name, this.newFrom() ?? this.current()).then((ok) => {
      if (ok) this.newName.set("");
    });
  }

  checkoutTitle(b: BranchInfo): string {
    if (b.checkedOutIn !== undefined)
      return `checked out in ${b.checkedOutIn || "the project checkout"}`;
    const ag = this.agent();
    return ag ? `check out in ${ag.name}'s worktree` : "select an agent to check out into";
  }

  checkout(projectId: string, branch: string): void {
    const ag = this.agent();
    if (!ag) return;
    void this.store.checkoutAgent(projectId, ag.id, branch);
  }

  upstreamTitle(b: BranchInfo): string {
    return b.upstream
      ? `unset upstream (currently ${b.upstream})`
      : `set upstream to ${this.defaultUpstream(b.name) ?? "origin/<branch> (no remote)"}`;
  }

  private defaultUpstream(branch: string): string | null {
    const origin =
      this.store.remotes().find((r) => r.name === "origin") ?? this.store.remotes()[0];
    return origin ? `${origin.name}/${branch}` : null;
  }

  toggleUpstream(projectId: string, b: BranchInfo): void {
    if (b.upstream) {
      void this.store.setUpstream(projectId, b.name, null);
      return;
    }
    const target = this.defaultUpstream(b.name);
    if (!target) {
      this.ui.flash("no remote configured — add one first");
      return;
    }
    void this.store.setUpstream(projectId, b.name, target);
  }

  mergeIn(branch: string): void {
    const ag = this.agent();
    if (ag) this.agentActions.mergeAgent(ag.id, branch);
  }

  startRename(name: string): void {
    this.renameFor.set(name);
    this.renameTo.set(name);
    this.deleteFor.set(null);
  }

  doRename(projectId: string, old: string): void {
    const next = this.renameTo().trim();
    if (!next || next === old) {
      this.renameFor.set(null);
      return;
    }
    void this.store.rename(projectId, old, next).then((ok) => {
      if (ok) this.renameFor.set(null);
    });
  }

  doDelete(projectId: string, name: string, force: boolean): void {
    void this.store.delete(projectId, name, force).then((ok) => {
      if (ok) this.deleteFor.set(null);
    });
  }
}
