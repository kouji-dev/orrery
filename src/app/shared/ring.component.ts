import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";

/** Circular progress ring (0..1). */
@Component({
  selector: "app-ring",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <svg [attr.width]="size()" [attr.height]="size()" style="transform:rotate(-90deg);display:block">
      <circle [attr.cx]="c()" [attr.cy]="c()" [attr.r]="r()" fill="none" stroke="var(--hair)" [attr.stroke-width]="stroke()" />
      <circle
        [attr.cx]="c()"
        [attr.cy]="c()"
        [attr.r]="r()"
        fill="none"
        [attr.stroke]="color() || 'var(--accent)'"
        [attr.stroke-width]="stroke()"
        stroke-linecap="round"
        [attr.stroke-dasharray]="circ()"
        [attr.stroke-dashoffset]="offset()"
        style="transition:stroke-dashoffset 0.6s ease"
      />
    </svg>
  `,
})
export class RingComponent {
  readonly value = input<number>(0);
  readonly size = input<number>(30);
  readonly stroke = input<number>(3);
  readonly color = input<string | null>(null);

  readonly r = computed(() => (this.size() - this.stroke()) / 2);
  readonly c = computed(() => this.size() / 2);
  readonly circ = computed(() => 2 * Math.PI * this.r());
  readonly offset = computed(() => this.circ() * (1 - this.value()));
}
