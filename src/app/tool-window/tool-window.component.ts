import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { Agent, Project } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { IconComponent } from "../shared/icon.component";
import { BranchesPanelComponent } from "./branches-panel.component";
import { CommitGraphPanelComponent } from "./commit-graph-panel.component";
import { LocalHistoryPanelComponent } from "./local-history-panel.component";
import { TOOL_PANELS, ToolWindowStore } from "./tool-window.store";
import { KjButtonComponent, KjTabComponent, KjTabListComponent, KjTabsComponent } from "@kouji-ui/components";
import { SelectComponent } from "../shared/select.component";

/**
 * The IntelliJ-style bottom tool window (design toolwindow.jsx): a resizable
 * dock under the center content with a tab strip (accent underline), its own
 * project/worktree SCOPE — a tab can tile agents from several projects, so the
 * panels read from an explicit scope that by default FOLLOWS the focused agent
 * — and a close button. Closed by default; opened by the Git commands
 * (Ctrl+Shift+G / Ctrl+9 graph, Ctrl+Shift+B branches) or the palette.
 */
@Component({
  selector: "app-tool-window",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CommitGraphPanelComponent, BranchesPanelComponent, LocalHistoryPanelComponent, KjButtonComponent, KjTabsComponent, KjTabListComponent, KjTabComponent, SelectComponent],
  template: `
    <!-- resize grip (absolute — sits over the top hairline) -->
    <div class="grip" (mousedown)="onGripDown($event)"></div>

    <!-- tab strip -->
    <kj-tabs class="tw-tabs" style="flex:none" [value]="tw.panel() ?? ''" (valueChange)="tw.open($any($event))">
      <kj-tab-list>
        @for (p of panels; track p.kind) {
          @let on = p.kind === tw.panel();
          <kj-tab [value]="p.kind">
            <app-icon [name]="p.icon" size="sm" [color]="on ? 'var(--ui-ink)' : null" />{{ p.label }}
          </kj-tab>
        }
      </kj-tab-list>

      <!-- scope: project + worktree the panels act on -->
      <div style="margin-left:auto;display:flex;align-items:center;gap:var(--sp-3);padding-left:var(--sp-5)">
        <span class="up" style="color:var(--ink-4);flex:none">scope</span>
        @if (project(); as p) {
          <span style="display:flex;align-items:center;gap:var(--sp-2);flex:none">
            <app-icon size="md" [name]="p.icon" [color]="p.color" />
            <app-select
              title="Project the panels read from"
              [value]="p.id"
              [options]="projectOptions()"
              (valueChange)="pickProject($event)"
              style="width: round(calc(152px * var(--density)), 1px)"
            />
          </span>
        }
        <app-select
          title="Worktree the panel reads from"
          [value]="agent()?.id ?? ''"
          [options]="agentOptions()"
          (valueChange)="pickAgent($event)"
          style="width: round(calc(150px * var(--density)), 1px);flex:none"
        />
        @if (outOfSync()) {
          <kj-button kjVariant="toolbar" [title]="'Follow the focused agent · ' + focus()!.name" (click)="follow()">
            <app-icon size="md" name="link" />{{ focus()!.name }}
          </kj-button>
        }
        <button kjButton class="pane-btn" (click)="tw.close()" title="Hide tool window" style="align-self:center">
          <app-icon name="x" size="sm" />
        </button>
      </div>
    </kj-tabs>

    <!-- panel body -->
    <div style="flex:1;min-height:0;display:flex;flex-direction:column;background:var(--panel)">
      @switch (tw.panel()) {
        @case ('graph') { <app-commit-graph-panel [agent]="agent()" [project]="project()" /> }
        @case ('branches') { <app-branches-panel [agent]="agent()" [project]="project()" [agents]="projAgents()" /> }
        @case ('history') { <app-local-history-panel [agent]="agent()" /> }
      }
    </div>
  `,
  host: {
    "[style.height.px]": "h()",
  },
  styles: [
    `
      :host {
        position: relative;
        display: flex;
        flex-direction: column;
        min-height: 0;
        min-width: 0;
        flex: none;
        background: var(--panel);
        border-top: 1px solid var(--hair);
      }
      .grip {
        position: absolute;
        top: -2px;
        left: 0;
        right: 0;
        height: var(--sp-2);
        cursor: row-resize;
        z-index: 5;
      }
      /* the dock's strip carries the tabs AND the scope cluster on one row,
         so the hairline moves from the list to the strip itself */
      .tw-tabs {
        display: flex;
        align-items: center;
        border-bottom: 1px solid var(--hair);
      }
      .tw-tabs .kj-tab-list {
        border-bottom: none;
      }
    `,
  ],
})
export class ToolWindowComponent {
  readonly tw = inject(ToolWindowStore);
  readonly projects = inject(ProjectActionsService);
  readonly runtime = inject(AgentRuntimeService);

  readonly panels = TOOL_PANELS;

  // ---- dock height (design: 36vh clamped to [240, 420], drag-resizable) ----
  readonly h = signal(Math.round(Math.min(420, Math.max(240, window.innerHeight * 0.36))));

  onGripDown(e: MouseEvent): void {
    e.preventDefault();
    const startY = e.clientY;
    const startH = this.h();
    const move = (ev: MouseEvent) => {
      const next = startH + (startY - ev.clientY);
      this.h.set(Math.max(160, Math.min(window.innerHeight - 200, next)));
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      document.body.style.cursor = "";
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
    document.body.style.cursor = "row-resize";
  }

  // ---- scope ----
  // null = follow the focused agent (the default); set by the two selects.
  private readonly explicit = signal<{ projectId: string | null; agentId: string | null }>({
    projectId: null,
    agentId: null,
  });

  /** The focused agent of the active tab (what the scope follows by default). */
  readonly focus = computed<Agent | null>(() => this.runtime.activeAgent());

  readonly project = computed<Project | undefined>(() => {
    const all = this.projects.all();
    const ex = this.explicit();
    return (
      (ex.projectId ? all.find((p) => p.id === ex.projectId) : undefined) ??
      all.find((p) => p.id === this.focus()?.projectId) ??
      all[0]
    );
  });

  readonly projAgents = computed<Agent[]>(() => {
    const p = this.project();
    return p ? this.runtime.agents().filter((a) => a.projectId === p.id) : [];
  });

  readonly projectOptions = computed(() => this.projects.all().map((p) => ({ value: p.id, label: p.name })));

  readonly agentOptions = computed(() => {
    const list = this.projAgents();
    return list.length ? list.map((a) => ({ value: a.id, label: a.name })) : [{ value: "", label: "no worktrees" }];
  });

  readonly agent = computed<Agent | null>(() => {
    const list = this.projAgents();
    const ex = this.explicit();
    return (
      (ex.agentId ? list.find((a) => a.id === ex.agentId) : undefined) ??
      list.find((a) => a.id === this.focus()?.id) ??
      list[0] ??
      null
    );
  });

  /** An explicit scope is set and it diverges from the focused agent. */
  readonly outOfSync = computed(() => {
    const f = this.focus();
    const ex = this.explicit();
    if (!f || (!ex.projectId && !ex.agentId)) return false;
    if (!this.projects.all().some((p) => p.id === f.projectId)) return false;
    return this.agent()?.id !== f.id;
  });

  pickProject(id: string): void {
    this.explicit.set({ projectId: id, agentId: null });
  }
  pickAgent(id: string): void {
    this.explicit.update((s) => ({ projectId: s.projectId ?? this.project()?.id ?? null, agentId: id || null }));
  }
  follow(): void {
    this.explicit.set({ projectId: null, agentId: null });
  }
}
