import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  OnDestroy,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { Agent, MenuItem, Tab } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { DragService } from "../shared/drag.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { treeAgentIds } from "../workspace/pane-model";
import { LogoComponent } from "./logo.component";
import { NotificationCenterComponent } from "./notification-center.component";
import { WindowControlsComponent } from "./window-controls.component";

@Component({
  selector: "app-top-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [LogoComponent, IconComponent, StatusDotComponent, NotificationCenterComponent, WindowControlsComponent],
  template: `
    <header
      data-tauri-drag-region
      style="display:flex;align-items:stretch;background:var(--panel);border-bottom:1px solid var(--hair);height:44px;position:relative;z-index:5"
    >
      <!-- brand (also a window drag handle). Pinned to the OPEN sidebar width
           (--sidebar-w) so it lines up with the sidebar column and keeps that
           width even when the sidebar is collapsed to the compact rail. -->
      <div data-tauri-drag-region style="display:flex;align-items:center;gap:11px;padding:0 14px;flex:none;width:var(--sidebar-w, 252px);box-sizing:border-box">
        <app-logo style="pointer-events:none" />
        <div style="display:flex;flex-direction:column;line-height:1.12;pointer-events:none">
          <span class="disp" style="font-size:15px;font-weight:600;letter-spacing:0.005em">
            <span style="color:var(--accent)">O</span>rrery
          </span>
          <span style="font-size:9.5px;color:var(--ink-3);letter-spacing:0.04em">
            {{ projects.all().length }} projects · {{ runtime.agents().length }} agents
          </span>
        </div>
      </div>

      <div class="vdiv"></div>

      <!-- tabs (empty area drags the window) -->
      <div data-tauri-drag-region style="display:flex;align-items:stretch;flex:1;min-width:0;overflow-x:auto">
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
            [style.box-shadow]="dz === 'merge' ? 'inset 0 0 0 2px color-mix(in oklch, var(--accent), transparent 35%)' : null"
            [style.background]="active ? 'var(--panel-2)' : (isOrch ? 'var(--panel)' : 'transparent')"
            [style.color]="active ? 'var(--ink)' : 'var(--ink-3)'"
            [style.position]="isOrch ? 'sticky' : 'relative'"
            [style.left]="isOrch ? '0' : null"
            [style.z-index]="isOrch ? 2 : null"
            style="display:flex;align-items:center;gap:8px;padding:0 13px;cursor:pointer;white-space:nowrap;flex:none;border-right:1px solid var(--hair)"
          >
            @if (active) {
              <span style="position:absolute;left:0;right:0;top:0;height:2px;background:linear-gradient(90deg,var(--accent),var(--accent-2))"></span>
            }
            @if (dz === 'before') {
              <span style="position:absolute;left:-1px;top:4px;bottom:4px;width:3px;border-radius:2px;background:var(--accent)"></span>
            }
            @if (dz === 'after') {
              <span style="position:absolute;right:-1px;top:4px;bottom:4px;width:3px;border-radius:2px;background:var(--accent)"></span>
            }
            @if (dz === 'merge') {
              <span style="position:absolute;inset:0;background:color-mix(in oklch, var(--accent), transparent 90%);pointer-events:none"></span>
            }

            @if (isOrch) {
              <app-icon name="layers" size="sm" [color]="active ? 'var(--accent)' : null" />
              <span style="font-size:12px">Orchestrator</span>
            } @else if (isGroup) {
              <app-icon name="columns" size="sm" [color]="active ? 'var(--accent)' : 'var(--ink-3)'" />
              <span style="display:flex;gap:2px">
                @for (a of tas.slice(0, 3); track a.id) { <app-status-dot [status]="a.status" /> }
              </span>
              <span style="font-size:12px">{{ tas[0]?.name }} <span style="color:var(--ink-4)">+{{ ids.length - 1 }}</span></span>
            } @else {
              <app-status-dot [status]="tas[0] ? tas[0].status : 'idle'" />
              @if (proj) {
                <span [style.background]="proj.color" [title]="proj.name" style="width:6px;height:6px;border-radius:2px;flex:none"></span>
              }
              <span style="font-size:12px">{{ tas[0] ? tas[0].name : tab.id }}</span>
            }

            @if (!isOrch) {
              <button
                (click)="closeTab($event, tab.id)"
                class="tab-x"
                style="background:transparent;border:none;color:var(--ink-4);cursor:pointer;display:flex;padding:1px;border-radius:3px;margin-left:2px"
              >
                <app-icon name="x" size="sm" />
              </button>
            }
          </div>
        }
        @if (agentTabCount() >= 2) {
          <div style="display:flex;align-items:center;gap:6px;padding:0 12px;color:var(--ink-4);font-size:10px;white-space:nowrap">
            <app-icon name="columns" size="sm" [px]="12" />drag a tab onto another to tile them
          </div>
        }
      </div>

      <div class="vdiv"></div>

      <!-- right group: its measured width is mirrored into --right-w (see below) so
           the right panel column lines up exactly under this cluster -->
      <div #rightGroup style="display:flex;align-items:stretch;flex:none">
        <!-- actions: Run all/Pause all (left), then notification + theme buttons -->
        <div style="display:flex;align-items:center;gap:8px;padding:0 12px;flex:none">
          @let running = agentActions.anyRunning();
          <button
            [class]="'btn ' + (running ? 'ghost-hair' : 'primary')"
            (click)="agentActions.toggleRunAll()"
            title="Pause / start every agent"
            style="height:25px;padding:0 10px;min-width:96px;justify-content:center"
          >
            <app-icon [name]="running ? 'pause' : 'play'" size="sm" />
            {{ running ? 'Pause all' : 'Run all' }}
          </button>
          <app-notification-center />
          <button class="btn ghost-hair" (click)="ui.toggleTheme()" title="Toggle theme" style="padding:5px 8px">
            <app-icon [name]="ui.tweaks().theme === 'dark' ? 'sun' : 'moon'" size="sm" />
          </button>
        </div>

        <!-- window controls (borderless titlebar) -->
        <div class="vdiv"></div>
        <app-window-controls />
      </div>
    </header>
  `,
  styles: [`.tab-x:hover { color: var(--ink) !important; }`],
})
export class TopBarComponent implements AfterViewInit, OnDestroy {
  readonly ui = inject(UiStore);
  readonly runtime = inject(AgentRuntimeService);
  readonly projects = inject(ProjectActionsService);
  readonly agentActions = inject(AgentActionsService);
  private readonly drag = inject(DragService);

