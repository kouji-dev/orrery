import {
  ChangeDetectionStrategy,
  Component,
  input,
  output,
  signal,
} from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { KjTagComponent, KjTagRemoveComponent } from "@kouji-ui/components";

/**
 * A single tag chip — snake_case label with a tag glyph, built on kouji's
 * `<kj-tag>` (+ `<kj-tag-remove>` for the removable shape, which also brings
 * the Delete-key remove affordance). Three modes (composable):
 *  - `clickable` → the chip toggles (emits `toggle`); used as a filter control.
 *  - `removable` → shows an × button (emits `remove`); used in editable lists.
 *  - `active`/`dim` → visual state (selected in a filter / muted).
 */
@Component({
  selector: "app-tag",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjTagComponent, KjTagRemoveComponent],
  template: `
    @let on = active();
    @let h = hovered() && clickable();
    <kj-tag
      [title]="name()"
      [kjTagLabel]="name()"
      (kjTagRemoved)="remove.emit(name())"
      (click)="onClick($event)"
      (mouseenter)="hovered.set(true)"
      (mouseleave)="hovered.set(false)"
      [style.cursor]="clickable() ? 'pointer' : 'default'"
      [style.padding]="removable() ? '3px 4px 3px 8px' : '3px 8px'"
      [style.color]="on ? 'var(--ui-ink)' : (dim() ? 'var(--ink-3)' : 'var(--ink-2)')"
      [style.border]="'1px solid ' + (on ? 'var(--ui-line)' : (h ? 'var(--hair-2)' : 'var(--hair)'))"
      [style.background]="on ? 'var(--ui-sel)' : (h ? 'var(--panel-3)' : 'var(--panel-2)')"
      class="chip"
      style="flex:none;max-width: round(calc(170px * var(--density)), 1px);font-family:var(--font-mono);line-height:1;letter-spacing:0.01em;user-select:none;transition:background .12s,border-color .12s,color .12s"
    >
      <app-icon size="sm" name="tag" style="flex:none;opacity:0.65" />
      <span class="trunc">{{ name() }}</span>
      @if (removable()) {
        <kj-tag-remove
          class="tag-x"
          [kjTagRemoveLabel]="'Remove ' + name()"
          (click)="$event.stopPropagation()"
          style="display:grid;place-items:center;cursor:pointer;color:var(--ink-4);padding:0;width:var(--sp-6);height:var(--sp-6);border-radius:4px;flex:none"
        >
          <app-icon size="sm" name="x" />
        </kj-tag-remove>
      }
    </kj-tag>
  `,
  styles: [`.tag-x:hover { color: var(--ink) !important; }`],
})
export class TagChipComponent {
  readonly name = input.required<string>();
  readonly active = input(false);
  readonly dim = input(false);
  readonly clickable = input(false);
  readonly removable = input(false);

  readonly toggle = output<string>();
  readonly remove = output<string>();

  readonly hovered = signal(false);

  onClick(e: MouseEvent) {
    if (!this.clickable()) return;
    e.stopPropagation();
    this.toggle.emit(this.name());
  }
}
