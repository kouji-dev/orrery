import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { ToolWindowStore } from "./tool-window.store";
import { IconComponent } from "../shared/icon.component";

/**
 * v2 collapsed graph strip (~28px): history's one home, always one click away.
 * Rendered at the bottom of the center column whenever the tool window is
 * closed — branch, commits ahead of base, and the expand affordance. Clicking
 * (or the shown binding) opens the Git Graph panel in its place.
 */
@Component({
  selector: "app-graph-strip",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let ag = agent();
    <button
      class="graph-strip"
      (click)="toolWindow.open('graph')"
      title="Expand the commit graph · Ctrl+Shift+G"
    >
      <app-icon name="chevron" size="sm" color="var(--ink-4)" />
      <app-icon name="branch" size="sm" />
      <span style="color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;flex:none;max-width:280px">{{ branch() }}</span>
      @if (ag) {
        <span class="tnum">{{ ag.commits }} commit{{ ag.commits === 1 ? '' : 's' }} ahead of {{ base() }}</span>
      } @else {
        <span>select an agent to scope the graph</span>
      }
      <span style="margin-left:auto;display:inline-flex;align-items:center;gap:var(--sp-3)">
        <span class="kbd">Ctrl+Shift+G</span><span>expand graph</span>
      </span>
    </button>
  `,
  styles: [
    `
      .graph-strip {
        display: flex;
        align-items: center;
        gap: var(--sp-4);
        height: var(--ctl-h);
        padding: 0 var(--sp-6);
        width: 100%;
        border: none;
        border-top: 1px solid var(--hair);
        background: var(--panel);
        flex: none;
        font-family: inherit;
        font-size: var(--fs-xs);
        color: var(--ink-3);
        cursor: pointer;
        user-select: none;
      }
      .graph-strip:hover {
        background: var(--panel-3);
      }
    `,
  ],
})
export class GraphStripComponent {
  readonly toolWindow = inject(ToolWindowStore);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly projects = inject(ProjectActionsService);

  readonly agent = computed(() => this.runtime.activeAgent());
  readonly base = computed(() => {
    const ag = this.agent();
    const proj = ag ? this.projects.projectOf(ag.projectId) : this.projects.all()[0];
    return proj?.branch ?? "main";
  });
  readonly branch = computed(() => this.agent()?.branch ?? this.base());
}
