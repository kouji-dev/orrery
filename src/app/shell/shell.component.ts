import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ContextMenuComponent } from '../context-menu/context-menu.component';
import { AddProjectModalComponent } from '../modals/add-project-modal.component';
import { SettingsModalComponent } from '../modals/settings-modal.component';
import { SpawnModalComponent } from '../modals/spawn-modal.component';
import { UpdateToastComponent } from '../modals/update-toast.component';
import { WhatsNewModalComponent } from '../modals/whats-new-modal.component';
import { SettingsStore } from '../settings/settings.store';
import { UiStore } from '../ui/ui.store';
import { OverviewComponent } from '../overview/overview.component';
import { BacklogComponent } from '../backlog/backlog.component';
import { RightPanelComponent } from '../right-panel/right-panel.component';
import { SidebarComponent } from '../sidebar/sidebar.component';
import { CompactRailComponent } from '../sidebar/compact-rail.component';
import { StatusBarComponent } from '../status-bar/status-bar.component';
import { TopBarComponent } from '../top-bar/top-bar.component';
import { TweaksPanelComponent } from '../tweaks/tweaks-panel.component';
import { DevPanelComponent } from '../dev-tools/dev-panel.component';
import { PaneManagerComponent } from '../workspace/pane-manager.component';
import { TicketPageComponent } from '../backlog/ticket-page.component';

declare const ngDevMode: boolean | undefined;

@Component({
  selector: 'app-shell',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    TopBarComponent,
    SidebarComponent,
    CompactRailComponent,
    OverviewComponent,
    BacklogComponent,
    PaneManagerComponent,
    TicketPageComponent,
    RightPanelComponent,
    StatusBarComponent,
    SpawnModalComponent,
    AddProjectModalComponent,
    SettingsModalComponent,
    UpdateToastComponent,
    WhatsNewModalComponent,
    ContextMenuComponent,
    TweaksPanelComponent,
    DevPanelComponent,
  ],
  template: `
    <div class="bg-texture"></div>
    <div class="bg-glow"></div>
    <div class="shell">
      <app-top-bar />

      <div
        class="workspace"
        [class.no-right]="!ui.tweaks().rightPanel"
        [class.compact]="ui.sidebarCompact()"
      >
        @if (ui.sidebarCompact()) {
          <app-compact-rail />
        } @else {
          <app-sidebar />
        }

        @switch (ui.activeTabKind()) {
          @case ('orchestrator') { <app-overview /> }
          @case ('backlog') { <app-backlog /> }
          @case ('ticket') {
            <app-ticket-page [ticketId]="ui.activeTicketId()!" />
          }
          @default { <app-pane-manager [tabId]="ui.activeTab()" /> }
        }

        @if (ui.tweaks().rightPanel) {
          <app-right-panel />
        }
      </div>

      <app-status-bar />
    </div>

    @if (ui.spawning()) {
      <app-spawn-modal />
    }
    @if (ui.addingProject()) {
      <app-add-project-modal />
    }
    @if (settings.open()) {
      <app-settings-modal />
    }
    <app-whats-new-modal />
    <app-update-toast />
    <app-context-menu />
    <div class="anchor-rail">
      <app-tweaks-panel />
      <app-dev-panel />
    </div>
  `,
  styles: [
    `
      /* One fixed, bottom-right, vertical flex container for the floating action
         buttons (Tweaks + DevConsole/Perf). The DevConsole renders in EVERY build
         with NO tier trimming — feed, row expand, and the Resources tab show in
         prod too (aggregates live in-memory and the in-app panel is their only
         surface). The Perf FAB renders last, so it sits at the bottom (the
         corner) and Tweaks stacks above it. */
      .anchor-rail {
        position: fixed;
        right: 18px;
        bottom: 36px;
        z-index: 90;
        display: flex;
        flex-direction: column;
        align-items: flex-end;
        gap: var(--sp-6);
        pointer-events: none;
      }
      .anchor-rail > * {
        pointer-events: auto;
      }
    `,
  ],
})
export class ShellComponent {
  readonly ui = inject(UiStore);
  readonly settings = inject(SettingsStore);
}
