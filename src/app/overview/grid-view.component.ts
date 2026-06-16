import { ChangeDetectionStrategy, Component, inject, input } from "@angular/core";
import { Agent } from "../models";
import { ProjectActionsService } from "../projects/project-actions.service";
import { AgentCardComponent } from "./agent-card.component";

@Component({
  selector: "app-grid-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [AgentCardComponent],
  template: `
    <div style="display:grid;gap:var(--sp-6);padding:var(--sp-7);grid-template-columns:repeat(auto-fill,minmax(320px,1fr));align-content:start">
      @for (ag of agents(); track ag.id) {
        <app-agent-card [agent]="ag" [proj]="projects.projectOf(ag.projectId)" />
      }
    </div>
  `,
})
export class GridViewComponent {
  readonly projects = inject(ProjectActionsService);
  readonly agents = input.required<Agent[]>();
}
