import { ChangeDetectionStrategy, Component, computed, input, output } from "@angular/core";
import { KjOptionComponent, KjSelectComponent } from "@kouji-ui/components";

export interface SelectOption {
  value: string;
  label: string;
}

/**
 * A titled cluster of options — the `<optgroup>` role, expressed as a heading
 * row inside kouji's listbox. `<kj-select>` projects its panel content, so a
 * non-option child simply rides along: it carries no `kjOption`, so the
 * roving focus and type-ahead skip it.
 */
export interface SelectGroup {
  label: string;
  options: (string | SelectOption)[];
}

/** Internal shape every input form is flattened to before rendering. */
interface Group {
  label: string | null;
  options: SelectOption[];
}

function isGroup(o: string | SelectOption | SelectGroup): o is SelectGroup {
  return typeof o !== "string" && "options" in o;
}

function toOption(o: string | SelectOption): SelectOption {
  return typeof o === "string" ? { value: o, label: o } : o;
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
 *
 * `options` accepts plain strings (value === label), `{ value, label }` pairs,
 * and `{ label, options }` groups — the three can be mixed in one list.
 */
@Component({
  selector: "app-select",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjOptionComponent, KjSelectComponent],
  template: `
    <kj-select
      kjSize="sm"
      [placeholder]="placeholder()"
      [value]="value()"
      (valueChange)="valueChange.emit($any($event))"
    >
      @for (g of groups(); track $index) {
        @if (g.label) {
          <div class="opt-group" role="presentation">{{ g.label }}</div>
        }
        @for (o of g.options; track o.value) {
          <!-- kjLabel is what the TRIGGER renders. Without it kouji falls back
               to String(value), which printed a project's uuid where its name
               belongs. The projected text stays for the option row itself. -->
          <kj-option [value]="o.value" [kjLabel]="o.label">{{ o.label }}</kj-option>
        }
      }
    </kj-select>
  `,
  styles: [
    `
      /* The listbox panel is portalled to the overlay container, but this node
         is declared HERE, so it keeps this component's scope attribute and the
         rule still lands. Same micro-label role as .up elsewhere. */
      .opt-group {
        padding: var(--sp-4) var(--sp-5) var(--sp-2);
        font: var(--fw-normal) var(--fs-badge) / 1.4 var(--font-ui);
        color: var(--ink-4);
        text-transform: uppercase;
        letter-spacing: 0.12em;
      }
      /* no rule above the first group — the panel's own padding is the gap */
      .opt-group:not(:first-child) {
        margin-top: var(--sp-3);
        border-top: 1px solid var(--hair);
      }
    `,
  ],
})
export class SelectComponent {
  readonly value = input<string>("");
  readonly options = input<(string | SelectOption | SelectGroup)[]>([]);
  readonly placeholder = input<string>("Select…");
  readonly valueChange = output<string>();

  /** One ungrouped run per contiguous stretch of loose options, so a list that
   *  mixes both ("None — start from scratch", then two groups) keeps its order. */
  readonly groups = computed<Group[]>(() => {
    const out: Group[] = [];
    for (const o of this.options()) {
      if (isGroup(o)) {
        out.push({ label: o.label, options: o.options.map(toOption) });
        continue;
      }
      const tail = out[out.length - 1];
      if (tail && tail.label === null) tail.options.push(toOption(o));
      else out.push({ label: null, options: [toOption(o)] });
    }
    return out;
  });
}
