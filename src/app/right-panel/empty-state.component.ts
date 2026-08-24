import { ChangeDetectionStrategy, Component, input } from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import {
  KjEmptyStateComponent,
  KjEmptyStateDescriptionComponent,
  KjEmptyStateIconComponent,
} from "@kouji-ui/components";

@Component({
  selector: "app-empty-state",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjEmptyStateComponent, KjEmptyStateIconComponent, KjEmptyStateDescriptionComponent],
  template: `
    <kj-empty-state kjSize="sm">
      <kj-empty-state-icon><app-icon [name]="icon()" size="lg" style="opacity:0.5" /></kj-empty-state-icon>
      <kj-empty-state-description>{{ text() }}</kj-empty-state-description>
    </kj-empty-state>
  `,
  styles: [
    `
      :host {
        flex: 1;
        display: grid;
        place-items: center;
      }
      kj-empty-state {
        --kj-empty-state-max-width: round(calc(180px * var(--density)), 1px);
      }
    `,
  ],
})
export class EmptyStateComponent {
  readonly icon = input.required<string>();
  readonly text = input.required<string>();
}
