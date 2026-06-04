import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { AgentStatus, VizMode } from "../models";
import { OrchestraStore } from "../orchestra.store";
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
            {{ store.agents().length }} agents across {{ store.projects().length }} projects · {{ store.org }}
          </span>
        </div>
        <app-stat-block [n]="count('running')" label="Running" color="var(--st-running)" [pulse]="true" />
        <app-stat-block [n]="count('blocked')" label="Need you" color="var(--st-blocked)" />
        <app-stat-block [n]="count('waiting') + count('queued')" label="Waiting" color="var(--st-waiting)" />
        <app-stat-block [n]="count('done')" label="Done" color="var(--st-done)" />
        <div style="margin-left:auto;display:flex;align-items:center;gap:8px">
          <div style="display:flex;gap:2px;padding:3px;background:var(--panel-2);border-radius:var(--r-md);border:1px solid var(--hair)">
            @for (v of viz; track v.key) {
              @let on = store.viz() === v.key;
              <button
                class="btn"
                (click)="store.viz.set(v.key)"
                [style.background]="on ? 'var(--panel-3)' : 'transparent'"
                [style.color]="on ? 'var(--ink)' : 'var(--ink-3)'"
                [style.box-shadow]="on ? '0 0 0 1px var(--hair-2)' : 'none'"
                style="padding:4px 9px;border-radius:var(--r-sm)"
              >
                <app-icon [name]="v.icon" size="sm" [color]="on ? 'var(--accent)' : null" />{{ v.label }}
              </button>
            }
          </div>
          <button class="btn primary" (click)="store.openSpawn(null)"><app-icon name="plus" size="sm" />Spawn</button>
        </div>
      </div>

      <!-- body -->
      <div class="scroll-y" style="flex:1">
        @switch (store.viz()) {
          @case ('grid') { <app-grid-view [agents]="store.agents()" /> }
          @case ('kanban') { <app-kanban-view [agents]="store.agents()" /> }
          @case ('graph') { <app-graph-view [agents]="store.agents()" [projects]="store.projects()" /> }
          @case ('timeline') { <app-timeline-view [agents]="store.agents()" /> }
        }
      </div>
    </div>
  `,
})
export class OverviewComponent {
  readonly store = inject(OrchestraStore);
  readonly viz: VizDef[] = [
    { key: "grid", icon: "grid", label: "Grid" },
    { key: "kanban", icon: "columns", label: "Board" },
    { key: "graph", icon: "graph", label: "Graph" },
    { key: "timeline", icon: "timeline", label: "Timeline" },
  ];

  count(s: AgentStatus): number {
    return this.store.agents().filter((a) => a.status === s).length;
  }
}
