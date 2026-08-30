import {
  ChangeDetectionStrategy,
  Component,
  inject,
  signal,
} from "@angular/core";
import { Agent, MenuItem, Tab } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { CommandRegistryService } from "../commands/command-registry.service";
import { kbdLabel } from "../commands/fuzzy";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { SettingsStore } from "../settings/settings.store";
import { DragService } from "../shared/drag.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { treeAgentIds } from "../workspace/pane-model";
import { TabCloseGuardService } from "../workspace/tab-close-guard.service";
import { LogoComponent } from "./logo.component";
import { NotificationCenterComponent } from "./notification-center.component";
import { WindowControlsComponent } from "./window-controls.component";
import { VersionBadgeComponent } from "../shared/version-badge.component";
import { TicketsStore } from "../stores/tickets.store";
import { KjButtonComponent, KjKbdComponent } from "@kouji-ui/components";
import { mix } from "../utils";

@Component({
  selector: "app-top-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [LogoComponent, IconComponent, StatusDotComponent, NotificationCenterComponent, WindowControlsComponent, VersionBadgeComponent, KjButtonComponent, KjKbdComponent],
  template: `
    <header
      data-tauri-drag-region
      style="display:flex;align-items:stretch;background:var(--panel);border-bottom:1px solid var(--hair);height:var(--topbar-h);position:relative;z-index:5;min-width:0"
    >
      <!-- brand (also a window drag handle). Pinned to the OPEN sidebar width
           (--sidebar-w) so it lines up with the sidebar column and keeps that
           width even when the sidebar is collapsed to the compact rail. -->
      <div data-tauri-drag-region style="display:flex;align-items:center;gap:var(--sp-5);padding:0 var(--sp-6);flex:none;width:var(--sidebar-w);box-sizing:border-box">
        <app-logo style="pointer-events:none" />
        <div style="display:flex;flex-direction:column;line-height:1.12;pointer-events:none">
          <span class="disp" style="display:flex;align-items:center;gap:var(--sp-4);font-size:var(--fs-lg);font-weight:var(--fw-medium);letter-spacing:0.005em">
            <span><span style="color:var(--ui-ink)">O</span>rrery</span>
            <!-- the version pill is a tag, not a button (design/app.html:4228
                 renders a clickable <span>): a kj-button host dragged in the
                 whole button recipe — primary ground, control height, hover
                 lift — for what is a label that happens to open the changelog. -->
            <app-version-badge
              style="pointer-events:auto;cursor:pointer"
              (click)="settings.openWhatsNew()"
            />
          </span>
          <!-- design/app.html:4354 — 9.5px, the quietest line in the chrome -->
          <span class="trunc" style="font-size:var(--fs-micro);color:var(--ink-3);letter-spacing:0.04em">
            {{ projects.all().length }} projects · {{ runtime.agents().length }} agents
          </span>
        </div>
      </div>

      <div class="vdiv"></div>

      <!-- tabs (empty area drags the window). Overflowing tabs scroll sideways —
           the scrollbar is hidden (this is the titlebar) and the wheel pans. -->
      <div class="tab-strip scroll-hide" data-tauri-drag-region (wheel)="onTabWheel($event)" style="display:flex;align-items:stretch;flex:1;min-width:0;overflow-x:auto">
        @for (tab of ui.tabs(); track tab.id) {
          @let isOrch = tab.kind === 'orchestrator';
          @let active = ui.activeTab() === tab.id;
          @let ids = tabAgentIds(tab);
          @let tas = tabAgents(ids);
          @let isGroup = ids.length > 1;
          @let dz = drop()?.id === tab.id ? drop()!.zone : null;
          @let proj = !isGroup && tas[0] ? projects.projectOf(tas[0].projectId) : null;
          <div
            (click)="ui.selectTab(tab.id)"
            [draggable]="!isOrch"
            (dragstart)="onDragStart($event, tab)"
            (dragend)="onDragEnd()"
            (dragover)="onDragOver($event, tab)"
            (dragleave)="onDragLeave(tab)"
            (drop)="onDrop($event, tab)"
            (contextmenu)="onTabContext($event, tab)"
            [style.opacity]="dragId() === tab.id ? 0.45 : 1"
            [style.box-shadow]="dz === 'merge' ? 'inset 0 0 0 2px var(--ui-line)' : null"
            [style.background]="active ? 'var(--panel-2)' : (isOrch ? 'var(--panel)' : 'transparent')"
            [style.color]="active ? 'var(--ink)' : 'var(--ink-3)'"
            [style.position]="isOrch ? 'sticky' : 'relative'"
            [style.left]="isOrch ? '0' : null"
            [style.z-index]="isOrch ? 2 : null"
            style="display:flex;align-items:center;gap:var(--sp-4);padding:0 var(--sp-6);cursor:pointer;white-space:nowrap;flex:none;border-right:1px solid var(--hair)"
          >
            @if (active) {
              <!-- design/app.html: 2px --ui-ind indicator pinned to the tab's top edge -->
              <span style="position:absolute;left:0;right:0;top:0;height:var(--sp-1);background:var(--ui-ind)"></span>
            }
            @if (dz === 'before') {
              <span style="position:absolute;left:-1px;top:4px;bottom:4px;width:3px;border-radius:2px;background:var(--ui-fill)"></span>
            }
            @if (dz === 'after') {
              <span style="position:absolute;right:-1px;top:4px;bottom:4px;width:3px;border-radius:2px;background:var(--ui-fill)"></span>
            }
            @if (dz === 'merge') {
              <span style="position:absolute;inset:0;background:var(--ui-sel);pointer-events:none"></span>
            }

            @if (isOrch) {
              <app-icon name="layers" size="sm" [color]="active ? 'var(--ui-ink)' : null" />
              <span>Orchestrator</span>
            } @else if (tab.kind === 'backlog') {
              <app-icon name="layers" size="sm" [color]="active ? 'var(--ui-ink)' : null" />
              <span>Backlog</span>
            } @else if (tab.kind === 'ticket') {
              <app-icon name="file" size="sm" [color]="active ? 'var(--ui-ink)' : null" />
              <span class="trunc" style="max-width: round(calc(140px * var(--density)), 1px)">{{ ticketTabLabel(tab) }}</span>
            } @else if (tab.kind === 'project') {
              <!-- v2 project tab: project chip where an agent tab has a status dot -->
              @let pp = projects.projectOf(tab.projectId ?? '');
              @if (pp) {
                <span [style.background]="mixc(pp.color, 82)" [style.border]="'1px solid ' + mixc(pp.color, 62)" style="width:var(--sp-7);height:var(--sp-7);flex:none;border-radius:4px;display:grid;place-items:center">
                  <app-icon [name]="pp.icon" size="sm" [color]="pp.color" />
                </span>
              }
              <span>{{ pp?.name ?? 'project' }}</span>
              <span class="chip tnum" style="padding:0 var(--sp-3)">{{ pp?.branch ?? 'main' }}</span>
            } @else if (isGroup) {
              <app-icon name="columns" size="sm" [color]="active ? 'var(--ui-ink)' : 'var(--ink-3)'" />
              <span style="display:flex;gap:var(--sp-1)">
                @for (a of tas.slice(0, 3); track a.id) { <app-status-dot [status]="a.status" /> }
              </span>
              <span>{{ tas[0]?.name }} <span style="color:var(--ink-4)">+{{ ids.length - 1 }}</span></span>
            } @else {
              <app-status-dot [status]="tas[0] ? tas[0].status : 'idle'" />
              @if (proj) {
                <span [style.background]="proj.color" [title]="proj.name" style="width:var(--sp-3);height:var(--sp-3);border-radius:2px;flex:none"></span>
              }
              <span>{{ tas[0] ? tas[0].name : '…' }}</span>
            }

            @if (!isOrch && tab.kind !== 'backlog') {
              <kj-button kjSize="icon"
                kjVariant="ghost"
                (click)="closeTab($event, tab.id)"
                class="tab-x"
                kjAriaLabel="Close tab"
                style="display:flex;margin-left:var(--sp-1);--kj-button-padding-x:1px;--kj-button-padding-y:1px;--kj-button-height:auto;--kj-button-radius:3px"
              >
                <app-icon name="x" size="sm" />
              </kj-button>
            }
          </div>
        }
        @if (agentTabCount() >= 2) {
          <div style="display:flex;align-items:center;gap:var(--sp-3);padding:0 var(--sp-6);color:var(--ink-4);white-space:nowrap">
            <app-icon size="md" name="columns" />drag a tab onto another to tile them
          </div>
        }
      </div>

      <!-- Search Everywhere entry point (design: topbar.jsx "nav stack + search
           everywhere"). Sits at the right end of the tab area, pinned outside the
           scrolling strip so overflowing tabs never push it away. -->
      <kj-button class="tb-search" kjVariant="outline" (click)="commands.open('search')" [title]="'Search Everywhere · ' + searchKbd" kjAriaLabel="Search Everywhere"
        style="display:flex;align-items:center;padding:0 var(--sp-6);flex:none">
        <app-icon name="search" size="sm" />
        @for (k of searchKeys; track $index) {
          <kj-kbd>{{ k }}</kj-kbd>
        }
      </kj-button>

      <div class="vdiv"></div>

      <!-- right group: right-aligned to the window edge (v2 — the right panel
           is gone, so nothing mirrors this cluster's width anymore) -->
      <div style="display:flex;align-items:stretch;flex:none;margin-left:auto">
        <!-- actions: notifications, then a factorized segmented pill
             (run/pause · theme · settings) — one shared border, icon-only -->
        <div style="display:flex;align-items:center;gap:var(--sp-4);padding:0 var(--sp-6);flex:none">
          @let running = agentActions.anyRunning();
          <app-notification-center />
          <!-- Plain div, not <kj-button-group>: the group pulls every button
               after the first by -1px (its own segmented seam), which slides
               the segment ON TOP of the design's <span class="pill-div">
               hairline. The design (app.html:4423) is a role="group" div, and
               .action-pill in styles.css already draws the shared border. -->
          <div class="action-pill" role="group" aria-label="Workspace controls">
            <kj-button kjSize="icon"
              kjVariant="ghost"
              [class]="'pill-seg run' + (running ? ' running' : '')"
              (click)="agentActions.toggleRunAll()"
              [title]="running ? 'Pause all agents' : 'Run all agents'"
              [kjAriaLabel]="running ? 'Pause all' : 'Run all'"
            >
              <app-icon [name]="running ? 'pause' : 'play'" size="sm" />
            </kj-button>
            <span class="pill-div"></span>
            <kj-button kjSize="icon" kjVariant="ghost" class="pill-seg" (click)="ui.toggleTheme()" title="Toggle theme" kjAriaLabel="Toggle theme">
              <app-icon [name]="ui.tweaks().theme === 'dark' ? 'sun' : 'moon'" size="sm" />
            </kj-button>
            <span class="pill-div"></span>
            <kj-button kjVariant="ghost" class="pill-seg tb-settings" (click)="settings.openModal()" title="Settings" kjAriaLabel="Settings">
              <app-icon name="settings" size="sm" />
              @if (settings.updateKnown()) { <span class="tb-upd-dot" title="Update available"></span> }
            </kj-button>
          </div>
        </div>

        <!-- window controls (borderless titlebar) -->
        <div class="vdiv"></div>
        <app-window-controls />
      </div>
    </header>

    <!-- unsaved-changes guard for closing a whole workspace tab -->
    @if (closeGuard.pending(); as p) {
      <div class="scrim tcg-scrim" (mousedown)="closeGuard.cancel()">
        <div class="popover tcg-card rise" (mousedown)="$event.stopPropagation()">
          <div class="tcg-title">Unsaved changes</div>
          @for (f of p.files.slice(0, 6); track f.agentId + ':' + f.path) {
            <div class="tcg-path">{{ f.path }}</div>
          }
          @if (p.files.length > 6) {
            <div class="tcg-path">…and {{ p.files.length - 6 }} more</div>
          }
          <small class="tcg-note">Closing this tab discards edits that were never saved.</small>
          <div class="tcg-actions">
            <kj-button kjVariant="outline" (click)="closeGuard.cancel()">Cancel</kj-button>
            <kj-button kjVariant="danger" style="display:flex;margin-left:auto" (click)="closeGuard.discardAndClose()">Discard all</kj-button>
            <kj-button kjVariant="default" (click)="closeGuard.saveAndClose()">Save all & close</kj-button>
          </div>
        </div>
      </div>
    }
  `,
  styles: [
    `
      /* NOTE there is no .tab-x ink rule, and there cannot be one: --kj-button-fg
         is declared by the variant ON kouji's inner .kj-button, so a value set on
         the <kj-button> host (inline or by class) only INHERITS and loses. This
         component's styles are emulated-scoped and cannot reach that inner node
         either. kjVariant=ghost is what supplies the muted rest ink and the full-ink
         hover the design (app.html:4387) asks for — the old inline
         --kj-button-fg:var(--ink-4) was dead for exactly this reason, which is
         why the close × was painting kouji's default accent square. */
      /* box + backdrop come from the shared .scrim / .popover recipes; only
         the stacking level and this card's metrics are local. */
      .tcg-scrim {
        z-index: 80;
      }
      .tcg-card {
        padding: var(--sp-6);
        min-width: round(calc(320px * var(--density)), 1px);
        max-width: round(calc(460px * var(--density)), 1px);
      }
      .tcg-title {
        color: var(--ink);
        font-weight: var(--fw-medium);
      }
      .tcg-path {
        margin-top: var(--sp-2);
        font-family: var(--font-mono);
        color: var(--ink-2);
        overflow-wrap: anywhere;
      }
      /* ink + size + line-height come from the global <small> rule */
      .tcg-note {
        margin-top: var(--sp-3);
      }
      .tcg-actions {
        display: flex;
        gap: var(--sp-3);
        margin-top: var(--sp-5);
      }
      /* pending-update dot on the settings gear (mirrors the in-modal nav dot) */
      .tb-upd-dot {
        position: absolute; top: 3px; right: 3px; width: var(--sp-3); height: var(--sp-3);
        border-radius: 50%; background: var(--set-amber, var(--sem-attn));
        box-shadow: 0 0 7px -1px var(--set-amber, var(--sem-attn));
      }
    `,
  ],
})
export class TopBarComponent {
  readonly ui = inject(UiStore);
  readonly settings = inject(SettingsStore);
  readonly runtime = inject(AgentRuntimeService);
  readonly projects = inject(ProjectActionsService);
  readonly agentActions = inject(AgentActionsService);
  readonly tickets = inject(TicketsStore);
  readonly commands = inject(CommandRegistryService);
  readonly closeGuard = inject(TabCloseGuardService);
  private readonly drag = inject(DragService);

