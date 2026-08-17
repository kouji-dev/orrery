import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { NotificationService } from "../notifications/notification.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { mix } from "../utils";
import { FilesTabComponent } from "./files-tab.component";
import { GitTabComponent } from "./git-tab.component";
import { InboxTabComponent } from "./inbox-tab.component";

interface TabDef {
  key: string;
  icon: string;
  label: string;
  badge?: number;
}

@Component({
  selector: "app-right-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, StatusDotComponent, FilesTabComponent, InboxTabComponent, GitTabComponent],
  template: `
    @let scope = runtime.activeAgent();
    <aside style="display:flex;flex-direction:column;min-height:0;min-width:0;overflow:hidden;background:var(--panel);border-left:1px solid var(--hair)">
      @if (scope) {
        <!-- agent header -->
        <div style="display:flex;align-items:center;gap:var(--sp-4);padding:0 var(--sp-6);height:38px;border-bottom:1px solid var(--hair)">
          <app-status-dot [status]="scope.status" />
          <span style="font-size:var(--fs-sm);font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ scope.name }}</span>
          @if (project()) {
            <span class="chip" [style.color]="project()!.color" [style.border-color]="mix(project()!.color, 65)" style="font-size:var(--fs-2xs);padding:1px var(--sp-3)">{{ project()!.name }}</span>
          }
        </div>

        <!-- tab bar -->
        <div style="display:flex;align-items:center;padding:0 var(--sp-3);border-bottom:1px solid var(--hair);height:36px">
          @for (t of tabs(); track t.key) {
            @let on = tab() === t.key;
            <button class="btn" (click)="tab.set(t.key)" [style.color]="on ? 'var(--ink)' : 'var(--ink-3)'" style="padding:var(--sp-3) var(--sp-5);border-radius:0;position:relative;flex:1;justify-content:center">
              @if (on) { <span style="position:absolute;left:8px;right:8px;bottom:0;height:var(--sp-1);background:var(--ui-ind)"></span> }
              <app-icon [name]="t.icon" size="sm" [color]="on ? 'var(--ui-ink)' : null" />
              <span style="font-size:var(--fs-sm)">{{ t.label }}</span>
              @if (t.badge) {
                <span class="chip tnum" style="font-size:var(--fs-2xs);padding:0 var(--sp-2);color:var(--ui-ink);border-color:var(--ui-sel-2)">{{ t.badge }}</span>
              }
            </button>
          }
        </div>

        @switch (tab()) {
          @case ('files') { <app-files-tab [agent]="scope" [project]="project()" /> }
          @case ('inbox') { <app-inbox-tab [scopeAgent]="scope" /> }
          @case ('git') { <app-git-tab [agent]="scope" [project]="project()" /> }
        }
      } @else {
        <!-- empty: this panel is agent-scoped -->
        <div style="flex:1;display:grid;place-items:center;padding:var(--sp-9);text-align:center">
          <div>
            <app-icon name="layers" size="lg" color="var(--hair-2)" />
            <div style="font-size:var(--fs-sm);color:var(--ink-3);margin-top:var(--sp-5)">No agent selected</div>
            <div style="font-size:var(--fs-xs);color:var(--ink-4);margin-top:var(--sp-1)">Open an agent to see its files, git &amp; inbox</div>
          </div>
        </div>
      }
    </aside>
  `,
})
export class RightPanelComponent {
  readonly runtime = inject(AgentRuntimeService);
  private projects = inject(ProjectActionsService);
  private notifications = inject(NotificationService);
  readonly tab = signal<string>("files");
  readonly mix = mix;

  readonly project = computed(() => {
    const sa = this.runtime.activeAgent();
    return sa ? this.projects.projectOf(sa.projectId) : undefined;
  });
  readonly pendingCount = computed(() => {
    const sa = this.runtime.activeAgent();
    const pending = this.notifications.pending();
    return sa ? pending.filter((n) => n.agentId === sa.id).length : pending.length;
  });
  readonly tabs = computed<TabDef[]>(() => [
    { key: "files", icon: "folder", label: "Files" },
    { key: "inbox", icon: "bell", label: "Inbox", badge: this.pendingCount() },
    { key: "git", icon: "git", label: "Git" },
  ]);
}
