import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { Agent, Project } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { AgentWorkStore } from "../agents/agent-work.store";
import { ProjectActionsService } from "../projects/project-actions.service";
import { AgentsStore } from "../stores/agents.store";
import { TicketsStore } from "../stores/tickets.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { ProjectGroupComponent } from "./project-group.component";
import { KjBadgeComponent, KjButtonComponent, KjInputComponent, KjInputGroupAddonComponent, KjInputGroupComponent } from "@kouji-ui/components";
import { SidebarFilesComponent } from "./files/sidebar-files.component";

@Component({
  selector: "app-sidebar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ProjectGroupComponent, SidebarFilesComponent, KjButtonComponent, KjBadgeComponent, KjInputComponent, KjInputGroupAddonComponent, KjInputGroupComponent],
  template: `
    <aside style="display:flex;flex-direction:column;min-height:0;background:var(--panel);border-right:1px solid var(--hair)">
      <div style="padding:var(--sp-5) var(--sp-6) var(--sp-4);border-bottom:1px solid var(--hair)">
        <!-- Backlog nav entry — design/app.html NavRow (4569): idle transparent,
             active = panel-3 ground + hairline ring + a 2.5px --ui-ind bar on the
             sidebar's left edge. -->
        <div style="position:relative;margin-bottom:var(--sp-3)">
          @if (backlogActive()) {
            <span style="position:absolute;left:calc(-1 * var(--sp-6));top:7px;bottom:7px;width:2.5px;border-radius:3px;background:var(--ui-ind)"></span>
          }
          <kj-button
            class="nav-row"
            [class.active]="backlogActive()"
            kjVariant="ghost"
            [kjFullWidth]="true"
            (click)="ui.openBacklog()"
            style="display:flex;--kj-button-gap:var(--sp-3);--kj-button-padding-y:var(--sp-2);--kj-button-padding-x:var(--sp-3);--kj-button-radius:var(--r-md);--kj-button---kj-button-height:auto;--kj-button-justify:flex-start"
          >
            <app-icon name="columns" size="sm" [color]="backlogActive() ? 'var(--ui-ink)' : 'var(--ink-3)'" />
            <span style="font-weight:var(--fw-medium)">Backlog</span>
            @if (openTicketCount() > 0) {
              <kj-badge class="tnum">{{ openTicketCount() }}</kj-badge>
            }
          </kj-button>
        </div>
        <!-- nowrap + the collapse button owning the auto-margin keeps it pinned
             to the far right of THIS row whatever the counts do -->
        <div style="display:flex;align-items:center;flex-wrap:nowrap;gap:var(--sp-3);margin-bottom:var(--sp-4);min-width:0">
          <app-icon name="layers" size="sm" color="var(--ui-ink)" />
          <span class="up trunc" style="color:var(--ink-3)">Projects</span>
          <kj-badge class="tnum">{{ projects.all().length }}</kj-badge>
          <kj-badge class="tnum">
            <span class="dot running" style="background:var(--st-running);width:var(--sp-3);height:var(--sp-3)"></span>{{ totalRunning() }}/5
          </kj-badge>
          <kj-button kjSize="icon" class="pane-act kj-push" kjVariant="ghost" (click)="toggleAll()"
            [title]="allCollapsed() ? 'Expand all projects' : 'Collapse all projects'"
            [kjAriaLabel]="allCollapsed() ? 'Expand all projects' : 'Collapse all projects'"
            style="flex:none;--kj-button-padding-x:var(--sp-1);--kj-button-padding-y:var(--sp-1);--kj-button-height:auto;--kj-button-radius:4px">
            <app-icon size="sm" [name]="allCollapsed() ? 'chevsDown' : 'chevsUp'" />
          </kj-button>
          <kj-button kjSize="icon" class="pane-act" kjVariant="ghost" (click)="ui.toggleSidebarCompact()" title="Collapse sidebar" kjAriaLabel="Collapse sidebar"
            style="flex:none;--kj-button-padding-x:var(--sp-1);--kj-button-padding-y:var(--sp-1);--kj-button-height:auto;--kj-button-radius:4px">
            <app-icon size="sm" name="panelLeft" />
          </kj-button>
        </div>
        <!-- kj-input-group does NOT cascade kjSize to the child input (it stays
             data-size-less), so the xs metrics are set as knobs here — they
             inherit down to the input that reads them. -->
        <kj-input-group style="--kj-input-font-family:var(--font-mono)">
          <kj-input-group-addon kjPosition="start" kjAriaHidden="true">
            <app-icon name="search" size="sm" color="var(--ink-4)" />
          </kj-input-group-addon>
          <kj-input
            [value]="ui.query()"
            (input)="ui.query.set($any($event.target).value)"
            placeholder="filter projects, agents…"
          />
          @if (ui.query()) {
            <kj-input-group-addon kjPosition="end">
              <app-icon name="x" size="sm" color="var(--ink-4)" style="cursor:pointer" (click)="ui.query.set('')" />
            </kj-input-group-addon>
          }
        </kj-input-group>
      </div>

      <!-- panel actions sit ABOVE the list (design/orrery-v2.html): the files
           section owns the sidebar's bottom edge, so a footer would be buried -->
      <div class="sb-actions" style="padding:var(--sp-4) var(--sp-5);border-bottom:1px solid var(--hair);display:flex;gap:var(--sp-4)">
        <kj-button kjVariant="outline" (click)="ui.openAddProject()">
          <app-icon name="folder" size="sm" />Add project
        </kj-button>
        <kj-button kjVariant="default" (click)="ui.openSpawn(null)" title="Spawn agent">
          <app-icon name="bolt" size="sm" />Agent
        </kj-button>
      </div>

      <!-- agents section: empty space below the groups still offers the
           panel-level actions; project/agent rows stopPropagation via
           ui.openMenu, so theirs win -->
      <div class="scroll-y" style="flex:1;min-height:0;padding:var(--sp-3) 0" (contextmenu)="onEmptyContext($event)">
        @for (g of groups(); track g.project.id) {
          <app-project-group
            [project]="g.project"
            [agents]="g.agents"
            [activeAgent]="activeAgent()"
            [collapsed]="!!collapsed()[g.project.id]"
            (toggle)="toggle($event)"
          />
        }
      </div>

      <!-- files section (v2): the repo tree with its worktree root chip -->
      <app-sidebar-files />
    </aside>
  `,
  styles: [
    `
      /* mockup NavRow/pane-btn hovers: neutral panel tints, no lift.
         Unlayered component styles outrank kouji's @layer kj.component,
         which otherwise tints ghost hover with a fg mix + translateY. */
      kj-button.pane-act {
        --kj-button-fg: var(--ink-3);
      }
      /* NavRow active (design/app.html:4569): the knobs have to land on the
         INNER .kj-button — the global ghost-variant rules declare them there,
         so a host-level value would lose the cascade. */
      /* The count pill is the design's neutral .chip (app.html:4578), not a
         bold accent pill: kouji declares the badge knobs ON .kj-badge, so the
         size + the push have to be set there — a host-level value (the host is
         display:contents) never reaches the box. */
      kj-button.nav-row kj-badge ::ng-deep .kj-badge {
        --kj-badge-font-size: var(--fs-badge);
        --kj-badge-padding-x: var(--sp-3);
        margin-left: auto;
      }
      kj-button.nav-row:not(.active) ::ng-deep .kj-button:hover {
        --kj-button-bg: var(--panel-2);
      }
      kj-button.nav-row.active ::ng-deep .kj-button {
        --kj-button-fg: var(--ink);
        --kj-button-bg: var(--panel-3);
        --kj-button-border-color: var(--hair-2);
      }
      kj-button.pane-act:hover {
        --kj-button-bg: var(--panel-3);
        --kj-button-fg: var(--ink);
      }
    `,
  ],
})
export class SidebarComponent {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
  readonly runtime = inject(AgentRuntimeService);
  private readonly agentsStore = inject(AgentsStore);
  private readonly ticketsStore = inject(TicketsStore);
  readonly collapsed = signal<Record<string, boolean>>({});

