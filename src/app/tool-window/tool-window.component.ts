import { ChangeDetectionStrategy, Component, computed, effect, inject } from "@angular/core";
import { AgentWorkStore } from "../agents/agent-work.store";
import { IconComponent } from "../shared/icon.component";
import { ScopeBarComponent } from "../shared/scope-bar.component";
import { ScopeSelection } from "../shared/scope";
import { BranchesPanelComponent } from "./branches-panel.component";
import { CommitGraphPanelComponent } from "./commit-graph-panel.component";
import { LocalHistoryPanelComponent } from "./local-history-panel.component";
import { TOOL_PANELS, ToolWindowStore } from "./tool-window.store";
import { KjButtonComponent, KjTabComponent, KjTabListComponent, KjTabsComponent } from "@kouji-ui/components";

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
  imports: [IconComponent, CommitGraphPanelComponent, BranchesPanelComponent, LocalHistoryPanelComponent, KjButtonComponent, KjTabsComponent, KjTabListComponent, KjTabComponent, ScopeBarComponent],
  providers: [ScopeSelection],
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
        <app-scope-bar [scope]="scope" />
        <button kjButton class="pane-btn" (click)="tw.close()" title="Hide tool window" style="align-self:center">
          <app-icon name="x" size="sm" />
        </button>
      </div>
    </kj-tabs>

    <!-- panel body -->
    <div style="flex:1;min-height:0;display:flex;flex-direction:column;background:var(--panel)">
      @switch (tw.panel()) {
        @case ('graph') { <app-commit-graph-panel [agent]="agent()" [project]="project()" /> }
        @case ('branches') { <app-branches-panel [agent]="realAgent()" [project]="project()" [agents]="projAgents()" /> }
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
  private readonly work = inject(AgentWorkStore);

  /** DOCK-LOCAL on purpose: the dock's scope is not the lookup overlays' scope
   *  (those share the root LookupScopeStore), so the two never drag each other. */
  readonly scope = inject(ScopeSelection);

  // What the strip and the panel bindings read — delegated so the template and
  // the pin effect keep their short names.
  readonly agent = this.scope.agent;
  readonly realAgent = this.scope.realAgent;
  readonly project = this.scope.project;
  readonly projAgents = this.scope.projAgents;

  readonly panels = TOOL_PANELS;

  constructor() {
    // Pin the scoped worktree's data while a dock panel shows it — a visible
    // key must never be LRU-evicted (see AgentWorkStore.pin).
    effect((onCleanup) => {
      const id = this.agent()?.id;
      if (id && this.tw.panel() !== null) onCleanup(this.work.pin(id));
    });
  }

  // ---- dock height (design: 36vh clamped to [240, 420], drag-resizable) ----
  // The preference lives in the store (persisted with the workspace); null =
  // the default. Always clamped so a restored value fits the current window.
  readonly h = computed(() => {
    const saved = this.tw.height();
    const def = Math.round(Math.min(420, Math.max(240, window.innerHeight * 0.36)));
    return saved == null ? def : Math.max(160, Math.min(window.innerHeight - 200, saved));
  });

  onGripDown(e: MouseEvent): void {
    e.preventDefault();
    const startY = e.clientY;
    const startH = this.h();
    const move = (ev: MouseEvent) => {
      const next = startH + (startY - ev.clientY);
      this.tw.height.set(Math.max(160, Math.min(window.innerHeight - 200, next)));
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
}
