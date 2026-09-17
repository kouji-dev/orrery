import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import { AgentStatus } from "../models";
import { mix, STATUS_META } from "../utils";
import { KjBadgeComponent } from "@kouji-ui/components";

/**
 * Agent status as a dot + label chip.
 *
 * Built on kouji's `<kj-badge>`: the status hue is per-agent and comes from
 * STATUS_META, so it is passed through the badge's `bg` / `fg` / `dotColor`
 * inputs rather than expressed as a variant. `filled` is the emphasised form
 * used where the pill has to carry on a busy surface.
 *
 * The public API is unchanged — call sites keep `[status]` and `[filled]`.
 */
@Component({
  selector: "app-status-pill",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjBadgeComponent],
  template: `
    <kj-badge
      [dot]="true"
      [dotColor]="meta().color"
      [fg]="meta().color"
      [bg]="filled() ? mix(meta().color, 88) : 'transparent'"
    >
      <span class="up">{{ meta().label }}</span>
    </kj-badge>
  `,
})
export class StatusPillComponent {
  readonly status = input.required<AgentStatus>();
  readonly filled = input<boolean>(false);

  readonly meta = computed(() => STATUS_META[this.status()]);

  /** Exposed to the template for the filled background tint. */
  protected readonly mix = mix;
}
