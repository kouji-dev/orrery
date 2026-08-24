import {
  ChangeDetectionStrategy,
  Component,
  input,
} from "@angular/core";

/**
 * Compact `+N −N` add/delete stat pill. Shows only non-zero counts.
 * Matches the existing git-tab / diff-view styling exactly.
 */
@Component({
  selector: "app-add-del",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span
      class="tnum"
      style="
        display: flex;
        gap: var(--sp-2);
        font-size: var(--fs-meta);
        flex: none"
    >
      @if (add() > 0) {
        <span style="color: var(--code-add-ink)">+{{ add() }}</span>
      }
      @if (del() > 0) {
        <span style="color: var(--code-del-ink)">−{{ del() }}</span>
      }
    </span>
  `,
})
export class AddDelComponent {
  readonly add = input<number>(0);
  readonly del = input<number>(0);
}
