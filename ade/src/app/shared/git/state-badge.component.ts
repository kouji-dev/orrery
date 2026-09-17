import {
  ChangeDetectionStrategy,
  Component,
  computed,
  input,
} from "@angular/core";
import { KjBadgeComponent } from "@kouji-ui/components";

/**
 * File-state badge: A (added), M (modified), D (deleted), R (renamed).
 *
 * Built on kouji's `<kj-badge>`: per-state colours are data, not design
 * variants, so they ride the badge's `bg`/`fg` inputs (kouji applies them
 * inline). The square silhouette lives in styles.css
 * (`app-state-badge .kj-badge`) — kouji's inner span carries no Angular scope
 * attribute, so component styles cannot reach it.
 */
@Component({
  selector: "app-state-badge",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjBadgeComponent],
  template: `
    <kj-badge [bg]="bg()" [fg]="ink()">{{ state() }}</kj-badge>
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
