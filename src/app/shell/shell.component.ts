import { ChangeDetectionStrategy, Component, effect, inject, Type, untracked } from '@angular/core';
import { KjDialogRef, KjDialogService } from '@kouji-ui/components';
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
import { SidebarComponent } from '../sidebar/sidebar.component';
import { CompactRailComponent } from '../sidebar/compact-rail.component';
import { StatusBarComponent } from '../status-bar/status-bar.component';
import { TopBarComponent } from '../top-bar/top-bar.component';
import { TweaksPanelComponent } from '../tweaks/tweaks-panel.component';
import { DevPanelComponent } from '../dev-tools/dev-panel.component';
import { PaneManagerComponent } from '../workspace/pane-manager.component';
import { TicketPageComponent } from '../backlog/ticket-page.component';
import { ToolWindowComponent } from '../tool-window/tool-window.component';
import { GraphStripComponent } from '../tool-window/graph-strip.component';
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
    StatusBarComponent,
    UpdateToastComponent,
    ContextMenuComponent,
    TweaksPanelComponent,
    DevPanelComponent,
    CommandOverlaysComponent,
    ToolWindowComponent,
    GraphStripComponent,
  ],
  template: `
    <div class="shell">
      <app-top-bar />

      <div
        class="workspace"
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
          } @else {
            <!-- v2: collapsed graph strip — history stays one click away -->
            <app-graph-strip />
          }
        </div>
      </div>

      <app-status-bar />
    </div>

    <!-- The five modals are service-launched (see bindModal) — they mount into
         kj's body-level overlay root, not here. -->
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
  private readonly dialog = inject(KjDialogService);

  constructor() {
    // Modals run on kj's dialog service — it owns the scrim, the focus trap,
    // the scroll lock, Esc and outside-click. The stores stay the single source
    // of truth for what's open (every existing call site keeps working).
    this.bindModal(() => this.ui.spawning(), SpawnModalComponent);
    this.bindModal(() => this.ui.addingProject(), AddProjectModalComponent);
    this.bindModal(() => this.ui.deletingWorktree(), DeleteWorktreeModalComponent);
    this.bindModal(() => this.settings.open(), SettingsModalComponent);
    this.bindModal(() => this.settings.whatsNewOpen(), WhatsNewModalComponent);

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

  /**
   * Mirrors a store flag onto a KjDialog. Opening is one-way from the store;
   * closing works from either side — a store close dismisses the overlay here,
   * and an overlay close (Esc, outside-click) destroys the modal component,
   * whose own onDestroy clears the flag. So the two can never drift.
   */
  private bindModal<T>(open: () => unknown, component: Type<T>): void {
    let ref: KjDialogRef<T> | null = null;
    effect(() => {
      // Read the flag INSIDE the reactive context so the effect tracks it...
      const shouldBeOpen = !!open();
      // ...but open/close OUTSIDE it. Constructing the dialog component runs
      // its constructor, and every one of these modals calls effect() there —
      // which throws NG0602 ("effect() cannot be called from within a reactive
      // context") when it happens inside this effect. The dialog then mounted
      // with none of its own effects wired, so it rendered as an empty shell.
      untracked(() => {
        if (shouldBeOpen) {
          ref ??= this.dialog.open<T>(component);
        } else if (ref) {
          const closing = ref;
          ref = null;
          closing.close();
        }
      });
    });
  }
}
