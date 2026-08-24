import { ChangeDetectionStrategy, Component, input } from "@angular/core";

@Component({
  selector: "app-stat-block",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div style="display:flex;flex-direction:column;gap:var(--sp-1);padding-right:var(--sp-8)">
      <div style="display:flex;align-items:baseline;gap:var(--sp-3)">
        <span class="disp tnum" [style.color]="color() || 'var(--ink)'" style="font-size:var(--fs-2xl);font-weight:var(--fw-medium);line-height:1">{{ n() }}</span>
        @if (pulse()) {
          <span class="dot running" [style.background]="color()"></span>
        }
      </div>
      <span class="up" style="color:var(--ink-3);white-space:nowrap">{{ label() }}</span>
    </div>
  `,
})
export class StatBlockComponent {
  readonly n = input.required<number>();
  readonly label = input.required<string>();
  readonly color = input<string | null>(null);
  readonly pulse = input<boolean>(false);
}
