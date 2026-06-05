import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";

@Component({
  selector: "app-status-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <footer style="display:flex;align-items:center;gap:14px;padding:0 14px;background:var(--panel);border-top:1px solid var(--hair);font-size:10.5px;color:var(--ink-3)">
      <span style="display:flex;gap:6px;align-items:center">
        <span class="dot running" style="background:var(--st-running)"></span>{{ running() }} running
      </span>
      @if (blocked() > 0) {
        <span style="display:flex;gap:6px;align-items:center;color:var(--st-blocked)">
          <span class="dot" style="background:var(--st-blocked)"></span>{{ blocked() }} need attention
        </span>
      }
      <span class="tnum">{{ projects.all().length }} projects · {{ runtime.agents().length }} agents</span>
      <span style="display:flex;gap:5px"><app-icon name="folder" size="sm" [px]="11" />{{ ui.worktreeRoot }}</span>
      <span class="tnum" style="margin-left:auto;display:flex;gap:5px;align-items:center">
        @if (ui.toast()) {
          <span class="grad-ink" style="font-weight:600">{{ ui.toast() }}</span>
          <span style="color:var(--ink-4)">·</span>
        }
        <app-icon name="link" size="sm" [px]="11" />orchestrator: healthy
      </span>
    </footer>
  `,
})
export class StatusBarComponent {
  readonly runtime = inject(AgentRuntimeService);
  readonly projects = inject(ProjectActionsService);
  readonly ui = inject(UiStore);
  readonly running = computed(() => this.runtime.agents().filter((a) => a.status === "running").length);
  readonly blocked = computed(() => this.runtime.agents().filter((a) => a.status === "blocked").length);
}
