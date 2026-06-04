import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { ContextMenuComponent } from "./orchestra/context-menu/context-menu.component";
import { AddProjectModalComponent } from "./orchestra/modals/add-project-modal.component";
import { SpawnModalComponent } from "./orchestra/modals/spawn-modal.component";
import { OrchestraStore } from "./orchestra/orchestra.store";
import { OverviewComponent } from "./orchestra/overview/overview.component";
import { RightPanelComponent } from "./orchestra/right-panel/right-panel.component";
import { SidebarComponent } from "./orchestra/sidebar/sidebar.component";
import { StatusBarComponent } from "./orchestra/status-bar/status-bar.component";
import { TopBarComponent } from "./orchestra/top-bar/top-bar.component";
import { TweaksPanelComponent } from "./orchestra/tweaks/tweaks-panel.component";
import { WorkspaceComponent } from "./orchestra/workspace/workspace.component";

@Component({
  selector: "app-root",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    TopBarComponent,
    SidebarComponent,
    OverviewComponent,
    WorkspaceComponent,
    RightPanelComponent,
    StatusBarComponent,
    SpawnModalComponent,
    AddProjectModalComponent,
    ContextMenuComponent,
    TweaksPanelComponent,
  ],
  template: `
    <div class="bg-texture"></div>
    <div class="bg-glow"></div>
    <div class="shell">
      <app-top-bar />

      <div class="workspace" [class.no-right]="!store.tweaks().rightPanel">
        <app-sidebar />

        @if (store.activeTab() === 'orchestrator') {
          <app-overview />
        } @else if (store.activeAgent(); as ag) {
          <app-workspace [agent]="ag" [project]="store.projectOf(ag.projectId)" />
        } @else {
          <div style="display:grid;place-items:center;color:var(--ink-4)">agent not found</div>
        }

        @if (store.tweaks().rightPanel) {
          <app-right-panel />
        }
      </div>

      <app-status-bar />
    </div>

    @if (store.spawning()) { <app-spawn-modal /> }
    @if (store.addingProject()) { <app-add-project-modal /> }
    <app-context-menu />
    <app-tweaks-panel />
  `,
})
export class AppComponent {
  readonly store = inject(OrchestraStore);
  // keep a stable reference for templates
  readonly activeAgent = computed(() => this.store.activeAgent());
}
