import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { KjButtonComponent, KjCheckboxComponent, KjDividerComponent } from "@kouji-ui/components";
import { DismissDirective } from "../shared/dismiss.directive";

/**
 * Header filter dropdown: multi-select tags (matches ANY of the selected) with
 * per-tag usage counts and a clear action. Owns its open state + outside-click.
 */
@Component({
  selector: "app-tag-filter",
  changeDetection: ChangeDetectionStrategy.OnPush,
  hostDirectives: [DismissDirective],
  imports: [IconComponent, KjButtonComponent, KjCheckboxComponent, KjDividerComponent],
  template: `
    @let n = selected().length;
    <div style="position:relative">
      <kj-button kjVariant="outline" (click)="open.set(!open())" style="--kj-button-font-size: var(--fs-label)" [style.--kj-button-fg]="n ? 'var(--ui-ink)' : 'var(--ink-2)'" [style.--kj-button-border-color]="n ? 'var(--ui-line)' : 'var(--hair)'">
        <app-icon name="tag" size="sm" [color]="n ? 'var(--ui-ink)' : 'var(--ink-3)'" />
        {{ n ? n + ' tag' + (n > 1 ? 's' : '') : 'Tags' }}
        <app-icon name="chevronD" size="sm" color="var(--ink-4)" />
      </kj-button>
      @if (open()) {
        <div
          class="surface"
          (click)="$event.stopPropagation()"
          style="position:absolute;top:100%;right:0;margin-top:var(--sp-3);z-index:20;padding:var(--sp-3);min-width: round(calc(214px * var(--density)), 1px);box-shadow:var(--shadow)"
        >
          @if (!allTags().length) {
            <div style="padding:var(--sp-4) var(--sp-5);font-size:var(--fs-meta);color:var(--ink-4)">No tags yet</div>
          }
          @if (n > 0) {
            <button kjButton type="button" class="tag-row" (click)="clear()" style="color:var(--ink-3)">
              <app-icon name="x" size="sm" style="flex:none" />Clear filter<span class="tag-count">{{ n }} on</span>
            </button>
          }
          @if (n > 0 && allTags().length > 0) {
            <kj-divider style="--kj-divider-color:var(--hair);--kj-divider-spacing:var(--sp-2)" />
          }
          <div class="scroll-y" style="max-height: round(calc(280px * var(--density)), 1px);overflow-y:auto;display:flex;flex-direction:column;gap:var(--sp-1)">
            @for (t of allTags(); track t) {
              @let isOn = selected().includes(t);
              <button kjButton
                type="button"
                class="tag-row"
                (click)="toggle(t)"
                [style.color]="isOn ? 'var(--ink)' : null"
              >
                <!-- presentational: the row button owns the interaction, the
                     checkbox just mirrors state (pointer-events off avoids a
                     nested control) -->
                <kj-checkbox size="sm" [checked]="isOn" style="pointer-events:none;flex:none" />
                <span class="tnum">{{ t }}</span>
                <span class="tag-count">{{ countOf(t) }}</span>
              </button>
            }
          </div>
        </div>
      }
    </div>
  `,
})
export class TagFilterComponent {
  readonly allTags = input<string[]>([]);
  readonly counts = input<Record<string, number>>({});
  readonly selected = input<string[]>([]);

  readonly selectedChange = output<string[]>();

  readonly open = signal(false);

  constructor() {
    // outside mousedown / Escape → close (shared DismissDirective on the host)
    inject(DismissDirective).appDismiss.subscribe(() => {
      if (this.open()) this.open.set(false);
    });
  }

  readonly countMap = computed(() => this.counts());

  countOf(t: string): number {
    return this.countMap()[t] ?? 0;
  }

  toggle(t: string) {
    const sel = this.selected();
    this.selectedChange.emit(
      sel.includes(t) ? sel.filter((x) => x !== t) : [...sel, t],
    );
  }

  clear() {
    this.selectedChange.emit([]);
  }
}
