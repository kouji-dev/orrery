import {
  ChangeDetectionStrategy,
  Component,
  computed,
  input,
  output,
} from "@angular/core";

/**
 * Short-sha chip. When `(chipClick)` is observed the chip becomes interactive
 * (pointer cursor, accent hover). `dim` renders the sha in a muted ink.
 */
@Component({
  selector: "app-sha-chip",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span
      class="chip tnum"
      [class.clickable]="hasListener()"
      [style.color]="dim() ? 'var(--ink-3)' : 'var(--ink-2)'"
      [style.cursor]="hasListener() ? 'pointer' : 'default'"
      style="
        font-size: var(--fs-2xs);
        padding: 0 var(--sp-3);
        letter-spacing: 0.02em;
      "
      (click)="hasListener() ? chipClick.emit(sha()) : null"
    >{{ short() }}</span>
  `,
  styles: [
    `
      .clickable:hover {
        color: var(--accent-2) !important;
        border-color: color-mix(in oklch, var(--accent-2), transparent 55%);
      }
    `,
  ],
})
export class ShaChipComponent {
  readonly sha = input.required<string>();
  /** When true the sha text is rendered in muted ink (var(--ink-3)). */
  readonly dim = input<boolean>(false);

  readonly chipClick = output<string>();

  /** Whether a consumer bound to (chipClick) — controls interactive styling. */
  readonly hasListener = computed(() => {
    // Angular signals-based outputs expose .observed
    return (this.chipClick as unknown as { observed: boolean }).observed;
  });

  readonly short = computed(() => this.sha().slice(0, 7));
}
