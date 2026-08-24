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
    if (!this.el.nativeElement.contains(e.target as Node)) this.appDismiss.emit();
  }
}
