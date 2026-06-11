import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from "@angular/core";
import { Agent, Project } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { DragService } from "../shared/drag.service";
import { UiStore } from "../ui/ui.store";
import { PaneNodeComponent } from "./pane-node.component";
import {
  countLeaves,
  DropSide,
  leaf,
  leafAgentOf,
  PaneCtx,
  patchLeaf,
  PaneView,
  removeLeaf,
  setRatio,
  splitLeaf,
  splitLeafSide,
  treeAgentIds,
} from "./pane-model";

/**
 * The split-pane workspace for one agent tab. Owns the tab's pane tree (stored in
 * UiStore, keyed by tab id) and renders it recursively via PaneNode. Implements
 * PaneCtx so the recursive node calls back here for split/close/assign/resize and
 * for drag-and-drop (a sidebar agent or a tab dropped onto a pane).
 */
@Component({
  selector: "app-pane-manager",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PaneNodeComponent],
  template: `
    <div style="display:flex;flex-direction:column;min-height:0;min-width:0;background:var(--panel-2)">
      <div style="flex:1;min-height:0;min-width:0;padding:8px;display:flex">
        @if (root(); as r) { <app-pane-node [node]="r" [ctx]="this" /> }
      </div>
    </div>
  `,
})
export class PaneManagerComponent implements PaneCtx {
  private ui = inject(UiStore);
  private runtime = inject(AgentRuntimeService);
  private projectActions = inject(ProjectActionsService);
  private drag = inject(DragService);

  /** The active agent tab id. */
  readonly tabId = input.required<string>();
  private readonly focus = signal<string | null>(null);

  readonly root = computed(() => this.ui.paneRoots()[this.tabId()]);

  // a sidebar agent / tab dragged over a pane: which pane + which side it'd land.
  // (a WritableSignal also satisfies the PaneCtx `dropTarget()` accessor.)
  readonly dropTarget = signal<{ paneId: string; side: DropSide } | null>(null);

  // ----- PaneCtx surface (read by the recursive node) -----
  agents(): Agent[] {
    return this.runtime.agents();
  }
  projects(): Project[] {
    return this.projectActions.all();
  }
  focusId(): string | null {
    return this.focus();
  }
  canClose(): boolean {
    const r = this.root();
    return !!r && countLeaves(r) > 1;
  }

  onSplit(leafId: string, dir: "v" | "h") {
    this.ui.setRoot(this.tabId(), (r) => splitLeaf(r, leafId, dir, leaf(this.pickNextAgent(), "terminal")));
  }
  onClose(leafId: string) {
    this.ui.setRoot(this.tabId(), (r) => removeLeaf(r, leafId));
  }
  onAgent(leafId: string, agentId: string) {
    this.ui.setRoot(this.tabId(), (r) => patchLeaf(r, leafId, { agentId }));
    this.ui.setScope(agentId);
  }
  onView(leafId: string, view: PaneView) {
    this.ui.setRoot(this.tabId(), (r) => patchLeaf(r, leafId, { view }));
  }
  onRatio(splitId: string, ratio: number) {
    this.ui.setRoot(this.tabId(), (r) => setRatio(r, splitId, ratio));
  }
  onFocus(leafId: string) {
    this.focus.set(leafId);
    const r = this.root();
    const aid = r ? leafAgentOf(r, leafId) : null;
    if (aid) this.ui.setScope(aid);
  }

  // ----- drag-and-drop into a pane -----
  onDropOver(paneId: string | null, side: DropSide) {
    this.dropTarget.set(paneId ? { paneId, side } : null);
  }
  onPaneDrop(paneId: string) {
    const d = this.drag.payload();
    const dt = this.dropTarget();
    const side: DropSide = dt && dt.paneId === paneId ? dt.side : "center";
    if (d?.agentId) {
      this.ui.setRoot(this.tabId(), (r) => splitLeafSide(r, paneId, side, leaf(d.agentId!, "terminal")));
      this.ui.setScope(d.agentId);
    }
    this.dropTarget.set(null);
    this.drag.end();
  }

  /** A sensible agent to drop into a freshly-split pane: an unused running agent,
   *  else any unused agent, else the first one. */
  private pickNextAgent(): string | null {
    const r = this.root();
    const used = new Set(r ? treeAgentIds(r) : []);
    const all = this.agents();
    const free =
      all.find((a) => !used.has(a.id) && a.status === "running") ??
      all.find((a) => !used.has(a.id)) ??
      all[0];
    return free ? free.id : null;
  }
}
