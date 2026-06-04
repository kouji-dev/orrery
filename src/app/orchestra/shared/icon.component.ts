import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import { ICONS } from "../utils";

@Component({
  selector: "app-icon",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: "display:inline-flex;line-height:0", "[style.color]": "color() || null" },
  template: `
    <svg
      [attr.width]="dim()"
      [attr.height]="dim()"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.7"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      style="display:block;flex:none"
    >
      <path [attr.d]="path()" />
    </svg>
  `,
})
export class IconComponent {
  readonly name = input.required<string>();
  readonly size = input<"sm" | "lg" | "md">("md");
  /** optional explicit pixel size, overrides `size` */
  readonly px = input<number | null>(null);
  readonly color = input<string | null>(null);

  readonly path = computed(() => ICONS[this.name()] ?? ICONS["dots"]);
  readonly dim = computed(() => {
    const p = this.px();
    if (p != null) return p;
    return this.size() === "sm" ? 13 : this.size() === "lg" ? 18 : 15;
  });
}
