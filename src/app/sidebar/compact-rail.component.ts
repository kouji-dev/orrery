import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { Agent, Project } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { AgentActionsService } from "../agents/agent-actions.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { DragService } from "../shared/drag.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { mix } from "../utils";

/**
 * Collapsed sidebar: a 54px rail of project icons. Hovering a project pops a
 * floating list of its agents (the same open / context-menu / spawn actions as
 * the full sidebar). The header "expand" button restores the full sidebar.
 */
@Component({
  selector: "app-compact-rail",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, StatusDotComponent, ToolBadgeComponent],
  template: `
    <aside
      style="display:flex;flex-direction:column;align-items:center;min-height:0;width:54px;background:var(--panel);border-right:1px solid var(--hair);padding:8px 0;gap:4px;position:relative"
    >
      <button class="rail-btn" (click)="ui.toggleSidebarCompact()" title="Expand sidebar" style="margin-bottom:2px">
        <app-icon name="columns" size="sm" color="var(--accent)" />
      </button>
      <div style="width:24px;height:1px;background:var(--hair);margin:2px 0 4px"></div>

      <div class="scroll-y" style="flex:1;width:100%;display:flex;flex-direction:column;align-items:center;gap:5px">
        @for (p of projects.all(); track p.id) {
          @let pa = agentsOf(p.id);
          <div
            class="rail-item"
            (mouseover)="enter(p.id, $event)"
            (mouseleave)="leave()"
          >
            <button
              class="rail-btn"
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
                style="width:22px;height:22px;border-radius:6px;display:grid;place-items:center"
              >
                <app-icon [name]="p.icon" size="sm" [px]="13" [color]="p.color" />
              </span>
              @if (runningOf(pa) > 0) {
                <span class="dot running" style="position:absolute;top:3px;right:3px;width:7px;height:7px;background:var(--st-running)"></span>
              }
              @if (needsOf(pa) > 0) {
                <span
                  class="tnum"
                  style="position:absolute;top:2px;right:2px;min-width:13px;height:13px;padding:0 3px;border-radius:7px;background:var(--st-blocked);color:#fff;font-size:8px;font-weight:700;display:grid;place-items:center;border:2px solid var(--panel)"
                >{{ needsOf(pa) }}</span>
              }
            </button>
          </div>
        }
      </div>

      <div style="width:24px;height:1px;background:var(--hair);margin:4px 0"></div>
      <button class="rail-btn" (click)="ui.openAddProject()" title="Add project">
        <app-icon name="folder" size="sm" color="var(--ink-3)" />
      </button>
      <button
        class="rail-btn"
        (click)="ui.openSpawn(null)"
        title="Spawn agent"
        style="background:linear-gradient(180deg,var(--accent),color-mix(in oklch,var(--accent),#000 14%));border:none;box-shadow:0 0 14px -5px rgba(var(--accent-rgb),.8)"
      >
        <app-icon name="bolt" size="sm" color="#06070b" />
      </button>

      @if (hoverProj(); as hp) {
        <div
          class="rise"
          (mouseover)="keep()"
          (mouseleave)="leave()"
          [style.top.px]="popTop()"
          style="position:fixed;left:52px;z-index:60;width:236px;background:var(--elev);border:1px solid var(--hair-2);border-radius:var(--r-md);box-shadow:var(--shadow);overflow:hidden"
        >
          <div style="display:flex;align-items:center;gap:8px;padding:9px 11px;border-bottom:1px solid var(--hair)">
            <span
              [style.background]="mix(hp.color, 82)"
              [style.border]="'1px solid ' + mix(hp.color, 62)"
              style="width:17px;height:17px;flex:none;border-radius:5px;display:grid;place-items:center"
            >
              <app-icon [name]="hp.icon" size="sm" [px]="11" [color]="hp.color" />
            </span>
            <span style="font-size:12px;font-weight:600">{{ hp.name }}</span>
            <span class="tnum" style="margin-left:auto;font-size:9.5px;color:var(--ink-4)">{{ hoverAgents().length }}</span>
            <button class="pane-btn" (click)="ui.openSpawn(hp.id)" title="Spawn agent">
              <app-icon name="bolt" size="sm" [px]="13" />
            </button>
          </div>
          <div style="padding:5px;max-height:280px;overflow-y:auto">
            @for (ag of hoverAgents(); track ag.id) {
              <div
                class="rail-pop-row"
                draggable="true"
                (dragstart)="drag.start({ kind: 'agent', agentId: ag.id }); $event.dataTransfer!.effectAllowed = 'copy'"
                (dragend)="drag.end()"
                (click)="ui.openAgent(ag.id)"
                (contextmenu)="ui.openMenu($event, agentActions.agentMenu(ag.id))"
                style="display:flex;align-items:center;gap:8px;padding:6px 8px;border-radius:6px;cursor:pointer"
              >
                <app-status-dot [status]="ag.status" />
                <span style="flex:1;font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.name }}</span>
                @if (needsAgent(ag)) {
                  <span style="width:5px;height:5px;border-radius:50%;background:var(--st-blocked)"></span>
                }
                <app-tool-badge [tool]="ag.tool" [size]="13" />
              </div>
            } @empty {
              <div style="padding:8px 10px;font-size:10.5px;color:var(--ink-4)">no agents — spawn one</div>
            }
          </div>
        </div>
      }
    </aside>
  `,
  styles: [
    `
      .rail-pop-row:hover {
        background: var(--panel-2);
      }
    `,
  ],
})
export class CompactRailComponent {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
  readonly agentActions = inject(AgentActionsService);
  readonly drag = inject(DragService);
  private readonly runtime = inject(AgentRuntimeService);

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
