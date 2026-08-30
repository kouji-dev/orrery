import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  input,
  signal,
} from "@angular/core";
import { Agent, AgentFile } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentWorkStore } from "../agents/agent-work.store";
import { ProjectActionsService } from "../projects/project-actions.service";
import { EstimateInput } from "../cost/estimate.service";
import { IconComponent } from "../shared/icon.component";
import { GitActionButtonComponent } from "../shared/git/git-action-button.component";
import { KjButtonComponent } from "@kouji-ui/components";

/**
 * v2 git action bar — every Act verb docked directly under the diff it judges
 * (design v2.jsx GitActionBar). One dense strip: commit-message input,
 * stage toggle, Commit / Push / Rebase / Merge split buttons (native primary +
 * AI variants with cost estimates — never hide agent cost), and Discard last,
 * visually separated. Disabled states are real: no changes → no commit or
 * discard; no commits ahead → no push or merge.
 */
@Component({
  selector: "app-git-action-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, GitActionButtonComponent, KjButtonComponent],
  template: `
    @let ag = agent();
    @let dirty = changes().length > 0;
    @let ahead = ag.commits > 0;
    <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-5);border-top:1px solid var(--hair);background:var(--panel);flex:none;flex-wrap:wrap;position:relative;z-index:3">
      <input
        [value]="msg()"
        (input)="msg.set($any($event.target).value)"
        [disabled]="!dirty"
        [placeholder]="dirty ? 'commit message — ' + changes().length + ' files' : 'working tree clean'"
        (keydown)="onMsgKey($event)"
        [style.opacity]="dirty ? 1 : 0.55"
        class="gab-msg"
        style="flex:1;min-width:round(calc(150px * var(--density)), 1px);height:var(--ctl-h);padding:0 var(--sp-5);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm);color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-sm);outline:none"
      />
      <kj-button
        kjVariant="outline"
        kjSize="xs"
        [kjDisabled]="!dirty"
        (click)="toggleStaged()"
        title="Stage / unstage the working tree"
        [style.--kj-button-bg]="dirty && staged() ? 'var(--ui-sel)' : null"
        [style.--kj-button-border-color]="dirty && staged() ? 'var(--ui-line)' : null"
        [style.--kj-button-fg]="dirty && staged() ? 'var(--ink)' : null"
      >
        <app-icon name="stage" size="sm" [color]="dirty && staged() ? 'var(--ui-ink)' : null" />
        {{ dirty ? (staged() ? 'Staged all · ' + changes().length : 'Unstaged') : 'Stage all' }}
      </kj-button>
      <app-git-action-button
        label="Commit"
        icon="commit"
        [small]="true"
        [kind]="dirty && staged() ? 'primary' : 'ghost-hair'"
        [disabled]="!dirty || !staged()"
        title="Commit staged changes · Ctrl+Enter"
        [estimateInput]="commitEstimate()"
        [variants]="[{ id: 'commit', label: 'Commit with AI (write the message)', icon: 'sparkles' }]"
        (native)="commit()"
        (ai)="agentActions.aiAction(ag.id, 'commit')"
      />
      <kj-button kjVariant="outline" kjSize="xs" [kjDisabled]="!ahead" (click)="agentActions.pushAgent(ag.id)" title="Push to origin">
        <app-icon name="push" size="sm" />Push
        @if (ahead) { <span class="chip tnum" style="font-size:var(--fs-badge);padding:0 var(--sp-3)">{{ ag.commits }}↑</span> }
      </kj-button>
      <!-- rebase/merge are branch-integration verbs — a project tab (the v2
           "shell" pseudo-agent) IS the base branch, so they hide there -->
      @if (ag.tool !== 'shell') {
        <app-git-action-button
          [label]="'Rebase onto ' + baseBranch()"
          icon="sparkles"
          [small]="true"
          [aiOnly]="true"
          [estimateInput]="rebaseEstimate()"
          [variants]="[
            { id: 'rebase', label: 'Rebase with AI', icon: 'sparkles' },
            { id: 'rebase-v', label: 'Rebase with AI (verbose)', verbose: true, icon: 'sparkles' },
          ]"
          (ai)="agentActions.aiAction(ag.id, 'rebase')"
        />
        <app-git-action-button
          [label]="'Merge ' + baseBranch() + ' → branch'"
          icon="merge"
          [small]="true"
          [disabled]="!ahead"
          [title]="'Merge ' + baseBranch() + ' into ' + ag.branch + ' · native'"
          [estimateInput]="mergeEstimate()"
          [variants]="[{ id: 'merge', label: 'Merge with AI (agent resolves conflicts)', icon: 'sparkles' }]"
          (native)="agentActions.mergeAgent(ag.id, baseBranch())"
          (ai)="agentActions.aiAction(ag.id, 'merge')"
        />
      }
      <span style="width:1px;height:var(--sp-8);background:var(--hair);margin:0 var(--sp-1);flex:none"></span>
      <kj-button kjVariant="danger" kjSize="xs" [kjDisabled]="!dirty" (click)="discard()">
        <app-icon name="discard" size="sm" />Discard
      </kj-button>
    </div>
  `,
  styles: [`.gab-msg:focus { border-color: var(--ui-focus) !important; }`],
})
export class GitActionBarComponent {
  readonly agentActions = inject(AgentActionsService);
  private readonly work = inject(AgentWorkStore);
  private readonly projects = inject(ProjectActionsService);
  readonly agent = input.required<Agent>();

