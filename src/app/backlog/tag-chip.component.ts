import {
  ChangeDetectionStrategy,
  Component,
  input,
  output,
  signal,
} from "@angular/core";
import { IconComponent } from "../shared/icon.component";

/**
 * A single tag chip — snake_case label with a tag glyph. Three modes (composable):
 *  - `clickable` → the chip toggles (emits `toggle`); used as a filter control.
 *  - `removable` → shows an × button (emits `remove`); used in editable lists.
 *  - `active`/`dim` → visual state (selected in a filter / muted).
 */
@Component({
  selector: "app-tag",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let on = active();
    @let h = hovered() && clickable();
    <span
      [title]="name()"
      (click)="onClick($event)"
      (mouseenter)="hovered.set(true)"
      (mouseleave)="hovered.set(false)"
      [style.cursor]="clickable() ? 'pointer' : 'default'"
      [style.padding]="removable() ? '3px 4px 3px 8px' : '3px 8px'"
      [style.color]="on ? 'var(--ui-ink)' : (dim() ? 'var(--ink-3)' : 'var(--ink-2)')"
      [style.border]="'1px solid ' + (on ? 'var(--ui-line)' : (h ? 'var(--hair-2)' : 'var(--hair)'))"
      [style.background]="on ? 'var(--ui-sel)' : (h ? 'var(--panel-3)' : 'var(--panel-2)')"
      style="display:inline-flex;align-items:center;gap:var(--sp-2);flex:none;max-width:170px;font-family:var(--font-mono);font-size:var(--fs-xs);line-height:1;letter-spacing:0.01em;border-radius:999px;white-space:nowrap;user-select:none;transition:background .12s,border-color .12s,color .12s"
    >
      <app-icon name="tag" size="sm" [px]="10" style="flex:none;opacity:0.65" />
      <span style="overflow:hidden;text-overflow:ellipsis">{{ name() }}</span>
      @if (removable()) {
        <button
          type="button"
          class="tag-x"
          [attr.aria-label]="'Remove ' + name()"
          (click)="onRemove($event)"
          style="display:grid;place-items:center;border:none;background:transparent;cursor:pointer;color:var(--ink-4);padding:0;width:var(--sp-6);height:var(--sp-6);border-radius:4px;flex:none"
        >
          <app-icon name="x" size="sm" [px]="10" />
        </button>
      }
    </span>
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

  onRemove(e: MouseEvent) {
    e.stopPropagation();
    this.remove.emit(this.name());
  }
}