  // tab drag state: which tab is being dragged + the live drop zone on a target.
  readonly dragId = signal<string | null>(null);
  readonly drop = signal<{ id: string; zone: "merge" | "before" | "after" } | null>(null);

  // The right-side action cluster (Run all + notification + theme + window
  // controls). Its live width is mirrored into the global --right-w so the right
  // panel column is always exactly as wide as this cluster — they read as one
  // column. A ResizeObserver keeps it correct across font swaps / label changes.
  private readonly rightGroup = viewChild<ElementRef<HTMLElement>>("rightGroup");
  private ro?: ResizeObserver;

  ngAfterViewInit() {
    const el = this.rightGroup()?.nativeElement;
    if (!el || typeof ResizeObserver === "undefined") return;
    const apply = () => {
      const w = Math.round(el.getBoundingClientRect().width);
      if (w > 0) document.documentElement.style.setProperty("--right-w", `${w}px`);
    };
    apply();
    this.ro = new ResizeObserver(apply);
    this.ro.observe(el);
  }

  ngOnDestroy() {
    this.ro?.disconnect();
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

  // ----- tab drag-and-drop (group / reorder), + dropping a sidebar agent on a tab -----
  onDragStart(e: DragEvent, tab: Tab) {
    if (tab.kind === "orchestrator") return;
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
    items.push({ label: "Ungroup all", icon: "columns", accent: "var(--accent)", onClick: () => this.ui.ungroupTab(tab.id) });
    items.push({ label: "Close group", icon: "x", danger: true, onClick: () => this.ui.closeTab(tab.id) });
    this.ui.openMenu(e, items);
  }

  closeTab(e: MouseEvent, id: string) {
    e.stopPropagation();
    this.ui.closeTab(id);
  }
}
