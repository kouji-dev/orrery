import { ChangeDetectionStrategy, Component, input } from "@angular/core";
import { Agent, Project } from "../models";
import { EmptyStateComponent } from "./empty-state.component";
import { FileTreeComponent } from "./file-tree.component";

@Component({
  selector: "app-files-tab",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [FileTreeComponent, EmptyStateComponent],
  template: `
    @if (agent()) {
      <app-file-tree [agent]="agent()!" [project]="project()" />
    } @else {
      <app-empty-state icon="folder" text="Select an agent to browse its worktree" />
    }
  `,
})
export class FilesTabComponent {
  readonly agent = input<Agent | null>(null);
  readonly project = input<Project | undefined>(undefined);
}
