import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { OrchestraStore } from "../orchestra.store";
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
    @let scope = store.activeAgent();
    <aside style="display:flex;flex-direction:column;min-height:0;background:var(--panel);border-left:1px solid var(--hair)">
      <!-- scope header -->
      <div style="display:flex;align-items:center;gap:8px;padding:0 12px;height:38px;border-bottom:1px solid var(--hair)">
        @if (scope) {
          <app-status-dot [status]="scope.status" />
          <span style="font-size:11.5px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ scope.name }}</span>
          @if (project()) {
            <span class="chip" [style.color]="project()!.color" [style.border-color]="mix(project()!.color, 65)" style="font-size:9px;padding:1px 6px">{{ project()!.name }}</span>
          }
        } @else {
          <app-icon name="layers" size="sm" color="var(--accent)" />
          <span style="font-size:11.5px;font-weight:600;color:var(--ink-2)">All projects</span>
        }
      </div>

      <!-- tab bar -->
      <div style="display:flex;align-items:center;padding:0 6px;border-bottom:1px solid var(--hair);height:36px">
        @for (t of tabs(); track t.key) {
          @let on = tab() === t.key;
          <button class="btn" (click)="tab.set(t.key)" [style.color]="on ? 'var(--ink)' : 'var(--ink-3)'" style="padding:7px 10px;border-radius:0;position:relative;flex:1;justify-content:center">
            @if (on) { <span style="position:absolute;left:8px;right:8px;bottom:0;height:2px;background:linear-gradient(90deg,var(--accent),var(--accent-2))"></span> }
            <app-icon [name]="t.icon" size="sm" [color]="on ? 'var(--accent)' : null" />
            <span style="font-size:11px">{{ t.label }}</span>
            @if (t.badge) {
              <span class="chip tnum" [style.border-color]="mix('var(--accent)', 60)" style="font-size:9px;padding:0 5px;color:var(--accent)">{{ t.badge }}</span>
            }
          </button>
        }
      </div>

      @switch (tab()) {
        @case ('files') { <app-files-tab [agent]="scope" [project]="project()" /> }
        @case ('inbox') { <app-inbox-tab [scopeAgent]="scope" /> }
        @case ('git') { <app-git-tab [agent]="scope" [project]="project()" /> }
      }
    </aside>
  `,
})
export class RightPanelComponent {
  readonly store = inject(OrchestraStore);
  readonly tab = signal<string>("files");
  readonly mix = mix;

  readonly project = computed(() => {
    const sa = this.store.activeAgent();
    return sa ? this.store.projectOf(sa.projectId) : undefined;
  });
  readonly pendingCount = computed(() => {
    const sa = this.store.activeAgent();
    return sa
      ? (sa.pending || []).length
      : this.store.agents().reduce((s, a) => s + (a.pending ? a.pending.length : 0), 0);
  });
  readonly tabs = computed<TabDef[]>(() => [
    { key: "files", icon: "folder", label: "Files" },
    { key: "inbox", icon: "bell", label: "Inbox", badge: this.pendingCount() },
    { key: "git", icon: "git", label: "Git" },
  ]);
}
