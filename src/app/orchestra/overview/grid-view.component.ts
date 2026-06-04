import { ChangeDetectionStrategy, Component, inject, input } from "@angular/core";
import { Agent } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { AgentCardComponent } from "./agent-card.component";

@Component({
  selector: "app-grid-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [AgentCardComponent],
  template: `
    <div style="display:grid;gap:14px;padding:18px;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));align-content:start">
      @for (ag of agents(); track ag.id) {
        <app-agent-card [agent]="ag" [proj]="store.projectOf(ag.projectId)" />
      }
    </div>
  `,
})
export class GridViewComponent {
  readonly store = inject(OrchestraStore);
  readonly agents = input.required<Agent[]>();
}
