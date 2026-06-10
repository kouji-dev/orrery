import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { UiStore } from "../ui/ui.store";
import { StatusDotComponent } from "../shared/status-dot.component";
import { fmtDur, mix, STATUS_META } from "../utils";

@Component({
  selector: "app-timeline-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [StatusDotComponent],
  template: `
    <div style="padding:18px;display:flex;flex-direction:column;gap:2px">
      <div class="up" style="display:grid;grid-template-columns:150px 1fr 70px;gap:12px;padding:0 4px 8px;font-size:9.5px">
        <span style="color:var(--ink-3)">Agent</span>
        <span style="color:var(--ink-3)">Elapsed · progress</span>
        <span style="color:var(--ink-3);text-align:right">Commits</span>
      </div>
      @for (ag of agents(); track ag.id) {
        @let w = width(ag);
        <div
          class="timeline-row"
          (click)="ui.openAgent(ag.id)"
          style="display:grid;grid-template-columns:150px 1fr 70px;gap:12px;align-items:center;padding:9px 4px;cursor:pointer;border-radius:var(--r-sm);border-bottom:1px solid var(--hair)"
        >
          <div style="display:flex;align-items:center;gap:7px;min-width:0">
            <app-status-dot [status]="ag.status" />
            <span class="disp" style="font-size:12.5px;font-weight:600;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.name }}</span>
          </div>
          <div style="position:relative;height:22px;display:flex;align-items:center">
            <div
              [style.width]="w + '%'"
              [style.background]="mix(color(ag), 55)"
              [style.border]="'1px solid ' + color(ag)"
              style="position:absolute;left:0;height:9px;border-radius:5px;overflow:hidden"
            >
              <div [style.width]="ag.progress * 100 + '%'" [style.background]="color(ag)" style="height:100%;opacity:0.5"></div>
              @if (ag.status === 'running') {
                <div class="activity" style="position:absolute;inset:0;background:transparent"></div>
              }
            </div>
            <span class="tnum" [style.left]="'calc(' + w + '% + 8px)'" style="position:absolute;font-size:10px;color:var(--ink-3);white-space:nowrap">
              {{ elapsed(ag) ? fmt(elapsed(ag)) : '—' }} · {{ pct(ag) }}%
            </span>
          </div>
          <span class="tnum" style="text-align:right;font-size:11px;color:var(--ink-2)">{{ ag.commits }}</span>
        </div>
      }
    </div>
  `,
  styles: [`.timeline-row:hover { background: var(--panel-2); }`],
})
export class TimelineViewComponent {
  readonly ui = inject(UiStore);
  readonly agents = input.required<Agent[]>();

  readonly fmt = fmtDur;
  readonly mix = mix;
  // elapsed comes from the shared runtime clock (not the Agent record), so the
  // bars/labels track time without the agents array changing identity
  private runtime = inject(AgentRuntimeService);
  readonly maxEl = computed(() =>
    Math.max(...this.agents().map((a) => this.runtime.elapsedFor(a.id)), 1),
  );

  elapsed(ag: Agent): number {
    return this.runtime.elapsedFor(ag.id);
  }
  color(ag: Agent): string {
    return STATUS_META[ag.status].color;
  }
  width(ag: Agent): number {
    return Math.max((this.elapsed(ag) / this.maxEl()) * 100, 3);
  }
  pct(ag: Agent): number {
    return Math.round(ag.progress * 100);
  }
}
