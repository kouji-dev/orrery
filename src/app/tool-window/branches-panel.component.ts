import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import { Agent, Project } from "../models";
import { EstimateInput } from "../cost/estimate.service";
import { GitActionButtonComponent } from "../shared/git/git-action-button.component";
import { IconComponent } from "../shared/icon.component";

const A32 = "arrives with A3.2 (native branch & remote ops)";

/**
 * Bottom-dock "Branches" panel (design git-panels.jsx BranchesPanel).
 * The BRANCH LIST is real — the project's detected branches plus each
 * worktree's agent branch. Every mutation (fetch/pull/checkout/new branch,
 * remote add/prune/…) waits on the A3.2 native ops: the controls render
 * design-faithful but DISABLED with an explanatory tooltip — no fake data,
 * no pretend progress.
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
            <button class="btn ghost-hair" disabled [title]="a32" style="margin-left:auto;padding:var(--sp-1) var(--sp-3);font-size:var(--fs-xs)">
              <app-icon name="plus" size="sm" />Add
            </button>
          </div>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            <div style="padding:var(--sp-6);display:grid;gap:var(--sp-3)">
              <div style="display:flex;align-items:center;gap:var(--sp-3)">
                <app-icon name="globe" size="sm" color="var(--ink-4)" />
                <span style="font-size:var(--fs-sm);color:var(--ink-3)">no remotes to show yet</span>
              </div>
              <div style="font-size:var(--fs-2xs);color:var(--ink-4);line-height:1.5;text-wrap:pretty">
                the remote list — with fetch, prune, set-URL and remove — {{ a32 }}
              </div>
            </div>
          </div>
        </div>

        <!-- branches -->
        <div style="display:flex;flex-direction:column;min-height:0">
          <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
            <span class="up" style="font-size:var(--fs-3xs);color:var(--ink-3)">Branches · {{ p.name }}</span>
            <div style="margin-left:auto;display:flex;gap:var(--sp-3);align-items:center">
              <app-git-action-button label="Fetch" icon="refresh" [small]="true" [disabled]="true" [title]="a32" [estimateInput]="branchEstimate()" />
              <app-git-action-button label="Pull" icon="stage" [small]="true" [disabled]="true" [title]="a32" [estimateInput]="branchEstimate()" />
              <app-git-action-button
                label="New branch"
                icon="plus"
                kind="primary"
                [small]="true"
                [disabled]="true"
                [title]="a32"
                [estimateInput]="branchEstimate()"
                [variants]="[{ id: 'name', label: 'Name branch from the task', op: 'branch', icon: 'spark' }]"
              />
            </div>
          </div>
          <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair);flex:none;background:var(--panel-2)">
            <input
              disabled
              placeholder="new branch name…"
              [title]="a32"
              style="flex:1;background:var(--panel);border:1px solid var(--hair);border-radius:var(--r-sm);padding:var(--sp-2) var(--sp-4);color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-sm);outline:none;opacity:.55"
            />
            <span style="font-size:var(--fs-xs);color:var(--ink-4)">from</span>
            <select class="osel" disabled [title]="a32" style="width:160px;flex:none;padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm);opacity:.55">
              <option>{{ current() }}</option>
            </select>
          </div>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            @for (b of branches(); track b) {
              @let on = b === current();
              <div
                style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-6);border-bottom:1px solid var(--hair)"
                [style.background]="on ? 'color-mix(in oklch, var(--accent), transparent 92%)' : 'transparent'"
              >
                <app-icon name="branch" size="sm" [color]="on ? 'var(--accent)' : 'var(--ink-4)'" />
                <span style="font-size:var(--fs-sm)" [style.color]="on ? 'var(--ink)' : 'var(--ink-2)'">{{ b }}</span>
                @if (on) {
                  <span class="chip" style="font-size:var(--fs-3xs);padding:0 var(--sp-2);color:var(--accent);border-color:color-mix(in oklch, var(--accent), transparent 58%)">HEAD</span>
                }
                <div style="margin-left:auto;display:flex;gap:var(--sp-1)">
                  @if (!on) {
                    <button class="btn br-op" disabled [title]="a32"><app-icon name="enter" size="sm" [px]="11" />Checkout</button>
                  }
                  <button class="btn br-op" disabled [title]="a32"><app-icon name="link" size="sm" [px]="11" />Upstream</button>
                  @if (!on) {
                    <button class="btn br-op" disabled [title]="a32"><app-icon name="merge" size="sm" [px]="11" />Merge in</button>
                  }
                  <button class="btn br-op" disabled [title]="a32"><app-icon name="rename" size="sm" [px]="11" />Rename</button>
                  @if (!on) {
                    <button class="btn br-op danger" disabled [title]="a32"><app-icon name="trash" size="sm" [px]="11" />Delete</button>
                  }
                </div>
              </div>
            }
            @if (!branches().length) {
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
      .br-op.danger {
        color: var(--st-blocked);
      }
    `,
  ],
})
export class BranchesPanelComponent {
  readonly agent = input<Agent | null>(null);
  readonly project = input<Project | undefined>(undefined);
  /** The scoped project's agents — each worktree contributes its branch. */
  readonly agents = input<Agent[]>([]);

  readonly a32 = A32;

  /** The checked-out branch of the scoped worktree (or the project checkout). */
  readonly current = computed(() => this.agent()?.branch ?? this.project()?.branch ?? "main");

  /** REAL branch list: backend-detected project branches + worktree branches. */
  readonly branches = computed<string[]>(() => {
    const p = this.project();
    const base = p?.branches ?? (p?.branch ? [p.branch] : []);
    const agentBranches = this.agents().map((a) => a.branch);
    return [...base, ...agentBranches].filter((b, i, arr) => b && arr.indexOf(b) === i);
  });

  readonly branchEstimate = computed<EstimateInput>(() => ({
    op: "branch",
    model: this.agent()?.model,
  }));
}
