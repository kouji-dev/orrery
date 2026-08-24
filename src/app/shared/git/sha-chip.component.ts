import {
  ChangeDetectionStrategy,
  Component,
  computed,
  input,
} from "@angular/core";
import { KjBadgeComponent } from "@kouji-ui/components";

/**
 * Short-sha chip.
 *
 * Built on kouji's `<kj-badge>`: a sha chip is a label pill, which is exactly
 * what a badge is. `dim` is per-instance data, not a design variant, so the
 * muted ink rides the badge's `fg` input — kouji applies `bg`/`fg` as inline
 * styles, so they win per element.
 *
 * The chip skin (mono face, full pill, hairline, hover) lives in styles.css:
 * it dresses kouji's inner `.kj-badge` span, which sits in kouji's own view and
 * carries no scope attribute, so component styles cannot reach it.
 */
@Component({
  selector: "app-sha-chip",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjBadgeComponent],
  template: `
    <kj-badge
      bg="var(--panel-2)"
      [fg]="dim() ? 'var(--ink-3)' : 'var(--ink-2)'"
    >{{ short() }}</kj-badge>
  `,
})
export class ShaChipComponent {
  /** The commit sha. NOT `input.required`: a required signal input read from a
   *  computed during creation throws NG0950 under raw vitest JIT — the same
   *  constraint that keeps several components on @Input() decorators. Every
   *  call site binds it; the empty default only affects that test environment. */
  readonly sha = input<string>("");
  /** When true the sha text is rendered in muted ink (var(--ink-3)). */
  readonly dim = input<boolean>(false);

  readonly short = computed(() => this.sha().slice(0, 7));
}
