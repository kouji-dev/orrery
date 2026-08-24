import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  input,
  linkedSignal,
  output,
  viewChild,
} from "@angular/core";
import { KjDropdownMenu, KjListNavigator, KjTypeAhead } from "@kouji-ui/core";
import { DismissDirective } from "../shared/dismiss.directive";

/**
 * Shared chrome for every context-menu surface: the fixed elevated box at a
 * screen point, viewport clamping after layout, and dismissal (outside
 * mousedown / Escape → `closed`, via the shared DismissDirective). Content is
 * projected — the store-driven ContextMenuComponent renders MenuItems; the
 * file menus (file tree, diff file list) project their own
 * action/rename/delete flows. Styling comes from the global .menu-panel /
 * .menu-item / .menu-sep / .menu-label / .menu-input / .menu-row classes
 * (styles.css).
 *
 * The kouji a11y half lives HERE as host directives so every consumer that
 * projects `[kjDropdownMenuItem]` rows gets the WAI-ARIA APG menu keyboard
 * contract (Up/Down/Home/End/type-ahead, Enter/Space, roving tabindex) —
 * projected content resolves DI against this host, which a directive inside
 * this template could never provide. `kjFocusMode` / `kjOrientation` /
 * `kjActivateOnHover` are re-exposed, so a menu consumer opts into roving
 * focus with `kjFocusMode="roving"` (plus `role="menu"` + a label on the
 * element); consumers that project non-menu content (rename fields, custom
 * rows) register no items and the directives stay inert.
 */
@Component({
  selector: "app-menu-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DismissDirective],
  hostDirectives: [
    { directive: KjDropdownMenu },
    { directive: KjListNavigator, inputs: ["kjOrientation", "kjFocusMode", "kjActivateOnHover", "kjWrap"] },
  ],
  // KjListNavigator only does type-ahead when a KjTypeAhead is reachable; kj's
  // own KjDropdownMenuContent provides it, and we are not using that panel.
  providers: [KjTypeAhead],
  template: `
    <div
      #box
      class="menu-panel rise"
      [style.left.px]="pos().x"
      [style.top.px]="pos().y"
      (appDismiss)="closed.emit()"
      (mousedown)="$event.stopPropagation()"
      (contextmenu)="$event.preventDefault()"
    >
      <ng-content />
    </div>
  `,
})
export class MenuPanelComponent {
  readonly x = input.required<number>();
  readonly y = input.required<number>();
  /** Outside mousedown or Escape — the OWNER closes (it holds the open state). */
  readonly closed = output<void>();

  /** Panel position — re-seeds from the requested click point whenever the
   *  menu re-anchors, then the clamp below nudges it into the viewport. */
  readonly pos = linkedSignal<{ x: number; y: number }>(() => ({ x: this.x(), y: this.y() }));
  private box = viewChild.required<ElementRef<HTMLDivElement>>("box");

  constructor() {
    // clamp into the viewport after the menu is laid out
    afterRenderEffect(() => {
      const el = this.box().nativeElement;
      const r = el.getBoundingClientRect();
      let nx = this.x(),
        ny = this.y();
      if (nx + r.width > window.innerWidth - 8) nx = window.innerWidth - r.width - 8;
      if (ny + r.height > window.innerHeight - 8) ny = window.innerHeight - r.height - 8;
      const cur = this.pos();
      if (cur.x !== nx || cur.y !== ny) this.pos.set({ x: nx, y: ny });
    });
  }
}
