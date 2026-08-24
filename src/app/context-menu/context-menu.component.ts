import { ChangeDetectionStrategy, Component, effect, inject, untracked } from "@angular/core";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { MenuItem } from "../models";
import { MenuPanelComponent } from "./menu-panel.component";
import { KjKbdComponent } from "@kouji-ui/components";
import { KjButton, KjDropdownMenuItem } from "@kouji-ui/core";
import { KjDividerComponent } from "@kouji-ui/components";

/**
 * The store-driven context menu: renders `ui.contextMenu()` MenuItems inside
 * the shared MenuPanelComponent chrome (positioning, clamping, dismissal).
 *
 * The chrome stays Orrery's because the menu is opened imperatively from the
 * store — kj's `[kjDropdownMenuTrigger]` is declarative and binds to a trigger
 * element that does not exist here, and its `'point'` mount uses `pointAt()`,
 * which places the panel at the click point but does NOT clamp it into the
 * viewport the way MenuPanelComponent does. What kj CAN supply without a
 * trigger is the a11y half, and that is what is wired below: `[kjDropdownMenu]`
 * registers the items and `[kjListNavigator]` in roving mode adds the WAI-ARIA
 * APG menu keyboard contract (Up/Down/Home/End/type-ahead, Enter/Space to
 * activate, roving tabindex) on top of role="menu" / role="menuitem". Before
 * this the menu was mouse-only.
 */
@Component({
  selector: "app-context-menu",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, MenuPanelComponent, KjKbdComponent, KjDividerComponent, KjDropdownMenuItem, KjButton],
  template: `
    @let menu = ui.contextMenu();
    @if (menu) {
      <!-- the kjDropdownMenu + kjListNavigator a11y model is hosted BY
           app-menu-panel (host directives) so the projected items resolve it;
           roving focus + menu semantics are opted into here -->
      <app-menu-panel
        [x]="menu.x"
        [y]="menu.y"
        (closed)="ui.closeMenu()"
        kjFocusMode="roving"
        role="menu"
        aria-label="Context menu"
      >
        @for (it of menu.items; track $index) {
          @if (it.sep) {
            <kj-divider style="--kj-divider-color:var(--hair);--kj-divider-spacing:var(--sp-2)" />
          } @else {
            <button kjButton
              kjDropdownMenuItem
              class="menu-item"
              [class.danger]="it.danger"
              [kjLabel]="it.label ?? ''"
              [kjShortcut]="it.kbd ?? null"
              [kjDisabled]="!!it.disabled"
              [disabled]="it.disabled"
              (kjSelect)="run(it)"
            >
              @if (it.icon) {
                <app-icon [name]="it.icon" size="sm" [color]="it.danger ? 'var(--st-blocked)' : (it.accent || 'var(--ink-3)')" style="flex:none" />
              }
              <span style="flex:1">{{ it.label }}</span>
              @if (it.kbd) { <kj-kbd>{{ it.kbd }}</kj-kbd> }
            </button>
          }
        }
      </app-menu-panel>
    }
  `,
})
export class ContextMenuComponent {
  readonly ui = inject(UiStore);

  /**
   * Roving focus pulls DOM focus into the menu on open (that is the point of
   * the APG contract). Nothing else would put it back, so the editor / terminal
   * that was focused when the menu opened would lose the caret on dismissal —
   * remember it and hand it back when the menu closes.
   */
  private returnFocus: HTMLElement | null = null;

  constructor() {
    effect(() => {
      const open = !!this.ui.contextMenu();
      untracked(() => {
        if (open) {
          this.returnFocus ??= (document.activeElement as HTMLElement | null) ?? null;
        } else if (this.returnFocus) {
          const el = this.returnFocus;
          this.returnFocus = null;
          // A menu action may have moved focus itself (a rename field, a
          // dialog). Only reclaim focus when it is still sitting in the menu
          // that is about to disappear.
          const now = document.activeElement as HTMLElement | null;
          const stranded = !now || now === document.body || !!now.closest(".menu-panel");
          if (stranded && el.isConnected) el.focus();
        }
      });
    });
  }

  run(it: MenuItem) {
    if (it.disabled) return;
    it.onClick?.();
    this.ui.closeMenu();
  }
}
