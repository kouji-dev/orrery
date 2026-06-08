import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { AgentStatus, VizMode } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { GraphViewComponent } from "./graph-view.component";
import { GridViewComponent } from "./grid-view.component";
import { KanbanViewComponent } from "./kanban-view.component";
import { StatBlockComponent } from "./stat-block.component";
import { TimelineViewComponent } from "./timeline-view.component";

interface VizDef {
  key: VizMode;
  icon: string;
  label: string;
}

@Component({
  selector: "app-overview",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    StatBlockComponent,
    IconComponent,
    GridViewComponent,
    KanbanViewComponent,
    GraphViewComponent,
    TimelineViewComponent,
  ],
  template: `
    <div style="display:flex;flex-direction:column;min-height:0;background:var(--panel-2)">
      <!-- stat header -->
      <div style="display:flex;align-items:center;padding:14px 18px;border-bottom:1px solid var(--hair);background:var(--panel)">
        <div style="margin-right:24px">
          <h1 class="disp" style="font-size:16px;font-weight:600;letter-spacing:-0.02em">Orchestrator</h1>
          <span style="font-size:10.5px;color:var(--ink-3)">
            {{ runtime.agents().length }} agents across {{ projects.all().length }} projects · {{ ui.org }}
          </span>
        </div>
        <app-stat-block [n]="count('running')" label="Running" color="var(--st-running)" [pulse]="true" />
        <app-stat-block [n]="count('blocked')" label="Need you" color="var(--st-blocked)" />
        <app-stat-block [n]="count('waiting') + count('queued')" label="Waiting" color="var(--st-waiting)" />
        <app-stat-block [n]="count('done')" label="Done" color="var(--st-done)" />
        <div style="margin-left:auto;display:flex;align-items:center;gap:8px">
          <div style="display:flex;gap:2px;padding:3px;background:var(--panel-2);border-radius:var(--r-md);border:1px solid var(--hair)">
            @for (v of viz; track v.key) {
              @let on = ui.viz() === v.key;
              <button
                class="btn"
                (click)="ui.viz.set(v.key)"
                [style.background]="on ? 'var(--panel-3)' : 'transparent'"
                [style.color]="on ? 'var(--ink)' : 'var(--ink-3)'"
                [style.box-shadow]="on ? '0 0 0 1px var(--hair-2)' : 'none'"
                style="padding:4px 9px;border-radius:var(--r-sm)"
              >
                <app-icon [name]="v.icon" size="sm" [color]="on ? 'var(--accent)' : null" />{{ v.label }}
              </button>
            }
          </div>
          <button class="btn primary" (click)="ui.openSpawn(null)"><app-icon name="bolt" size="sm" />Spawn</button>
        </div>
      </div>

      <!-- body -->
      <div class="scroll-y" style="flex:1">
        @switch (ui.viz()) {
          @case ('grid') { <app-grid-view [agents]="runtime.agents()" /> }
          @case ('kanban') { <app-kanban-view [agents]="runtime.agents()" /> }
          @case ('graph') { <app-graph-view [agents]="runtime.agents()" [projects]="projects.all()" /> }
          @case ('timeline') { <app-timeline-view [agents]="runtime.agents()" /> }
        }
      </div>
    </div>
  `,
})
export class OverviewComponent {
  readonly runtime = inject(AgentRuntimeService);
  readonly projects = inject(ProjectActionsService);
  readonly ui = inject(UiStore);
  readonly viz: VizDef[] = [
    { key: "grid", icon: "grid", label: "Grid" },
    { key: "kanban", icon: "columns", label: "Board" },
    { key: "graph", icon: "graph", label: "Graph" },
    { key: "timeline", icon: "timeline", label: "Timeline" },
  ];

  count(s: AgentStatus): number {
    return this.runtime.agents().filter((a) => a.status === s).length;
  }
}
