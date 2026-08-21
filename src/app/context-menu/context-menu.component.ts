import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { MenuItem } from "../models";
import { MenuPanelComponent } from "./menu-panel.component";

/** The store-driven context menu: renders `ui.contextMenu()` MenuItems inside
 *  the shared MenuPanelComponent chrome (positioning, clamping, dismissal). */
@Component({
  selector: "app-context-menu",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, MenuPanelComponent],
  template: `
    @let menu = ui.contextMenu();
    @if (menu) {
      <app-menu-panel [x]="menu.x" [y]="menu.y" (closed)="ui.closeMenu()">
        @for (it of menu.items; track $index) {
          @if (it.sep) {
            <div class="menu-sep"></div>
          } @else {
            <button
              class="menu-item"
              [class.danger]="it.danger"
              [disabled]="it.disabled"
              (click)="run(it)"
            >
              @if (it.icon) {
                <app-icon [name]="it.icon" size="sm" [color]="it.danger ? 'var(--st-blocked)' : (it.accent || 'var(--ink-3)')" style="flex:none" />
              }
              <span style="flex:1">{{ it.label }}</span>
              @if (it.kbd) { <span class="kbd">{{ it.kbd }}</span> }
            </button>
          }
        }
      </app-menu-panel>
    }
  `,
})
export class ContextMenuComponent {
  readonly ui = inject(UiStore);

  run(it: MenuItem) {
    if (it.disabled) return;
    it.onClick?.();
    this.ui.closeMenu();
  }
}
