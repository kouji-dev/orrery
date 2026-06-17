import {
  ChangeDetectionStrategy,
  Component,
  computed,
  input,
} from "@angular/core";

/**
 * File-state badge: A (added), M (modified), D (deleted), R (renamed).
 * Uses design-system status tokens — no hardcoded px colours.
 */
@Component({
  selector: "app-state-badge",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span
      [style.color]="ink()"
      [style.background]="bg()"
      style="
        flex: none;
        width: var(--sp-6);
        height: var(--sp-6);
        border-radius: 3px;
        display: grid;
        place-items: center;
        font-size: var(--fs-2xs);
        font-weight: 700;
        user-select: none;
      "
    >{{ state() }}</span>
  `,
})
export class StateBadgeComponent {
  readonly state = input.required<string>();

  readonly ink = computed(() => {
    switch (this.state()) {
      case "A": return "var(--code-add-ink)";
      case "D": return "var(--code-del-ink)";
      case "R": return "var(--st-waiting)";
      default:  return "var(--accent-2)"; // M and unknown
    }
  });

  readonly bg = computed(() => {
    switch (this.state()) {
      case "A": return "var(--code-add-bg)";
      case "D": return "var(--code-del-bg)";
      case "R": return "color-mix(in oklch, var(--st-waiting), transparent 86%)";
      default:  return "color-mix(in oklch, var(--accent-2), transparent 86%)";
    }
  });
}
