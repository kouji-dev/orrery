import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { Agent } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { TicketsStore } from "../stores/tickets.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { ProjectGroupComponent } from "./project-group.component";

@Component({
  selector: "app-sidebar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ProjectGroupComponent],
  template: `
    <aside style="display:flex;flex-direction:column;min-height:0;background:var(--panel);border-right:1px solid var(--hair)">
      <div style="padding:10px 12px 8px;border-bottom:1px solid var(--hair)">
        <!-- Backlog nav entry -->
        <button
          (click)="ui.openBacklog()"
          [style.background]="ui.activeTabKind() === 'backlog' ? 'var(--panel-2)' : 'transparent'"
          [style.color]="ui.activeTabKind() === 'backlog' ? 'var(--ink)' : 'var(--ink-3)'"
          style="display:flex;align-items:center;gap:7px;width:100%;padding:5px 6px;border-radius:var(--r-sm);border:none;cursor:pointer;font-size:12px;margin-bottom:6px"
        >
          <app-icon name="layers" size="sm" [color]="ui.activeTabKind() === 'backlog' ? 'var(--accent)' : null" />
          <span>Backlog</span>
          @if (openTicketCount() > 0) {
            <span
              class="tnum"
              style="margin-left:auto;min-width:16px;height:16px;padding:0 4px;border-radius:8px;background:var(--accent);color:#06070b;font-size:9px;font-weight:700;display:grid;place-items:center"
            >{{ openTicketCount() }}</span>
          }
        </button>
        <div style="display:flex;align-items:center;gap:7px;margin-bottom:8px">
          <app-icon name="layers" size="sm" color="var(--accent)" />
          <span class="up" style="font-size:9.5px;color:var(--ink-3)">Projects</span>
          <span class="chip tnum" style="font-size:9px;padding:0 6px">{{ projects.all().length }}</span>
          <span class="chip tnum" style="margin-left:auto;font-size:9px;padding:1px 6px">
            <span class="dot running" style="background:var(--st-running);width:6px;height:6px"></span>{{ totalRunning() }}/5
          </span>
          <button class="pane-btn" (click)="ui.toggleSidebarCompact()" title="Collapse sidebar">
            <app-icon name="panelLeft" size="sm" [px]="14" />
          </button>
        </div>
        <div style="display:flex;align-items:center;gap:7px;padding:5px 8px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm)">
          <app-icon name="search" size="sm" color="var(--ink-4)" />
          <input
            [value]="ui.query()"
            (input)="ui.query.set($any($event.target).value)"
            placeholder="filter agents…"
            style="flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:11.5px"
          />
          @if (ui.query()) {
            <app-icon name="x" size="sm" color="var(--ink-4)" style="cursor:pointer" (click)="ui.query.set('')" />
          }
        </div>
      </div>

      <div class="scroll-y" style="flex:1;padding:6px 0">
        @for (p of projects.all(); track p.id) {
          @let pa = agentsFor(p.id);
          @if (!ui.query() || pa.length) {
            <app-project-group
              [project]="p"
              [agents]="pa"
              [activeAgent]="activeAgent()"
              [collapsed]="!!collapsed()[p.id]"
              (toggle)="toggle($event)"
            />
          }
        }
      </div>

      <div style="padding:10px;border-top:1px solid var(--hair);display:flex;gap:8px">
        <button class="btn ghost-hair" (click)="ui.openAddProject()" style="flex:1;justify-content:center">
          <app-icon name="folder" size="sm" />Add project
        </button>
        <button class="btn primary" (click)="ui.openSpawn(null)" title="Spawn agent" style="padding:5px 11px">
          <app-icon name="bolt" size="sm" />Agent
        </button>
      </div>
    </aside>
  `,
})
export class SidebarComponent {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
  readonly runtime = inject(AgentRuntimeService);
  private readonly ticketsStore = inject(TicketsStore);
  readonly collapsed = signal<Record<string, boolean>>({});

  readonly activeAgent = computed(() => this.runtime.activeAgent()?.id ?? null);
  readonly totalRunning = computed(
    () => this.runtime.agents().filter((a) => a.status === "running").length,
  );
  readonly openTicketCount = computed(
    () => this.ticketsStore.all().filter((t) => t.status !== "done").length,
  );

  agentsFor(projectId: string): Agent[] {
    const q = this.ui.query().toLowerCase();
    return this.runtime
      .agents()
      .filter(
        (a) =>
          a.projectId === projectId &&
          (!q || a.name.toLowerCase().includes(q) || a.task.toLowerCase().includes(q)),
      );
  }

  toggle(id: string) {
    this.collapsed.update((c) => ({ ...c, [id]: !c[id] }));
  }
}