  // id, not the Agent object: runtime overlay patches re-create agent objects;
  // the stable id keeps downstream computeds from churning (same reasoning as
  // DiffViewComponent).
  private readonly agentId = computed(() => this.agent().id);
  readonly changes = computed<AgentFile[]>(() => this.work.changesFor(this.agentId()).data);
  readonly baseBranch = computed(
    () => this.projects.projectOf(this.agent().projectId)?.branch ?? "main",
  );

  readonly msg = signal("");
  /** Mock semantics: the toggle arms/disarms Commit — everything is staged by
   *  default (agent_commit stages what it commits); "Unstaged" parks the bar. */
  readonly staged = signal(true);

  private lastId: string | null = null;
  constructor() {
    // reset the draft when the bar switches agents
    effect(() => {
      const id = this.agentId();
      if (id !== this.lastId) {
        this.lastId = id;
        this.msg.set("");
        this.staged.set(true);
      }
    });
  }

  // Why *40: status carries line counts, not bytes — 40 bytes/line is the same
  // conservative envelope the old Git tab used (a cost ENVELOPE, not an invoice).
  private readonly diffBytes = computed(() =>
    this.changes().reduce((s, f) => s + (f.add + f.del) * 40, 0),
  );
  readonly commitEstimate = computed<EstimateInput>(() => ({
    op: "commit",
    files: this.changes().length,
    diffBytes: this.diffBytes(),
    model: this.agent().model,
  }));
  readonly rebaseEstimate = computed<EstimateInput>(() => ({
    op: "rebase",
    files: this.changes().length,
    diffBytes: this.diffBytes(),
    model: this.agent().model,
  }));
  readonly mergeEstimate = computed<EstimateInput>(() => ({
    op: "merge",
    files: this.changes().length,
    diffBytes: this.diffBytes(),
    model: this.agent().model,
  }));

  toggleStaged(): void {
    this.staged.update((v) => !v);
  }

  onMsgKey(e: KeyboardEvent): void {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") this.commit();
  }

  commit(): void {
    if (!this.changes().length || !this.staged()) return;
    const msg = this.msg().trim() || "wip: " + this.agent().name;
    this.agentActions.commitAgent(this.agentId(), [], msg); // [] = all changes
    this.msg.set("");
  }

  discard(): void {
    this.agentActions.discardAgent(this.agentId(), []); // [] = all changes
  }
}
