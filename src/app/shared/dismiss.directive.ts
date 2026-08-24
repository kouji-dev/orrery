import { Directive, ElementRef, inject, output } from "@angular/core";

/**
 * Shared light-dismiss behaviour for popovers / panels the app positions
 * itself: a mousedown anywhere outside the host, or Escape, emits
 * `(appDismiss)`. Replaces the outside-click + Escape `@HostListener` pair
 * that used to be copy-pasted into every floating panel component.
 *
 * <div class="panel" (appDismiss)="open.set(false)">…</div>
 */
@Directive({
  selector: "[appDismiss]",
  host: {
    "(document:mousedown)": "onDocMousedown($event)",
    "(document:keydown.escape)": "appDismiss.emit()",
  },
})
export class DismissDirective {
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef);
  readonly appDismiss = output<void>();

  onDocMousedown(e: MouseEvent) {
    const t = e.target as HTMLElement | null;
    if (this.el.nativeElement.contains(t)) return;
    // kouji portals its floating panels (confirm popups, dropdown menus,
    // selects) into body > .kj-overlay-container. One opened from INSIDE this
    // host is logically part of it: dismissing on its mousedown destroys the
    // panel — and the projected content that owns the handler — before the
    // click lands. That is why the file list's Delete confirm silently did
    // nothing: the menu (and with it the confirm popup) was gone by mouseup.
    if (t?.closest(".kj-overlay-container")) return;
    this.appDismiss.emit();
  }
}
