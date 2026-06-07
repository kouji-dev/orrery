import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { ContextMenuComponent } from "./orchestra/context-menu/context-menu.component";
import { AddProjectModalComponent } from "./orchestra/modals/add-project-modal.component";
import { SpawnModalComponent } from "./orchestra/modals/spawn-modal.component";
import { UiStore } from "./orchestra/ui/ui.store";
import { OverviewComponent } from "./orchestra/overview/overview.component";
import { RightPanelComponent } from "./orchestra/right-panel/right-panel.component";
import { SidebarComponent } from "./orchestra/sidebar/sidebar.component";
import { CompactRailComponent } from "./orchestra/sidebar/compact-rail.component";
import { StatusBarComponent } from "./orchestra/status-bar/status-bar.component";
import { TopBarComponent } from "./orchestra/top-bar/top-bar.component";
import { TweaksPanelComponent } from "./orchestra/tweaks/tweaks-panel.component";
import { DevPanelComponent } from "./orchestra/dev-tools/dev-panel.component";
import { PaneManagerComponent } from "./orchestra/workspace/pane-manager.component";

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
