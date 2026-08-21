import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  effect,
  ElementRef,
  HostListener,
  input,
  output,
  signal,
  viewChild,
} from "@angular/core";

/**
 * Shared chrome for every context-menu surface: the fixed elevated box at a
 * screen point, viewport clamping after layout, and dismissal (outside
 * mousedown / Escape → `closed`). Content is projected — the store-driven
 * ContextMenuComponent renders MenuItems; the file menus (file tree, diff
 * file list) project their own action/rename/delete flows. Styling comes from
 * the global .menu-panel / .menu-item / .menu-sep / .menu-label / .menu-input
 * / .menu-row classes (styles.css).
 */
@Component({
  selector: "app-menu-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div
      #box
      class="menu-panel rise"
      [style.left.px]="pos().x"
      [style.top.px]="pos().y"
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

  readonly pos = signal<{ x: number; y: number }>({ x: 0, y: 0 });
  private box = viewChild.required<ElementRef<HTMLDivElement>>("box");

  constructor() {
    // seed from the requested click point (re-seeds when the menu re-anchors)
    effect(() => this.pos.set({ x: this.x(), y: this.y() }));
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

  // Inside clicks never reach the document — the panel stops mousedown
  // propagation — so any that does is an outside click.
  @HostListener("document:mousedown")
  onDocDown() {
    this.closed.emit();
  }

  @HostListener("document:keydown.escape")
  onEsc() {
    this.closed.emit();
  }
}
