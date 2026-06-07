import { ChangeDetectionStrategy, Component, computed, effect, inject, input, signal } from "@angular/core";
import { Agent, Project } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusPillComponent } from "../shared/status-pill.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { fmtDur, mix, toolMeta } from "../utils";
import { DiffViewComponent } from "./diff-view.component";
import { TerminalComponent } from "./terminal.component";

interface PaneDef {
  key: string;
  icon: string;
  label: string;
  badge?: number | "!" | null;
}

@Component({
  selector: "app-workspace",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    StatusPillComponent,
    IconComponent,
    ToolBadgeComponent,
    DiffViewComponent,
    TerminalComponent,
  ],
  template: `
    @let ag = agent();
    <div style="display:flex;flex-direction:column;min-height:0;background:var(--panel-2)">
      <!-- agent header -->
      <div style="padding:12px 18px;border-bottom:1px solid var(--hair);background:var(--panel)">
        <div style="display:flex;align-items:center;gap:10px">
          <h1 class="disp" style="font-size:15px;font-weight:600">{{ ag.name }}</h1>
          <app-status-pill [status]="ag.status" [filled]="true" />
          @if (ag.status === 'running') {
            @if (ag.working) { <div class="activity" style="width:60px"></div> }
            <span style="font-size:10px" [style.color]="ag.needsInput ? 'var(--st-blocked)' : 'var(--ink-3)'">
              {{ ag.working ? 'working…' : ag.needsInput ? 'needs input' : 'quiet' }}
            </span>
          }
          <div style="margin-left:auto;display:flex;gap:6px">
            @if (ag.status === 'running') {
              <button class="btn ghost-hair" (click)="agentActions.act(ag.id, 'pause')"><app-icon name="pause" size="sm" />Pause</button>
            } @else if (ag.status !== 'done') {
              <button class="btn ghost-hair" (click)="agentActions.act(ag.id, ag.started ? 'resume' : 'start')">
                <app-icon name="play" size="sm" />{{ ag.started ? 'Resume' : 'Start' }}
              </button>
            }
            <button class="btn ghost-hair" (click)="agentActions.act(ag.id, 'commit')"><app-icon name="commit" size="sm" />Commit</button>
            <button class="btn primary" (click)="agentActions.act(ag.id, 'merge')">
              <app-icon name="merge" size="sm" />Merge to main
            </button>
          </div>
        </div>
        <div style="display:flex;align-items:center;gap:12px;margin-top:8px;font-size:11px;color:var(--ink-3)">
          <span style="color:var(--ink-2)">{{ ag.task }}</span>
        </div>
        <div class="tnum" style="display:flex;align-items:center;gap:14px;margin-top:8px;font-size:10.5px;color:var(--ink-3)">
          @if (project()) {
            <span [style.color]="project()!.color" style="display:flex;gap:5px;align-items:center">
              <app-icon [name]="project()!.icon" size="sm" [px]="12" />{{ project()!.name }}
            </span>
          }
          <span style="display:flex;gap:5px"><app-icon name="branch" size="sm" [px]="12" color="var(--accent-2)" />{{ ag.branch }}</span>
          <span style="display:flex;gap:5px;align-items:center" [title]="tool(ag.tool).name"><app-tool-badge [tool]="ag.tool" [size]="13" />{{ ag.model }}{{ ag.effort ? ' · ' + ag.effort : '' }}</span>
          <span style="display:flex;gap:5px"><app-icon name="commit" size="sm" [px]="12" />{{ ag.commits }} commits</span>
          <span style="display:flex;gap:5px"><app-icon name="clock" size="sm" [px]="12" />{{ ag.elapsed ? fmt(ag.elapsed) : '—' }}</span>
        </div>
        <!-- full worktree path, beneath the project/branch row -->
        <div class="tnum" style="margin-top:6px;font-size:10px;color:var(--ink-4);display:flex;gap:5px;align-items:center">
          <app-icon name="folder" size="sm" [px]="12" />
          <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [title]="ag.worktree">{{ ag.worktree }}</span>
        </div>
      </div>

      <!-- pane tabs -->
      <div style="display:flex;align-items:center;gap:2px;padding:0 12px;background:var(--panel);border-bottom:1px solid var(--hair)">
        @for (p of panes(); track p.key) {
          @let on = pane() === p.key;
          <button class="btn" (click)="pane.set(p.key)" [style.color]="on ? 'var(--ink)' : 'var(--ink-3)'" style="padding:9px 12px;border-radius:0;position:relative">
            @if (on) { <span style="position:absolute;left:10px;right:10px;bottom:0;height:2px;background:linear-gradient(90deg,var(--accent),var(--accent-2))"></span> }
            <app-icon [name]="p.icon" size="sm" [color]="on ? 'var(--accent)' : null" />{{ p.label }}
            @if (p.badge != null) {
              <span
                class="chip tnum"
                [style.color]="p.badge === '!' ? 'var(--st-blocked)' : 'var(--ink-2)'"
                [style.border-color]="p.badge === '!' ? mix('var(--st-blocked)', 60) : 'var(--hair)'"
                style="font-size:9px;padding:0 5px"
              >{{ p.badge }}</span>
            }
          </button>
        }
      </div>

      <!-- pane body -->
      @switch (pane()) {
        @case ('diff') { <app-diff-view [agent]="ag" /> }
        @case ('terminal') { <app-terminal [agent]="ag" /> }
      }
    </div>
  `,
})
export class WorkspaceComponent {
  readonly agentActions = inject(AgentActionsService);
  private ui = inject(UiStore);
  readonly agent = input.required<Agent>();
  readonly project = input<Project | undefined>(undefined);

  readonly pane = signal<string>("terminal");
  readonly fmt = fmtDur;
  readonly mix = mix;
  readonly tool = toolMeta;

  readonly panes = computed<PaneDef[]>(() => {
    const ag = this.agent();
    return [
      { key: "diff", icon: "diff", label: "Diff", badge: ag.git_changes?.files.length ?? 0 },
      { key: "terminal", icon: "terminal", label: "Terminal" },
    ];
  });

  private lastId: string | null = null;
  private lastPaneSeq = 0;
  constructor() {
    // reset the active pane when switching to a different agent, honoring any pane hint
    effect(() => {
      const id = this.agent().id;
      if (id !== this.lastId) {
        this.lastId = id;
        this.pane.set(this.ui.paneHint[id] || "terminal");
      }
    });
    // honor a one-shot pane request even for the already-active agent (e.g. inbox
    // "open terminal"). seq de-dupes; manual tab clicks set pane directly so they
    // never touch paneRequest and are never clobbered here.
    effect(() => {
      const req = this.ui.paneRequest();
      if (req && req.id === this.agent().id && req.seq !== this.lastPaneSeq) {
        this.lastPaneSeq = req.seq;
        this.pane.set(req.pane);
      }
    });
  }
}
