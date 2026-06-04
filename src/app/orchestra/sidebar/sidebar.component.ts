import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { Agent } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";
import { ProjectGroupComponent } from "./project-group.component";

@Component({
  selector: "app-sidebar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ProjectGroupComponent],
  template: `
    <aside style="display:flex;flex-direction:column;min-height:0;background:var(--panel);border-right:1px solid var(--hair)">
      <div style="padding:10px 12px 8px;border-bottom:1px solid var(--hair)">
        <div style="display:flex;align-items:center;gap:7px;margin-bottom:8px">
          <app-icon name="layers" size="sm" color="var(--accent)" />
          <span class="up" style="font-size:9.5px;color:var(--ink-3)">Projects</span>
          <span class="chip tnum" style="font-size:9px;padding:0 6px">{{ store.projects().length }}</span>
          <span class="chip tnum" style="margin-left:auto;font-size:9px;padding:1px 6px">
            <span class="dot running" style="background:var(--st-running);width:6px;height:6px"></span>{{ totalRunning() }}/5
          </span>
        </div>
        <div style="display:flex;align-items:center;gap:7px;padding:5px 8px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm)">
          <app-icon name="search" size="sm" color="var(--ink-4)" />
          <input
            [value]="store.query()"
            (input)="store.query.set($any($event.target).value)"
            placeholder="filter agents…"
            style="flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:11.5px"
          />
          @if (store.query()) {
            <app-icon name="x" size="sm" color="var(--ink-4)" style="cursor:pointer" (click)="store.query.set('')" />
          }
        </div>
      </div>

      <div class="scroll-y" style="flex:1;padding:6px 0">
        @for (p of store.projects(); track p.id) {
          @let pa = agentsFor(p.id);
          @if (!store.query() || pa.length) {
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
        <button class="btn ghost-hair" (click)="store.openAddProject()" style="flex:1;justify-content:center">
          <app-icon name="folder" size="sm" />Add project
        </button>
        <button class="btn primary" (click)="store.openSpawn(null)" title="Spawn agent" style="padding:5px 11px">
          <app-icon name="plus" size="sm" />Agent
        </button>
      </div>
    </aside>
  `,
})
export class SidebarComponent {
  readonly store = inject(OrchestraStore);
  readonly collapsed = signal<Record<string, boolean>>({});

  readonly activeAgent = computed(() =>
    this.store.activeTab() === "orchestrator" ? null : this.store.activeTab(),
  );
  readonly totalRunning = computed(
    () => this.store.agents().filter((a) => a.status === "running").length,
  );

  agentsFor(projectId: string): Agent[] {
    const q = this.store.query().toLowerCase();
    return this.store
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
