import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";

export interface SelectOption {
  value: string;
  label: string;
}

/**
 * Shared styled native <select> using the design's `.osel` look.
 *
 * <app-select [value]="model" [options]="opts" (valueChange)="setModel($event)" />
 */
@Component({
  selector: "app-select",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <select class="osel" [value]="value()" (change)="onChange($event)">
      @for (o of normalized(); track o.value) {
        <option [value]="o.value">{{ o.label }}</option>
      }
    </select>
  `,
})
export class SelectComponent {
  readonly value = input<string>("");
  readonly options = input<(string | SelectOption)[]>([]);
  readonly valueChange = output<string>();

  normalized(): SelectOption[] {
    return this.options().map((o) =>
      typeof o === "string" ? { value: o, label: o } : o,
    );
  }

  onChange(e: Event) {
    this.valueChange.emit((e.target as HTMLSelectElement).value);
  }
}
