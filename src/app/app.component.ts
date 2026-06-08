import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { ContextMenuComponent } from "./context-menu/context-menu.component";
import { AddProjectModalComponent } from "./modals/add-project-modal.component";
import { SpawnModalComponent } from "./modals/spawn-modal.component";
import { UiStore } from "./ui/ui.store";
import { OverviewComponent } from "./overview/overview.component";
import { RightPanelComponent } from "./right-panel/right-panel.component";
import { SidebarComponent } from "./sidebar/sidebar.component";
import { CompactRailComponent } from "./sidebar/compact-rail.component";
import { StatusBarComponent } from "./status-bar/status-bar.component";
import { TopBarComponent } from "./top-bar/top-bar.component";
import { TweaksPanelComponent } from "./tweaks/tweaks-panel.component";
import { DevPanelComponent } from "./dev-tools/dev-panel.component";
import { PaneManagerComponent } from "./workspace/pane-manager.component";

@Component({
  selector: "app-root",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    TopBarComponent,
    SidebarComponent,
    CompactRailComponent,
    OverviewComponent,
    PaneManagerComponent,
    RightPanelComponent,
    StatusBarComponent,
    SpawnModalComponent,
    AddProjectModalComponent,
    ContextMenuComponent,
    TweaksPanelComponent,
    DevPanelComponent,
  ],
  template: `
    <div class="bg-texture"></div>
    <div class="bg-glow"></div>
    <div class="shell">
      <app-top-bar />

      <div class="workspace" [class.no-right]="!ui.tweaks().rightPanel" [class.compact]="ui.sidebarCompact()">
        @if (ui.sidebarCompact()) {
          <app-compact-rail />
        } @else {
          <app-sidebar />
        }

        @if (ui.activeTab() === 'orchestrator') {
          <app-overview />
        } @else {
          <app-pane-manager [tabId]="ui.activeTab()" />
        }

        @if (ui.tweaks().rightPanel) {
          <app-right-panel />
        }
      </div>

      <app-status-bar />
    </div>

    @if (ui.spawning()) { <app-spawn-modal /> }
    @if (ui.addingProject()) { <app-add-project-modal /> }
    <app-context-menu />
    <app-tweaks-panel />
    <app-dev-panel />
  `,
})
export class AppComponent {
  readonly ui = inject(UiStore);
}
