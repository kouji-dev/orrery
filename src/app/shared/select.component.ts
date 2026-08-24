import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";
import { KjOptionComponent, KjSelectComponent } from "@kouji-ui/components";

export interface SelectOption {
  value: string;
  label: string;
}

/**
 * Shared select control.
 *
 * Wraps kouji's `<kj-select>` rather than a native `<select>`: the native
 * element could not be styled to match the rest of the chrome without
 * `appearance: none` plus a hand-drawn caret, and it gave no keyboard model we
 * controlled. kouji's listbox brings roving focus and type-ahead of its own.
 *
 * The public API is unchanged, so every existing `<app-select>` call site keeps
 * working:
 *
 * <app-select [value]="model" [options]="opts" (valueChange)="setModel($event)" />
 */
@Component({
  selector: "app-select",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjOptionComponent, KjSelectComponent],
  template: `
    <kj-select
      kjSize="sm"
      [value]="value()"
      (valueChange)="valueChange.emit($any($event))"
    >
      @for (o of normalized(); track o.value) {
        <kj-option [value]="o.value">{{ o.label }}</kj-option>
      }
    </kj-select>
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
}
