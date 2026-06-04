import { ChangeDetectionStrategy, Component, input } from "@angular/core";
import { IconComponent } from "../shared/icon.component";

@Component({
  selector: "app-empty-state",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div style="flex:1;display:grid;place-items:center;padding:24px">
      <div style="text-align:center;color:var(--ink-4)">
        <app-icon [name]="icon()" size="lg" style="opacity:0.5" />
        <div style="font-size:11px;margin-top:8px;max-width:180px;line-height:1.5">{{ text() }}</div>
      </div>
    </div>
  `,
})
export class EmptyStateComponent {
  readonly icon = input.required<string>();
  readonly text = input.required<string>();
}