  constructor() {
    // one full-count pass for every agent's sidebar counters; afterwards only
    // running/visible agents' watcher scans update them
    inject(AgentWorkStore).initTotals();
  }

  readonly activeAgent = computed(() => this.runtime.activeAgent()?.id ?? null);
  readonly totalRunning = computed(
    () => this.runtime.agents().filter((a) => a.status === "running").length,
  );
  readonly openTicketCount = computed(
    () => this.ticketsStore.all().filter((t) => t.status !== "done").length,
  );
  /** The backlog tab is the active one — drives the NavRow's active skin. */
  readonly backlogActive = computed(() => this.ui.activeTabKind() === "backlog");

  /**
   * The filter reads PROJECT-FIRST: a query that hits a project's name keeps
   * that whole project — every agent under it, even one whose own name/task
   * misses — so "orrery" means "show me orrery", not "show me nothing".
   * Projects the query does not name fall back to matching their agents, and
   * drop out entirely when none match.
   */
  readonly groups = computed<{ project: Project; agents: Agent[] }[]>(() => {
    const q = this.ui.query().trim().toLowerCase();
    // spawn placeholders ride along until agent://created lands under their id
    const live = this.runtime.agents();
    const liveIds = new Set(live.map((a) => a.id));
    const agents = [...live, ...this.agentsStore.pendingAgents().filter((p) => !liveIds.has(p.id))];
    const byProject = new Map<string, Agent[]>();
    for (const a of agents) {
      const list = byProject.get(a.projectId);
      if (list) list.push(a);
      else byProject.set(a.projectId, [a]);
    }
    const out: { project: Project; agents: Agent[] }[] = [];
    for (const project of this.projects.all()) {
      const own = byProject.get(project.id) ?? [];
      if (!q || project.name.toLowerCase().includes(q)) {
        out.push({ project, agents: own });
        continue;
      }
      const hits = own.filter(
        (a) => a.name.toLowerCase().includes(q) || a.task.toLowerCase().includes(q),
      );
      if (hits.length) out.push({ project, agents: hits });
    }
    return out;
  });

  /** All *visible* groups collapsed — the button flips to "expand all" there. */
  readonly allCollapsed = computed(() => {
    const g = this.groups();
    return g.length > 0 && g.every((x) => this.collapsed()[x.project.id]);
  });

  toggle(id: string) {
    this.collapsed.update((c) => ({ ...c, [id]: !c[id] }));
  }

  /** One click folds every visible project away, the next brings them back. */
  toggleAll(): void {
    const groups = this.groups();
    if (this.allCollapsed()) {
      this.collapsed.update((c) => {
        const next = { ...c };
        for (const g of groups) delete next[g.project.id];
        return next;
      });
      return;
    }
    this.collapsed.update((c) => ({
      ...c,
      ...Object.fromEntries(groups.map((g) => [g.project.id, true])),
    }));
  }

  /** Right-click on empty panel space: the panel-level actions. */
  onEmptyContext(e: MouseEvent): void {
    this.ui.openMenu(e, [
      { label: "Add Project…", icon: "folderOpen", onClick: () => this.ui.openAddProject() },
      { label: "Spawn Agent…", icon: "bolt", onClick: () => this.ui.openSpawn(null) },
    ]);
  }
}
