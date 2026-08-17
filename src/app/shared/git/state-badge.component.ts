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
      case "A": return "var(--vcs-added)";
      case "D": return "var(--vcs-deleted)";
      case "R": return "var(--vcs-renamed)";
      default:  return "var(--vcs-modified)"; // M and unknown
    }
  });

  readonly bg = computed(() => {
    switch (this.state()) {
      case "A": return "color-mix(in oklch, var(--vcs-added), transparent 88%)";
      case "D": return "transparent";
      case "R": return "color-mix(in oklch, var(--vcs-renamed), transparent 88%)";
      default:  return "color-mix(in oklch, var(--vcs-modified), transparent 88%)";
    }
  });
}
