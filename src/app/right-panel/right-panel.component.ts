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
import {
  KjBadgeComponent,
  KjTabComponent,
  KjTabListComponent,
  KjTabsComponent,
  KjEmptyStateComponent,
  KjEmptyStateDescriptionComponent,
  KjEmptyStateIconComponent,
  KjEmptyStateTitleComponent,
} from "@kouji-ui/components";

interface TabDef {
  key: string;
  icon: string;
  label: string;
  badge?: number;
}

@Component({
  selector: "app-right-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    StatusDotComponent,
    FilesTabComponent,
    InboxTabComponent,
    GitTabComponent,
    KjBadgeComponent,
    KjTabsComponent,
    KjTabListComponent,
    KjTabComponent,
    KjEmptyStateComponent,
    KjEmptyStateIconComponent,
    KjEmptyStateTitleComponent,
    KjEmptyStateDescriptionComponent,
  ],
  template: `
    @let scope = runtime.activeAgent();
    <aside style="display:flex;flex-direction:column;min-height:0;min-width:0;overflow:hidden;background:var(--panel);border-left:1px solid var(--hair)">
      @if (scope) {
        <!-- agent header -->
        <div class="pane-head" style="height:38px">
          <app-status-dot [status]="scope.status" />
          <span class="trunc" style="font-weight:var(--fw-medium)">{{ scope.name }}</span>
          @if (project()) {
            <kj-badge [fg]="project()!.color" [style.border-color]="mix(project()!.color, 65)">{{ project()!.name }}</kj-badge>
          }
        </div>

        <!-- real tabs: role=tablist/tab/tabpanel, roving focus and aria-controls
             come from kouji. Panels stay in the @switch — each tab body is an
             expensive component and kj-tab-panel would keep all three alive. -->
        <kj-tabs class="rp-tabs" [value]="tab()" (valueChange)="tab.set($event)">
          <kj-tab-list>
            @for (t of tabs(); track t.key) {
              @let on = tab() === t.key;
              <kj-tab [value]="t.key">
                <app-icon [name]="t.icon" size="sm" [color]="on ? 'var(--ui-ink)' : null" />
                <span>{{ t.label }}</span>
                @if (t.badge) {
                  <kj-badge class="tnum">{{ t.badge }}</kj-badge>
                }
              </kj-tab>
            }
          </kj-tab-list>
        </kj-tabs>

        @switch (tab()) {
          @case ('files') { <app-files-tab [agent]="scope" [project]="project()" /> }
          @case ('inbox') { <app-inbox-tab [scopeAgent]="scope" /> }
          @case ('git') { <app-git-tab [agent]="scope" [project]="project()" /> }
        }
      } @else {
        <!-- empty: this panel is agent-scoped -->
        <div class="pane-empty pad">
          <kj-empty-state kjSize="sm">
            <kj-empty-state-icon><app-icon name="layers" size="lg" color="var(--hair-2)" /></kj-empty-state-icon>
            <kj-empty-state-title>No agent selected</kj-empty-state-title>
            <kj-empty-state-description>Open an agent to see its files, git &amp; inbox</kj-empty-state-description>
          </kj-empty-state>
        </div>
      }
    </aside>
  `,
  styles: [
    `
      /* design pane-tab badge (app.html:5518): one step down in type and a
         tighter gutter than the app's default chip. kouji declares the knobs
         ON .kj-badge, so a host-level value never reaches it. */
      .kj-tab-strip kj-badge ::ng-deep .kj-badge {
        --kj-badge-font-size: var(--fs-badge);
        --kj-badge-padding-x: var(--sp-2);
      }
    `,
  ],
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
