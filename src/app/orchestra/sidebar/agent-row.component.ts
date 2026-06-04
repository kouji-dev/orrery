import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent } from "../models";
import { OrchestraStore } from "../orchestra.store";
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
      class="agent-row"
      [class.active]="active()"
      (click)="store.openAgent(ag.id)"
      (contextmenu)="store.openMenu($event, store.agentMenu(ag.id))"
      style="display:flex;flex-direction:column;gap:3px;padding:6px 10px 7px;cursor:pointer;position:relative;border-radius:var(--r-md);margin:1px 8px 1px 14px"
    >
      @if (active()) {
        <span style="position:absolute;left:-8px;top:7px;bottom:7px;width:2.5px;border-radius:3px;background:linear-gradient(var(--accent),var(--accent-2))"></span>
      }
      <div style="display:flex;align-items:center;gap:7px">
        <app-status-dot [status]="ag.status" />
        <span style="font-size:var(--fs-tree);color:var(--ink);font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.name }}</span>
        @if (needs()) {
          <span style="width:5px;height:5px;border-radius:50%;background:var(--st-blocked);flex:none"></span>
        }
        <app-tool-badge [tool]="ag.tool" [size]="14" />
        <span class="tnum" style="margin-left:auto;font-size:9.5px;color:var(--ink-4)">{{ ag.elapsed ? fmt(ag.elapsed) : '—' }}</span>
      </div>
      <div style="display:flex;align-items:center;gap:6px;padding-left:15px">
        <app-icon name="branch" size="sm" [px]="11" color="var(--ink-4)" />
        <span style="font-size:10.5px;color:var(--ink-3);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.branch.replace('agent/', '') }}</span>
        @if (ag.files.length > 0) {
          <span class="tnum" style="margin-left:auto;font-size:10px;display:flex;gap:5px;flex:none">
            <span style="color:var(--code-add-ink)">+{{ totAdd() }}</span>
            <span style="color:var(--code-del-ink)">−{{ totDel() }}</span>
          </span>
        }
      </div>
      @if (ag.status === 'running') {
        <div class="activity" style="margin-left:15px;margin-top:2px"></div>
      }
    </div>
  `,
  styles: [
    `
      .agent-row:hover:not(.active) {
        background: var(--panel-2);
      }
      .agent-row.active {
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
  readonly store = inject(OrchestraStore);
  readonly agent = input.required<Agent>();
  readonly active = input<boolean>(false);

  readonly fmt = fmtDur;
  readonly totAdd = computed(() => this.agent().files.reduce((s, f) => s + f.add, 0));
  readonly totDel = computed(() => this.agent().files.reduce((s, f) => s + f.del, 0));
  readonly needs = computed(() => {
    const ag = this.agent();
    return (
      ag.status === "blocked" ||
      (ag.pending && ag.pending.some((p) => p.kind === "permission" || p.kind === "decision"))
    );
  });
}
