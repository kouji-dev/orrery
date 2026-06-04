import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import { IconComponent } from "./icon.component";

export type ButtonVariant = "ghost" | "ghost-hair" | "primary";

/**
 * Shared button atom. Renders the design's `.btn` styling with an optional
 * leading icon. Projected content is the label.
 *
 * <app-button variant="primary" icon="bolt" (act)="spawn()">Spawn</app-button>
 */
@Component({
  selector: "app-button",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <button
      type="button"
      [class]="cls()"
      [disabled]="disabled()"
      [style.justify-content]="block() ? 'center' : null"
      [style.flex]="block() ? '1' : null"
    >
      @if (icon()) {
        <app-icon [name]="icon()!" [size]="iconSize()" [color]="iconColor()" />
      }
      <ng-content />
    </button>
  `,
})
export class ButtonComponent {
  readonly variant = input<ButtonVariant>("ghost");
  readonly icon = input<string | null>(null);
  readonly iconSize = input<"sm" | "lg" | "md">("sm");
  readonly iconColor = input<string | null>(null);
  readonly disabled = input<boolean>(false);
  /** stretch + center — for full-width action buttons */
  readonly block = input<boolean>(false);

  readonly cls = computed(() => {
    const v = this.variant();
    return "btn" + (v === "ghost" ? "" : " " + v);
  });
}
