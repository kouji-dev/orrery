import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import { AgentStatus } from "../models";

@Component({
  selector: "app-status-dot",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span [class]="cls()"></span>`,
})
export class StatusDotComponent {
  readonly status = input.required<AgentStatus>();
  readonly cls = computed(() => "dot " + this.status());
}
