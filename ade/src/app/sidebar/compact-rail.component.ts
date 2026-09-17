import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { Agent, Project } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { AgentActionsService } from "../agents/agent-actions.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { TicketsStore } from "../stores/tickets.store";
import { DragService } from "../shared/drag.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { mix } from "../utils";
import { KjButton } from "@kouji-ui/core";
import { KjDividerComponent, KjSpinnerComponent } from "@kouji-ui/components";

/**
 * Collapsed sidebar: a 54px rail of project icons. Hovering a project pops a
 * floating list of its agents (the same open / context-menu / spawn actions as
 * the full sidebar). The header "expand" button restores the full sidebar.
 */
@Component({
  selector: "app-compact-rail",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, StatusDotComponent, ToolBadgeComponent, KjButton, KjDividerComponent, KjSpinnerComponent],
  template: `
    <aside
      style="display:flex;flex-direction:column;align-items:center;min-height:0;width:54px;background:var(--panel);border-right:1px solid var(--hair);padding:var(--sp-4) 0;gap:var(--sp-2);position:relative"
    >
      <button kjButton class="rail-btn" (click)="ui.toggleSidebarCompact()" title="Expand sidebar" style="margin-bottom:var(--sp-1)">
        <app-icon name="panelLeft" size="sm" color="var(--ui-ink)" />
      </button>
      <kj-divider style="--kj-divider-color:var(--hair);--kj-divider-spacing:var(--sp-1)" />

      <!-- Backlog entry: the count badge sits inside the button — .rail-btn is
           already position:relative + grid (styles.css), so no wrapper needed.
           The rail is the one place the design sanctions an accent surface for
           the active view (design/app.html:4708). The skin is inline, not the
           shared .accent-sel: the .rail-btn rule in styles.css resets background
           and border at equal specificity FURTHER DOWN the sheet, so the class
           would lose — an inline style is the only thing that outranks it. -->
      <button kjButton
        class="rail-btn"
        [style.background]="backlogActive() ? 'var(--ui-sel)' : null"
        [style.border-color]="backlogActive() ? 'var(--ui-line)' : null"
        (click)="ui.openBacklog()"
        title="Backlog"
      >
        <app-icon name="columns" size="sm" [color]="backlogActive() ? 'var(--ui-ink)' : 'var(--ink-3)'" />
        @if (openTicketCount() > 0) {
          <span
            class="tnum"
            style="position:absolute;top:2px;right:2px;min-width:13px;height:13px;padding:0 3px;border-radius:7px;background:var(--ui-fill);color:var(--ui-on-fill);font-size:var(--fs-micro);font-weight:var(--fw-strong);display:grid;place-items:center;border:2px solid var(--panel)"
          >{{ openTicketCount() }}</span>
        }
      </button>
      <kj-divider style="--kj-divider-color:var(--hair);--kj-divider-spacing:var(--sp-1)" />

      <div class="scroll-y" style="flex:1;width:100%;display:flex;flex-direction:column;align-items:center;gap:var(--sp-2)">
        @for (p of projects.all(); track p.id) {
          @let pa = agentsOf(p.id);
          <button kjButton
            class="rail-btn"
            (mouseover)="enter(p.id, $event)"
            (mouseleave)="leave()"
            (click)="openFirst(pa)"
            (contextmenu)="ui.openMenu($event, projects.projectMenu(p.id))"
            [style.border-color]="activeProj() === p.id ? mix(p.color, 50) : 'transparent'"
            [style.background]="
              activeProj() === p.id
                ? mix(p.color, 86)
                : hover()?.id === p.id
                  ? 'var(--panel-2)'
                  : 'transparent'
            "
          >
            <span
              [style.background]="mix(p.color, 84)"
              [style.border]="'1px solid ' + mix(p.color, 60)"
              style="width:var(--ctl-h-sm);height:var(--ctl-h-sm);border-radius:6px;display:grid;place-items:center"
            >
              <app-icon size="lg" [name]="p.icon" [color]="p.color" />
            </span>
            @if (runningOf(pa) > 0) {
              <span class="dot running" style="position:absolute;top:3px;right:3px;width:var(--sp-3);height:var(--sp-3);background:var(--st-running)"></span>
            }
            @if (needsOf(pa) > 0) {
              <span
                class="tnum"
                style="position:absolute;top:2px;right:2px;min-width:var(--sp-6);height:var(--sp-6);padding:0 var(--sp-1);border-radius:7px;background:var(--st-blocked);color:var(--on-solid);font-size:var(--fs-badge);font-weight:var(--fw-strong);display:grid;place-items:center;border:2px solid var(--panel)"
              >{{ needsOf(pa) }}</span>
            }
          </button>
        }
      </div>

      <!-- files section (v2) collapses to one folder icon on the rail -->
      <kj-divider style="--kj-divider-color:var(--hair);--kj-divider-spacing:var(--sp-2)" />
      <button kjButton class="rail-btn" (click)="expandFiles()" title="Files — expand sidebar">
        <app-icon name="folderOpen" size="sm" color="var(--ink-3)" />
      </button>

      <kj-divider style="--kj-divider-color:var(--hair);--kj-divider-spacing:var(--sp-2)" />
      <button kjButton class="rail-btn" (click)="ui.openAddProject()" title="Add project">
        <app-icon name="folder" size="sm" color="var(--ink-3)" />
      </button>
      <button kjButton
        class="rail-btn"
        (click)="ui.openSpawn(null)"
        title="Spawn agent"
        style="background:var(--ui-sel);border:1px solid var(--ui-sel-2);box-shadow:none"
      >
        <app-icon name="bolt" size="sm" color="var(--ui-ink)" />
      </button>

      @if (hoverProj(); as hp) {
        <div
          class="rise popover"
          (mouseover)="keep()"
          (mouseleave)="leave()"
          [style.top.px]="popTop()"
          style="position:fixed;left:52px;z-index:60;width: round(calc(236px * var(--density)), 1px);overflow:hidden"
        >
          <div class="pane-head">
            <span
              [style.background]="mix(hp.color, 82)"
              [style.border]="'1px solid ' + mix(hp.color, 62)"
              style="width:17px;height:17px;flex:none;border-radius:5px;display:grid;place-items:center"
            >
              <app-icon size="md" [name]="hp.icon" [color]="hp.color" />
            </span>
            <span style="font-weight:var(--fw-medium)">{{ hp.name }}</span>
            <span class="tnum" style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-4)">{{ hoverAgents().length }}</span>
            <button kjButton class="pane-btn" (click)="ui.openSpawn(hp.id)" title="Spawn agent">
              <app-icon size="lg" name="bolt" />
            </button>
          </div>
          <div style="padding:var(--sp-2);max-height: round(calc(280px * var(--density)), 1px);overflow-y:auto">
            @for (ag of hoverAgents(); track ag.id) {
              <div
                class="row-hover"
                [class.pending]="!!ag.transition"
                [attr.draggable]="ag.transition ? null : 'true'"
                (dragstart)="drag.start({ kind: 'agent', agentId: ag.id }); $event.dataTransfer!.effectAllowed = 'copy'"
                (dragend)="drag.end()"
                (click)="ui.openAgent(ag.id)"
                (contextmenu)="ui.openMenu($event, agentActions.agentMenu(ag.id))"
                style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-3) var(--sp-4);border-radius:6px;cursor:pointer"
                [style.opacity]="ag.transition ? 0.55 : null"
                [style.pointer-events]="ag.transition ? 'none' : null"
              >
                @if (ag.transition) {
                  <kj-spinner kjSize="xs" [kjAriaLabel]="ag.transition" />
                } @else {
                  <app-status-dot [status]="ag.status" />
                }
                <span class="trunc" style="flex:1">{{ ag.name }}</span>
                @if (needsAgent(ag)) {
                  <span style="width:var(--sp-2);height:var(--sp-2);border-radius:50%;background:var(--st-blocked)"></span>
                }
                <app-tool-badge [tool]="ag.tool" [size]="13" />
              </div>
            } @empty {
              <div style="padding:var(--sp-4) var(--sp-5);font-size:var(--fs-meta);color:var(--ink-4)">no agents — spawn one</div>
            }
          </div>
        </div>
      }
    </aside>
  `,
})
export class CompactRailComponent {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
  readonly agentActions = inject(AgentActionsService);
  readonly drag = inject(DragService);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly ticketsStore = inject(TicketsStore);

