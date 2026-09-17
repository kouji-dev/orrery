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
import { KjButtonComponent, KjTabComponent, KjTabListComponent, KjTabsComponent } from "@kouji-ui/components";

interface VizDef {
  key: VizMode;
  icon: string;
  label: string;
}

@Component({
  selector: "app-overview",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [StatBlockComponent, IconComponent, GridViewComponent, KanbanViewComponent, GraphViewComponent, TimelineViewComponent, KjButtonComponent, KjTabsComponent, KjTabListComponent, KjTabComponent],
  template: `
    <div style="display:flex;flex-direction:column;min-height:0;min-width:0;background:var(--panel-2)">
      <!-- stat header — the stat group and the view controls wrap to their own
           rows when the pane is narrow, so content never widens the layout -->
      <div class="pane-head" style="flex-wrap:wrap;gap:var(--sp-5) 0;padding:var(--sp-6) var(--sp-7);background:var(--panel);min-width:0">
        <div style="margin-right:var(--sp-9);min-width:0">
          <h1>Orchestrator</h1>
          <span style="color:var(--ink-3)">
            {{ runtime.agents().length }} agents across {{ projects.all().length }} projects · {{ ui.org }}
          </span>
        </div>
        <div style="display:flex;align-items:center;flex-wrap:wrap;gap:var(--sp-4) 0;min-width:0">
          <app-stat-block [n]="count('running')" label="Running" color="var(--st-running)" [pulse]="true" />
          <app-stat-block [n]="count('blocked')" label="Need you" color="var(--st-blocked)" />
          <app-stat-block [n]="count('waiting') + count('queued')" label="Waiting" color="var(--st-waiting)" />
          <app-stat-block [n]="count('done')" label="Done" color="var(--st-done)" />
        </div>
        <div style="margin-left:auto;display:flex;align-items:center;flex-wrap:wrap;justify-content:flex-end;gap:var(--sp-4);padding-left:var(--sp-6)">
          <!-- a view switcher IS a tab strip: the body below is its panel.
               pills is the tray shape (design/app.html:5090). -->
          <kj-tabs variant="pills" [value]="ui.viz()" (valueChange)="ui.viz.set($any($event))">
            <kj-tab-list aria-label="Visualization" style="flex-wrap:wrap">
              @for (v of viz; track v.key) {
                @let on = ui.viz() === v.key;
                <kj-tab [value]="v.key">
                  <app-icon [name]="v.icon" size="sm" [color]="on ? 'var(--ui-ink)' : null" />{{ v.label }}
                </kj-tab>
              }
            </kj-tab-list>
          </kj-tabs>
          <kj-button kjVariant="default" (click)="ui.openSpawn(null)"><app-icon name="bolt" size="sm" />Spawn</kj-button>
        </div>
      </div>

      <!-- body: both axes scroll — wide views (board columns, grid cards) must
           never force the pane (or the window) wider -->
      <div class="scroll-y" style="flex:1;overflow-x:auto">
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
