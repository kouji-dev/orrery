import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { Agent } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { UiStore } from "../ui/ui.store";
import { StatusDotComponent } from "../shared/status-dot.component";
import { fmtDur, mix, STATUS_META } from "../utils";
import { KjProgressBarComponent } from "@kouji-ui/components";

@Component({
  selector: "app-timeline-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [StatusDotComponent, KjProgressBarComponent],
  template: `
    <div style="padding:var(--sp-7);display:flex;flex-direction:column;gap:var(--sp-1)">
      <div class="up" style="display:grid;grid-template-columns:150px 1fr 70px;gap:var(--sp-6);padding:0 var(--sp-2) var(--sp-4);">
        <span style="color:var(--ink-3)">Agent</span>
        <span style="color:var(--ink-3)">Elapsed · progress</span>
        <span style="color:var(--ink-3);text-align:right">Commits</span>
      </div>
      @for (ag of agents(); track ag.id) {
        @let w = width(ag);
        <div
          class="row-hover"
          (click)="ui.openAgent(ag.id)"
          style="display:grid;grid-template-columns:150px 1fr 70px;gap:var(--sp-6);align-items:center;padding:var(--sp-4) var(--sp-2);cursor:pointer;border-radius:var(--r-sm);border-bottom:1px solid var(--hair)"
        >
          <div style="display:flex;align-items:center;gap:var(--sp-3);min-width:0">
            <app-status-dot [status]="ag.status" />
            <h4 class="trunc">{{ ag.name }}</h4>
          </div>
          <div style="position:relative;height:var(--ctl-h-sm);display:flex;align-items:center">
            <div
              [style.width]="w + '%'"
              [style.background]="mix(color(ag), 55)"
              [style.border]="'1px solid ' + color(ag)"
              style="position:absolute;left:0;height:var(--sp-4);border-radius:5px;overflow:hidden"
            >
              <kj-progress-bar
                [kjValue]="pct(ag)"
                kjAriaLabel="Agent progress"
                [style.--kj-progress-bar-fill]="'color-mix(in oklch,' + color(ag) + ',transparent 50%)'"
                style="--kj-progress-bar-track:transparent;--kj-progress-bar-radius:0;--kj-progress-bar-height:100%"
              />
              @if (ag.status === 'running') {
                <div class="activity" style="position:absolute;inset:0;background:transparent"></div>
              }
            </div>
            <span class="tnum" [style.left]="'calc(' + w + '% + 8px)'" style="position:absolute;color:var(--ink-3);white-space:nowrap">
              {{ elapsed(ag) ? fmt(elapsed(ag)) : '—' }} · {{ pct(ag) }}%
            </span>
          </div>
          <span class="tnum" style="text-align:right;color:var(--ink-2)">{{ ag.commits }}</span>
        </div>
      }
    </div>
  `,
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