  readonly openTicketCount = computed(
    () => this.ticketsStore.all().filter((t) => t.status !== "done").length,
  );
  /** The backlog tab is the active one — drives the rail item's active skin. */
  readonly backlogActive = computed(() => this.ui.activeTabKind() === "backlog");

  /** Rail folder icon: restore the full sidebar with the files section open. */
  expandFiles(): void {
    this.ui.sidebarFilesCollapsed.set(false);
    this.ui.toggleSidebarCompact();
  }

  readonly mix = mix;
  readonly hover = signal<{ id: string; top: number } | null>(null);
  private timer: ReturnType<typeof setTimeout> | null = null;

  /** project id of the currently scoped agent (highlights its rail icon). */
  readonly activeProj = computed(() => this.runtime.activeAgent()?.projectId ?? null);

  readonly hoverProj = computed<Project | undefined>(() => {
    const h = this.hover();
    return h ? this.projects.all().find((p) => p.id === h.id) : undefined;
  });
  readonly hoverAgents = computed<Agent[]>(() => {
    const hp = this.hoverProj();
    return hp ? this.agentsOf(hp.id) : [];
  });
  /** clamp the popover so a long list never spills off the bottom of the window. */
  readonly popTop = computed(() => {
    const h = this.hover();
    if (!h) return 48;
    const listH = Math.min(60 + this.hoverAgents().length * 32, 320);
    return Math.max(48, Math.min(h.top, window.innerHeight - listH - 40));
  });

  agentsOf(projectId: string): Agent[] {
    return this.runtime.agents().filter((a) => a.projectId === projectId);
  }
  runningOf(agents: Agent[]): number {
    return agents.filter((a) => a.status === "running").length;
  }
  needsOf(agents: Agent[]): number {
    return agents.filter((a) => this.needsAgent(a)).length;
  }
  needsAgent(a: Agent): boolean {
    return a.status === "blocked" || !!a.pending?.some((p) => p.kind === "permission" || p.kind === "decision");
  }

  openFirst(agents: Agent[]) {
    if (agents[0]) this.ui.openAgent(agents[0].id);
  }

  enter(id: string, ev: MouseEvent) {
    if (this.timer) clearTimeout(this.timer);
    const el = (ev.currentTarget as HTMLElement).getBoundingClientRect();
    this.hover.set({ id, top: el.top });
  }
  keep() {
    if (this.timer) clearTimeout(this.timer);
  }
  leave() {
    this.timer = setTimeout(() => this.hover.set(null), 130);
  }
}
