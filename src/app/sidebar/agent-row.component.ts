import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { AgentWorkStore } from "../agents/agent-work.store";
import { DragService } from "../shared/drag.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { fmtDur } from "../utils";

@Component({
  selector: "app-agent-row",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [StatusDotComponent, ToolBadgeComponent, IconComponent],
  template: `
    @let ag = agent();
    <div
      class="agent-row row-hover"
      [class.active]="active()"
      draggable="true"
      (dragstart)="drag.start({ kind: 'agent', agentId: ag.id }); $event.dataTransfer!.effectAllowed = 'copy'"
      (dragend)="drag.end()"
      (click)="ui.openAgent(ag.id)"
      (contextmenu)="ui.openMenu($event, agentActions.agentMenu(ag.id))"
      title="drag onto a pane or tab to add its terminal"
      style="display:flex;flex-direction:column;gap:var(--sp-1);padding:var(--sp-3) var(--sp-5) var(--sp-3);cursor:pointer;position:relative;border-radius:var(--r-md);margin:1px var(--sp-4) 1px var(--sp-6)"
    >
      @if (active()) {
        <span style="position:absolute;left:-8px;top:7px;bottom:7px;width:2.5px;border-radius:3px;background:var(--ui-ind)"></span>
      }
      <div style="display:flex;align-items:center;gap:var(--sp-3)">
        <app-status-dot [status]="ag.status" />
        <span class="trunc" style="color:var(--ink);font-weight:var(--fw-medium)">{{ ag.name }}</span>
        @if (needs()) {
          <span style="width:var(--sp-2);height:var(--sp-2);border-radius:50%;background:var(--st-blocked);flex:none"></span>
        }
        <app-tool-badge [tool]="ag.tool" [size]="14" />
        <span class="tnum" style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-4)">{{ elapsed() ? fmt(elapsed()) : '—' }}</span>
      </div>
      <div style="display:flex;align-items:center;gap:var(--sp-3);padding-left:var(--sp-7)">
        <app-icon size="md" name="branch" color="var(--ink-4)" />
        <span class="trunc" style="font-size:var(--fs-meta);color:var(--ink-3)">{{ ag.branch.replace('agent/', '') }}</span>
        @if (tot(); as t) {
          @if (t.files > 0) {
            <span class="tnum" style="margin-left:auto;font-size:var(--fs-meta);display:flex;gap:var(--sp-2);flex:none">
              <span style="color:var(--code-add-ink)">+{{ t.add }}</span>
              <span style="color:var(--code-del-ink)">−{{ t.del }}</span>
            </span>
          }
        }
      </div>
      @if (ag.status === 'running') {
        <div class="activity" style="margin-left:var(--sp-7);margin-top:var(--sp-1)"></div>
      }
    </div>
  `,
  styles: [
    `
      /* selected outranks the shared .row-hover ground */
      .agent-row.active,
      .agent-row.active:hover {
        background: var(--panel-3);
        border: 1px solid var(--hair-2);
      }
      .agent-row:not(.active) {
        border: 1px solid transparent;
      }
    `,
  ],
})
export class AgentRowComponent {
  readonly ui = inject(UiStore);
  readonly agentActions = inject(AgentActionsService);
  readonly drag = inject(DragService);
  readonly agent = input.required<Agent>();
  readonly active = input<boolean>(false);

  readonly fmt = fmtDur;
  private work = inject(AgentWorkStore);
  private runtime = inject(AgentRuntimeService);
  // derived from the shared clock — only this text re-renders on a tick, the
  // agent input (and the agents array behind it) keeps its identity
  readonly elapsed = computed(() => this.runtime.elapsedFor(this.agent().id));
  /** Sidebar counters come from the dedicated totals map (init pass + full
   *  scans) — NOT from changesFor, whose entries are LRU-evicted and whose
   *  counts-only background scans would show +0 −0. */
  readonly tot = computed(() => this.work.totalsFor(this.agent().id));
  readonly needs = computed(() => {
    const ag = this.agent();
    return (
      ag.status === "blocked" ||
      (ag.pending && ag.pending.some((p) => p.kind === "permission" || p.kind === "decision"))
    );
  });
}
