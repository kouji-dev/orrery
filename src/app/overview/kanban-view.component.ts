import { ChangeDetectionStrategy, Component, inject, input } from "@angular/core";
import { Agent, AgentStatus } from "../models";
import { AgentWorkStore } from "../agents/agent-work.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { STATUS_META } from "../utils";

interface Col {
  key: AgentStatus;
  label: string;
  alt?: AgentStatus;
}

@Component({
  selector: "app-kanban-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [StatusDotComponent, IconComponent],
  template: `
    <!-- columns keep a readable floor; past that the overview body scrolls sideways -->
    <div style="display:grid;grid-template-columns:repeat(4,minmax(180px,1fr));gap:12px;padding:18px;align-items:start;min-height:0">
      @for (c of cols; track c.key) {
        @let items = colItems(c);
        <div style="display:flex;flex-direction:column;gap:9px">
          <div [style.border-bottom]="'2px solid ' + color(c.key)" style="display:flex;align-items:center;gap:7px;padding:0 2px 4px">
            <span class="up" style="font-size:10px;color:var(--ink-2)">{{ c.label }}</span>
            <span class="chip tnum" style="margin-left:auto;font-size:9.5px;padding:0 6px">{{ items.length }}</span>
          </div>
          @for (ag of items; track ag.id) {
            <div
              class="surface rise kanban-card"
              (click)="ui.openAgent(ag.id)"
              style="padding:11px;cursor:pointer;display:flex;flex-direction:column;gap:7px"
            >
              <div style="display:flex;align-items:center;gap:7px">
                <app-status-dot [status]="ag.status" />
                <span class="disp" style="font-size:12.5px;font-weight:600">{{ ag.name }}</span>
              </div>
              <span style="font-size:11px;color:var(--ink-2);line-height:1.45;text-wrap:pretty">{{ ag.task }}</span>
              @if (ag.status === 'running') {
                <div class="activity"></div>
              }
              <div class="tnum" style="display:flex;align-items:center;gap:8px;font-size:10px;color:var(--ink-3)">
                <app-icon name="branch" size="sm" [px]="10" />
                <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.branch.replace('agent/', '') }}</span>
                <span style="margin-left:auto;color:var(--code-add-ink)">+{{ add(ag) }}</span>
              </div>
            </div>
          }
          @if (!items.length) {
            <div style="padding:14px;text-align:center;color:var(--ink-4);font-size:10.5px;border:1px dashed var(--hair);border-radius:var(--r-md)">empty</div>
          }
        </div>
      }
    </div>
  `,
  styles: [`.kanban-card:hover { border-color: var(--hair-2) !important; }`],
})
export class KanbanViewComponent {
  readonly ui = inject(UiStore);
  private work = inject(AgentWorkStore);
  readonly agents = input.required<Agent[]>();

  readonly cols: Col[] = [
    { key: "queued", label: "Queued" },
    { key: "running", label: "Running" },
    { key: "blocked", label: "Needs you", alt: "waiting" },
    { key: "done", label: "Done" },
  ];

  colItems(c: Col): Agent[] {
    return this.agents().filter((a) => a.status === c.key || a.status === c.alt);
  }
  color(key: AgentStatus): string {
    return STATUS_META[key].color;
  }
  add(ag: Agent): number {
    return this.work.changesFor(ag.id).data.reduce((s, f) => s + f.add, 0);
  }
}
