import {
  afterNextRender,
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  inject,
  input,
  linkedSignal,
  output,
  signal,
  viewChild,
} from "@angular/core";
import { KjDropdownMenu, KjListNavigator, KjTypeAhead } from "@kouji-ui/core";
import { DismissDirective } from "../shared/dismiss.directive";
import { MenuPlacement, placeMenu } from "./place";

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
 *
 * WHY THE PANEL IS SIZED IN CSS (`width: max-content` on `.menu-panel`): a
 * position:fixed box with only `left` set is shrink-to-fit against the space
 * LEFT of the viewport edge. Opened near the right edge it therefore rendered
 * at its min-content width — labels wrapped, the box read as "cut off" — and
 * its width then depended on where the clamp had just moved it, so the one-shot
 * measurement below clamped against a width that no longer existed and the menu
 * ended up flush with the viewport edge. `max-content` makes the measurement
 * placement-independent, so the flip/clamp converges in a single pass.
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
      [style.top.px]="pos().bottom == null ? pos().y : null"
      [style.bottom.px]="pos().bottom"
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
  /** 'left' (default): x is the panel's left edge. 'right': x is its RIGHT
   *  edge — for dropdowns anchored to a control's right side. */
  readonly alignX = input<"left" | "right">("left");
  /** Dropup anchor: when the panel would overflow the viewport bottom, place
   *  its BOTTOM at this y (the anchor's top) instead of clamping over it. */
  readonly flipY = input<number | null>(null);
  /** Outside mousedown or Escape — the OWNER closes (it holds the open state). */
  readonly closed = output<void>();

  /** Panel position — re-seeds from the requested click point whenever the
   *  menu re-anchors, then the flip/clamp below nudges it into the viewport.
   *  bottom != null = dropup: pinned via CSS `bottom` so late content growth
   *  (font settle, async rows) keeps the panel's bottom on the anchor. */
  readonly pos = linkedSignal<MenuPlacement>(() => ({
    x: this.x(),
    y: this.y(),
    bottom: null,
  }));
  private box = viewChild.required<ElementRef<HTMLDivElement>>("box");
  /** Bumped whenever the measurement the placement rests on goes stale: the
   *  panel's own box resized (mode swap actions → rename → delete, async rows,
   *  font settle) or the window did. afterRenderEffect only re-runs for signals
   *  it read, and neither of those is one. */
  private readonly stale = signal(0);
  private readonly destroyRef = inject(DestroyRef);

  constructor() {
    // place into the viewport after the menu is laid out
    afterRenderEffect(() => {
      this.stale();
      const el = this.box().nativeElement;
      const r = el.getBoundingClientRect();
      const next = placeMenu(
        { x: this.x(), y: this.y(), alignX: this.alignX(), flipY: this.flipY() },
        { w: r.width, h: r.height },
        { w: window.innerWidth, h: window.innerHeight },
      );
      const cur = this.pos();
      if (cur.x !== next.x || cur.y !== next.y || cur.bottom !== next.bottom) this.pos.set(next);
    });

    afterNextRender(() => {
      const el = this.box().nativeElement;
      const bump = () => this.stale.update((n) => n + 1);

      // content swaps and font settle change the box without touching any
      // signal this component reads
      const ro = new ResizeObserver(bump);
      ro.observe(el);
      window.addEventListener("resize", bump);

      // Scroll-while-open: the menu is anchored to a POINT, and the row it was
      // opened from has just moved out from under it. Re-placing would leave it
      // pointing at the wrong row, so dismiss — the same thing every native menu
      // does. Registering here (after the first render) skips the scroll events
      // a freshly mounted pane fires. Scrolling INSIDE the panel is exempt: a
      // menu taller than the viewport scrolls its own items.
      const onScroll = (e: Event) => {
        const t = e.target as Node | null;
        if (t && (t === el || (t instanceof Node && el.contains(t)))) return;
        this.closed.emit();
      };
      window.addEventListener("scroll", onScroll, true);

      this.destroyRef.onDestroy(() => {
        ro.disconnect();
        window.removeEventListener("resize", bump);
        window.removeEventListener("scroll", onScroll, true);
      });
    });
  }
}
