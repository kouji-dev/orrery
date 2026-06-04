import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import { AgentStatus } from "../models";
import { mix, STATUS_META } from "../utils";
import { StatusDotComponent } from "./status-dot.component";

@Component({
  selector: "app-status-pill",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [StatusDotComponent],
  template: `
    <span class="chip" [style]="pillStyle()">
      <app-status-dot [status]="status()" />
      <span class="up" style="font-size:9.5px;letter-spacing:0.1em">{{ meta().label }}</span>
    </span>
  `,
})
export class StatusPillComponent {
  readonly status = input.required<AgentStatus>();
  readonly filled = input<boolean>(false);

  readonly meta = computed(() => STATUS_META[this.status()]);
  readonly pillStyle = computed(() => {
    const m = this.meta();
    return this.filled()
      ? { color: m.color, borderColor: mix(m.color, 60), background: mix(m.color, 88) }
      : { color: m.color };
  });
}
