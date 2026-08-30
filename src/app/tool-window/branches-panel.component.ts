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
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import {
  KjBadgeComponent,
  KjButtonComponent,
  KjConfirmPopupActionComponent,
  KjConfirmPopupActionsComponent,
  KjConfirmPopupCancelComponent,
  KjConfirmPopupComponent,
  KjConfirmPopupContentComponent,
  KjConfirmPopupMessageComponent,
  KjConfirmPopupTriggerComponent,
  KjInputComponent,
} from "@kouji-ui/components";
import { SelectComponent } from "../shared/select.component";

/**
 * Bottom-dock "Branches" panel — LIVE since A3.2: fetch/pull (CLI shell-out so
 * the OS credential helper auths), checkout into the scoped agent's worktree,
 * new branch / rename / delete / upstream via git2 with worktree-occupancy
 * pre-checks, "Merge in" through the existing native-merge conflict session.
 */
@Component({
  selector: "app-branches-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    KjButtonComponent,
    KjBadgeComponent,
    KjConfirmPopupComponent,
    KjConfirmPopupTriggerComponent,
    KjConfirmPopupContentComponent,
    KjConfirmPopupMessageComponent,
    KjConfirmPopupActionsComponent,
    KjConfirmPopupActionComponent,
    KjConfirmPopupCancelComponent,
    KjInputComponent,
    SelectComponent,
  ],
  template: `
    @if (!project()) {
      <div class="pane-empty pad">select a project to see its branches &amp; remotes</div>
    } @else {
      @let p = project()!;
      <div style="display:grid;grid-template-columns:300px 1fr;min-height:0;flex:1">
        <!-- remotes -->
        <div style="display:flex;flex-direction:column;min-height:0;border-right:1px solid var(--hair)">
          <span class="up pane-head" style="color:var(--ink-3)">Remotes</span>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            @for (r of store.remotes(); track r.name) {
              <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair)">
                <app-icon name="globe" size="sm" color="var(--ink-4)" />
                <div style="flex:1;min-width:0">
                  <div style="color:var(--ink-2)">{{ r.name }}</div>
                  <div class="trunc" style="font-size:var(--fs-meta);color:var(--ink-4)" [title]="r.url">{{ r.url }}</div>
                </div>
                <kj-button kjVariant="toolbar" [kjDisabled]="store.busy()" (click)="store.fetch(p.id, r.name)" title="git fetch --prune {{ r.name }}">
                  <app-icon size="md" name="refresh" />Fetch
                </kj-button>
              </div>
            }
            @if (!store.remotes().length) {
              <div class="pane-empty pad">no remotes configured</div>
            }
          </div>
        </div>

        <!-- branches -->
        <div style="display:flex;flex-direction:column;min-height:0">
          <div class="pane-head">
            <span class="up" style="color:var(--ink-3)">Branches · {{ p.name }}</span>
            @if (store.busy()) {
              <span class="dot running" style="background:var(--st-running)"></span>
            }
            <div style="margin-left:auto;display:flex;gap:var(--sp-3);align-items:center">
              <kj-button kjVariant="toolbar" [kjDisabled]="store.busy()" title="git fetch --all --prune" (click)="store.fetch(p.id)">
                <app-icon name="refresh" size="sm" />Fetch
              </kj-button>
              <kj-button kjVariant="toolbar" [kjDisabled]="store.busy()" [title]="pullTitle()" (click)="pull(p.id)">
                <app-icon name="stage" size="sm" />Pull
              </kj-button>
              <kj-button
                kjVariant="default"
                [kjDisabled]="store.busy() || !newName().trim()"
                title="create the branch named below"
                (click)="create(p.id)"
              >
                <app-icon name="plus" size="sm" />New branch
              </kj-button>
            </div>
          </div>
          <div class="pane-head" style="background:var(--panel-2)">
            <kj-input
              kjSize="sm"
              [value]="newName()"
              (input)="newName.set($any($event.target).value)"
              (keydown.enter)="create(p.id)"
              placeholder="new branch name…"
            />
            <span style="color:var(--ink-4)">from</span>
            <app-select [value]="newFrom() ?? current()" [options]="names()" (valueChange)="newFrom.set($event)" style="width: round(calc(160px * var(--density)), 1px);flex:none" />
          </div>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            @for (b of rows(); track b.name) {
              @let held = b.checkedOutIn !== undefined;
              <div
                class="pane-head"
                [style.background]="b.current ? 'var(--ui-sel)' : 'transparent'"
              >
                <app-icon name="branch" size="sm" [color]="b.current ? 'var(--ui-ink)' : 'var(--ink-4)'" />
                <span [style.color]="b.current ? 'var(--ink)' : 'var(--ink-2)'">{{ b.name }}</span>
                @if (b.current) {
                  <kj-badge style="font-size:var(--fs-badge);padding:0 var(--sp-2);color:var(--ui-ink);border-color:var(--ui-line)">HEAD</kj-badge>
                } @else if (held) {
                  <kj-badge style="font-size:var(--fs-badge);padding:0 var(--sp-2);color:var(--ink-4)" [title]="'checked out in ' + (b.checkedOutIn || 'the project checkout')">in use</kj-badge>
                }
                @if (b.upstream) {
                  <span class="tnum" style="font-size:var(--fs-meta);color:var(--ink-4)" [title]="'upstream ' + b.upstream">
                    {{ b.upstream }}@if (b.ahead || b.behind) { · ↑{{ b.ahead }} ↓{{ b.behind }} }
                  </span>
                }
                @if (renameFor() === b.name) {
                  <span style="margin-left:auto;width: round(calc(180px * var(--density)), 1px);flex:none">
                    <kj-input
                      [value]="renameTo()"
                      (input)="renameTo.set($any($event.target).value)"
                      (keydown.enter)="doRename(p.id, b.name)"
                      (keydown.escape)="renameFor.set(null)"
                    />
                  </span>
                  <kj-button kjVariant="toolbar" [kjDisabled]="!renameTo().trim()" (click)="doRename(p.id, b.name)">OK</kj-button>
                  <kj-button kjVariant="toolbar" (click)="renameFor.set(null)">Cancel</kj-button>
                } @else {
                  <div style="margin-left:auto;display:flex;gap:var(--sp-1)">
                    @if (!b.current && agent()) {
                      <kj-button kjVariant="toolbar" [title]="'diff ' + b.name + ' against ' + current() + ' — file list + per-file diffs in the center'" (click)="compare(b.name)">
                        <app-icon size="md" name="diff" />Diff
                      </kj-button>
                    }
                    @if (!b.current) {
                      <kj-button kjVariant="toolbar" [kjDisabled]="store.busy() || held || !agent()" [title]="checkoutTitle(b)" (click)="checkout(p.id, b.name)">
                        <app-icon size="md" name="enter" />Checkout
                      </kj-button>
                    }
                    <kj-button kjVariant="toolbar" [kjDisabled]="store.busy()" [title]="upstreamTitle(b)" (click)="toggleUpstream(p.id, b)">
                      <app-icon size="md" name="link" />Upstream
                    </kj-button>
                    @if (!b.current && agent()) {
                      <kj-button kjVariant="toolbar" [kjDisabled]="store.busy()" [title]="'merge ' + b.name + ' into ' + agent()!.branch + ' · native (conflicts open the resolver)'" (click)="mergeIn(b.name)">
                        <app-icon size="md" name="merge" />Merge in
                      </kj-button>
                    }
                    <kj-button kjVariant="toolbar" [kjDisabled]="store.busy() || held" [title]="held ? 'in use — cannot rename' : 'rename branch'" (click)="startRename(b.name)">
                      <app-icon size="md" name="rename" />Rename
                    </kj-button>
                    @if (!b.current) {
                      <kj-confirm-popup [kjDestructive]="true">
                        <kj-confirm-popup-trigger #delTrig="kjConfirmPopupTrigger">
                          <kj-button kjVariant="danger" [kjDisabled]="store.busy() || held" [title]="held ? 'in use — cannot delete' : 'delete branch'">
                            <app-icon size="md" name="trash" />Delete
                          </kj-button>
                        </kj-confirm-popup-trigger>
                        <kj-confirm-popup-content [kjFor]="delTrig">
                          <kj-confirm-popup-message>delete {{ b.name }}?</kj-confirm-popup-message>
                          <kj-confirm-popup-actions>
                            <kj-confirm-popup-cancel><kj-button kjVariant="toolbar">Cancel</kj-button></kj-confirm-popup-cancel>
                            <kj-confirm-popup-action><kj-button kjVariant="danger" title="delete even if unmerged" (click)="doDelete(p.id, b.name, true)">Force</kj-button></kj-confirm-popup-action>
                            <kj-confirm-popup-action><kj-button kjVariant="danger" (click)="doDelete(p.id, b.name, false)">Delete</kj-button></kj-confirm-popup-action>
                          </kj-confirm-popup-actions>
                        </kj-confirm-popup-content>
                      </kj-confirm-popup>
                    }
                  </div>
                }
              </div>
            }
            @if (!rows().length) {
              <div class="pane-empty pad">no branches detected — is this a git repo?</div>
            }
          </div>
        </div>
      </div>
    }
  `,
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

  /** Open the center range-inspection view diffing `branch` against the scoped
   *  worktree's branch — the backend revparses refs, so branch names work as
   *  range boundaries (tree-to-tree, oldest tip → newest tip). */
  compare(branch: string): void {
    const ag = this.agent();
    if (ag) this.ui.setGitView(ag.id, { kind: "range", shas: [branch, ag.branch] });
  }

  startRename(name: string): void {
    this.renameFor.set(name);
    this.renameTo.set(name);
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
    void this.store.delete(projectId, name, force);
  }
}
