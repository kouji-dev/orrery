import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  inject,
  input,
  output,
  signal,
} from "@angular/core";

/**
 * The drag handle between a fixed side column and a fluid body — one recipe
 * for every master-detail split (working-tree diff, commit diff, compare
 * diff). It owns nothing but the gesture: the host binds the width it reads
 * back from `widthChange`, so the preference can live wherever it belongs
 * (UiStore, persisted with the workspace).
 *
 * Double-click emits `reset` — the host's cue to drop back to its default.
 * While dragging, `body.col-resizing` kills text selection and pins the
 * col-resize cursor window-wide, so a fast drag that outruns the pointer never
 * paints an I-beam over the diff.
 */
@Component({
  selector: "app-pane-resizer",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="grip"></span>`,
  host: {
    class: "pane-resizer",
    "[class.dragging]": "dragging()",
    role: "separator",
    "aria-orientation": "vertical",
    tabindex: "0",
    title: "Drag to resize · double-click to reset",
    "(pointerdown)": "startDrag($event)",
    "(dblclick)": "reset.emit()",
    "(keydown.arrowLeft)": "nudge(-16, $event)",
    "(keydown.arrowRight)": "nudge(16, $event)",
  },
})
export class PaneResizerComponent {
  private readonly host = inject(ElementRef<HTMLElement>);

  /** Current column width in px — the drag starts from this. */
  readonly width = input.required<number>();
  readonly min = input(160);
  readonly max = input(520);

  /** The clamped width for every pointer move (and arrow-key nudge). */
  readonly widthChange = output<number>();
  /** Double-click: the host drops back to its own default. */
  readonly reset = output<void>();

  readonly dragging = signal(false);
  private startX = 0;
  private startW = 0;

  startDrag(ev: PointerEvent): void {
    ev.preventDefault();
    this.dragging.set(true);
    document.body.classList.add("col-resizing");
    this.startX = ev.clientX;
    this.startW = this.width();
    const target = this.host.nativeElement as HTMLElement;
    target.setPointerCapture?.(ev.pointerId);
    const move = (e: PointerEvent) => this.emit(this.startW + (e.clientX - this.startX));
    const up = (e: PointerEvent) => {
      this.dragging.set(false);
      document.body.classList.remove("col-resizing");
      target.releasePointerCapture?.(e.pointerId);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** Keyboard path: the handle is focusable, so the split is resizable without
   *  a pointer at all. */
  nudge(by: number, ev: Event): void {
    ev.preventDefault();
    this.emit(this.width() + by);
  }

  private emit(next: number): void {
    this.widthChange.emit(Math.min(this.max(), Math.max(this.min(), Math.round(next))));
  }
}
