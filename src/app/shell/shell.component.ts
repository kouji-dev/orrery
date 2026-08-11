import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { InterestService } from '../agents/interest.service';
import { CommandRegistryService } from '../commands/command-registry.service';
import { CommandOverlaysComponent } from '../commands/overlays.component';
import { ContextMenuComponent } from '../context-menu/context-menu.component';
import { AddProjectModalComponent } from '../modals/add-project-modal.component';
import { DeleteWorktreeModalComponent } from '../modals/delete-worktree-modal.component';
import { SettingsModalComponent } from '../modals/settings-modal.component';
import { SpawnModalComponent } from '../modals/spawn-modal.component';
import { UpdateToastComponent } from '../modals/update-toast.component';
import { WhatsNewModalComponent } from '../modals/whats-new-modal.component';
import { SettingsStore } from '../settings/settings.store';
import { UpdateWatcherService } from '../updater/update-watcher.service';
import { FileDropService } from '../shared/file-drop.service';
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
import { ToolWindowComponent } from '../tool-window/tool-window.component';
import { ToolWindowStore } from '../tool-window/tool-window.store';

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
    DeleteWorktreeModalComponent,
    SettingsModalComponent,
    UpdateToastComponent,
    WhatsNewModalComponent,
    ContextMenuComponent,
    TweaksPanelComponent,
    DevPanelComponent,
    CommandOverlaysComponent,
    ToolWindowComponent,
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

        <!-- center column: the active tab's content with the bottom tool
             window (IntelliJ-style dock) docked under it when open -->
        <div class="center-col">
          @switch (ui.activeTabKind()) {
            @case ('orchestrator') { <app-overview /> }
            @case ('backlog') { <app-backlog /> }
            @case ('ticket') {
              <app-ticket-page [ticketId]="ui.activeTicketId()!" />
            }
            @default { <app-pane-manager [tabId]="ui.activeTab()" /> }
          }
          @if (toolWindow.panel()) {
            <app-tool-window />
          }
        </div>

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
    @if (ui.deletingWorktree(); as deletingId) {
      <app-delete-worktree-modal [agentId]="deletingId" />
    }
    @if (settings.open()) {
      <app-settings-modal />
    }
    <app-whats-new-modal />
    <app-update-toast />
    <app-command-overlays />
    <app-context-menu />
    <div class="anchor-rail">
      <app-tweaks-panel />
      <app-dev-panel />
    </div>
  `,
  styles: [
    `
      /* The workspace's middle column: the active tab content in the 1fr row,
         the bottom tool window (when open) in the auto row underneath. The tab
         content components are display:contents hosts, so their roots become
         the grid children directly. */
      .center-col {
        display: grid;
        grid-template-rows: minmax(0, 1fr) auto;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
      }

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
  readonly toolWindow = inject(ToolWindowStore);

  constructor() {
    // Route OS file drops (absolute paths) into the terminal / prompt under the
    // drop point. Started here — the shell is the app's root UI surface.
    inject(FileDropService).start();
    // Global keybindings (command palette, Search Everywhere, recent files…)
    // dispatch from the command registry — one window-level listener.
    inject(CommandRegistryService).start();
    // A0.2: publish the PTY interest set (stream/digest/none per agent) to the
    // backend whenever the visible surfaces change — tab switch, pane layout,
    // overview card visibility, window hidden/shown.
    inject(InterestService).start();
    // Re-check for releases while the app RUNS (poll + on focus). The startup
    // check happens once on the splash screen, so without this an instance that
    // booted before a release shipped would only notice it after a restart.
    inject(UpdateWatcherService).start();
  }
}
