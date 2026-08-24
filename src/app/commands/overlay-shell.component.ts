import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  input,
  output,
  viewChild,
} from "@angular/core";
import { KjKbdComponent } from "@kouji-ui/components";

/** Segments of `text` with matched chars flagged (fuzzy highlight rendering). */
export function fzSegments(text: string, idx: number[]): { t: string; hit: boolean }[] {
  if (!idx.length) return [{ t: text, hit: false }];
  const set = new Set(idx);
  const out: { t: string; hit: boolean }[] = [];
  for (let i = 0; i < text.length; i++) {
    const hit = set.has(i);
    const last = out[out.length - 1];
    if (last && last.hit === hit) last.t += text[i];
    else out.push({ t: text[i], hit });
  }
  return out;
}

/** Modal chrome shared by every command overlay: dimmed backdrop, centered
 *  surface, click-outside close. Mirrors OverlayShell in the design reference
 *  (design/orrery/project/commands.jsx). */
@Component({
  selector: "app-overlay-shell",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [],
  template: `
    <div
      class="scrim"
      style="z-index:80;backdrop-filter:blur(2px);align-items:start;padding:0 var(--sp-8)"
      (mousedown)="onBackdrop($event)"
    >
      <div
        #box
        class="surface rise"
        role="dialog"
        [attr.aria-label]="label()"
        [style.width]="cssWidth()"
        [style.margin-top]="top()"
        style="max-width:100%;box-shadow:var(--shadow);overflow:hidden;display:flex;flex-direction:column;max-height:72vh"
      >
        <ng-content />
      </div>
    </div>
  `,
})
export class OverlayShellComponent {
  /** Nominal width in px at regular density. */
  readonly width = input(640);

  /**
   * Density-scaled width. A palette sized in frozen pixels keeps its box while
   * its contents grow with the type ramp, which is how the tweaks panel ended
   * up hiding its third option behind `overflow: hidden`. Scaling the box with
   * the same knob keeps the panel:text ratio constant at every density.
   */
  protected readonly cssWidth = computed(
    () => `round(calc(${this.width()}px * var(--density)), 1px)`,
  );
  readonly top = input("12vh");
  readonly label = input("");
  readonly closed = output<void>();
  private box = viewChild.required<ElementRef<HTMLElement>>("box");

  onBackdrop(e: MouseEvent) {
    if (!this.box().nativeElement.contains(e.target as Node)) this.closed.emit();
  }
}

/** Footer hint strip ("↑↓ navigate · ⏎ run · esc close"). */
@Component({
  imports: [KjKbdComponent],
  selector: "app-overlay-footer",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div style="display:flex;align-items:center;gap:var(--sp-6);padding:var(--sp-3) var(--sp-7);border-top:1px solid var(--hair);background:var(--panel-2);flex:none;font-size:var(--fs-meta);color:var(--ink-4)">
      @for (h of hints(); track h[0]) {
        <span style="display:flex;gap:var(--sp-2);align-items:center"><kj-kbd>{{ h[0] }}</kj-kbd>{{ h[1] }}</span>
      }
    </div>
  `,
})
export class OverlayFooterComponent {
  readonly hints = input<[string, string][]>([]);
}
