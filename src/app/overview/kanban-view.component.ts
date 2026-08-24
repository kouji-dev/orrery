import { ChangeDetectionStrategy, Component, inject, input } from "@angular/core";
import { Agent, AgentStatus } from "../models";
import { AgentWorkStore } from "../agents/agent-work.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { STATUS_META } from "../utils";
import { KjBadgeComponent } from "@kouji-ui/components";

interface Col {
  key: AgentStatus;
  label: string;
  alt?: AgentStatus;
}

@Component({
  selector: "app-kanban-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [StatusDotComponent, IconComponent, KjBadgeComponent],
  template: `
    <!-- columns keep a readable floor; past that the overview body scrolls sideways -->
    <div style="display:grid;grid-template-columns:repeat(4,minmax(180px,1fr));gap:var(--sp-6);padding:var(--sp-7);align-items:start;min-height:0">
      @for (c of cols; track c.key) {
        @let items = colItems(c);
        <div style="display:flex;flex-direction:column;gap:var(--sp-4)">
          <div [style.border-bottom]="'2px solid ' + color(c.key)" style="display:flex;align-items:center;gap:var(--sp-3);padding:0 var(--sp-1) var(--sp-2)">
            <span class="up" style="color:var(--ink-2)">{{ c.label }}</span>
            <kj-badge class="tnum" style="display:inline-flex;margin-left:auto">{{ items.length }}</kj-badge>
          </div>
          @for (ag of items; track ag.id) {
            <div
              class="surface rise kanban-card"
              (click)="ui.openAgent(ag.id)"
              style="padding:var(--sp-5);cursor:pointer;display:flex;flex-direction:column;gap:var(--sp-3)"
            >
              <div style="display:flex;align-items:center;gap:var(--sp-3)">
                <app-status-dot [status]="ag.status" />
                <h4>{{ ag.name }}</h4>
              </div>
              <!-- two-line clamp keeps kanban cards a uniform height -->
              <span style="color:var(--ink-2);line-height:1.45;text-wrap:pretty;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden;height:2.9em">{{ ag.task }}</span>
              @if (ag.status === 'running') {
                <div class="activity"></div>
              }
              <div class="tnum" style="display:flex;align-items:center;gap:var(--sp-4);color:var(--ink-3)">
                <app-icon size="sm" name="branch" />
                <span class="trunc">{{ ag.branch.replace('agent/', '') }}</span>
                <span style="margin-left:auto;color:var(--code-add-ink)">+{{ add(ag) }}</span>
              </div>
            </div>
          }
          @if (!items.length) {
            <div style="padding:var(--sp-6);text-align:center;color:var(--ink-4);border:1px dashed var(--hair);border-radius:var(--r-md)">empty</div>
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