  /** Platform-aware chip label for the Search Everywhere button ("Ctrl+K" / ⌘K). */
  readonly searchKbd = kbdLabel("Ctrl+k");
  /** One <kj-kbd> per key — kouji renders each key as its own chip and puts
   *  any separator between them, rather than one chip holding "Shift Shift". */
  readonly searchKeys = this.searchKbd.split(/\s+/).filter(Boolean);
  readonly mixc = mix;

  // tab drag state: which tab is being dragged + the live drop zone on a target.
  readonly dragId = signal<string | null>(null);
  readonly drop = signal<{ id: string; zone: "merge" | "before" | "after" } | null>(null);

  /** Vertical wheel pans the tab strip sideways (trackpad deltaX already works natively). */
  onTabWheel(e: WheelEvent) {
    if (e.ctrlKey) return; // don't hijack zoom gestures
    const el = e.currentTarget as HTMLElement;
    if (el.scrollWidth <= el.clientWidth) return;
    if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      el.scrollLeft += e.deltaY;
      e.preventDefault();
    }
  }

  tabAgentIds(tab: Tab): string[] {
    if (tab.kind === "orchestrator") return [];
    const root = this.ui.paneRoots()[tab.id];
    return root ? treeAgentIds(root) : [];
  }
  tabAgents(ids: string[]): Agent[] {
    const all = this.runtime.agents();
    return ids.map((id) => all.find((a) => a.id === id)).filter((a): a is Agent => !!a);
  }
  agentTabCount(): number {
    return this.ui.tabs().filter((t) => t.kind !== "orchestrator").length;
  }
  ticketTabLabel(tab: Tab): string {
    if (tab.ticketId === "draft") return "New ticket";
    const tk = tab.ticketId ? this.tickets.byId(tab.ticketId) : undefined;
    return tk?.title ? tk.title.slice(0, 30) + (tk.title.length > 30 ? "…" : "") : "Ticket";
  }

  // ----- tab drag-and-drop (group / reorder), + dropping a sidebar agent on a tab -----
  onDragStart(e: DragEvent, tab: Tab) {
    if (tab.kind !== "agent") return;
    this.dragId.set(tab.id);
    this.drag.start({ kind: "tab", tabId: tab.id, agentId: this.tabAgentIds(tab)[0] ?? null });
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
  }
  onDragEnd() {
    this.dragId.set(null);
    this.drop.set(null);
    this.drag.end();
  }
  onDragOver(e: DragEvent, tab: Tab) {
    const d = this.drag.payload();
    if (!d || tab.kind === "orchestrator") return;
    if (d.kind === "agent") {
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
      this.drop.set({ id: tab.id, zone: "merge" });
      return;
    }
    // tab drag → before / merge / after by cursor x
    if (d.tabId === tab.id) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "move";
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = e.clientX - r.left;
    const zone = x < r.width * 0.28 ? "before" : x > r.width * 0.72 ? "after" : "merge";
    this.drop.set({ id: tab.id, zone });
  }
  onDragLeave(tab: Tab) {
    this.drop.update((cur) => (cur && cur.id === tab.id ? null : cur));
  }
  onDrop(e: DragEvent, tab: Tab) {
    e.preventDefault();
    const d = this.drag.payload();
    if (!d) {
      this.drop.set(null);
      return;
    }
    if (d.kind === "agent" && d.agentId) {
      this.ui.addAgentToTab(d.agentId, tab.id);
    } else if (d.kind === "tab" && d.tabId) {
      const dz = this.drop();
      if (dz && dz.id === tab.id) {
        if (dz.zone === "merge") this.ui.mergeTabs(d.tabId, tab.id);
        else this.ui.reorderTab(d.tabId, tab.id, dz.zone === "before");
      }
    }
    this.onDragEnd();
  }

  onTabContext(e: MouseEvent, tab: Tab) {
    if (tab.kind === "orchestrator") return;
    const ids = this.tabAgentIds(tab);
    if (ids.length <= 1) {
      if (ids[0]) this.ui.openMenu(e, this.agentActions.agentMenu(ids[0]));
      return;
    }
    // grouped tab menu
    const items: MenuItem[] = this.tabAgents(ids).map((a) => ({
      label: "Detach " + a.name,
      icon: "enter",
      onClick: () => this.ui.detachAgent(tab.id, a.id),
    }));
    items.push({ sep: true });
    items.push({ label: "Ungroup all", icon: "columns", accent: "var(--ui-ink)", onClick: () => this.ui.ungroupTab(tab.id) });
    items.push({ label: "Close group", icon: "x", danger: true, onClick: () => this.closeGuard.requestClose(tab.id) });
    this.ui.openMenu(e, items);
  }

  closeTab(e: MouseEvent, id: string) {
    e.stopPropagation();
    this.closeGuard.requestClose(id);
  }
}
