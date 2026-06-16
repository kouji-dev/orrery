import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  HostListener,
  computed,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { IconComponent } from "../shared/icon.component";

/**
 * Header filter dropdown: multi-select tags (matches ANY of the selected) with
 * per-tag usage counts and a clear action. Owns its open state + outside-click.
 */
@Component({
  selector: "app-tag-filter",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let n = selected().length;
    <div style="position:relative">
      <button
        type="button"
        class="btn ghost-hair"
        (click)="open.set(!open())"
        [style.font-size]="'11.5px'"
        [style.color]="n ? 'var(--accent)' : 'var(--ink-2)'"
        [style.border-color]="n ? 'color-mix(in oklch, var(--accent), transparent 58%)' : 'var(--hair)'"
      >
        <app-icon name="tag" size="sm" [color]="n ? 'var(--accent)' : 'var(--ink-3)'" />
        {{ n ? n + ' tag' + (n > 1 ? 's' : '') : 'Tags' }}
        <app-icon name="chevronD" size="sm" color="var(--ink-4)" />
      </button>
      @if (open()) {
        <div
          class="surface"
          (click)="$event.stopPropagation()"
          style="position:absolute;top:100%;right:0;margin-top:6px;z-index:20;padding:6px;min-width:214px;box-shadow:var(--shadow)"
        >
          @if (!allTags().length) {
            <div style="padding:9px 10px;font-size:11px;color:var(--ink-4)">No tags yet</div>
          }
          @if (n > 0) {
            <button type="button" class="tag-row" (click)="clear()" style="color:var(--ink-3)">
              <app-icon name="x" size="sm" style="flex:none" />Clear filter<span class="tag-count">{{ n }} on</span>
            </button>
          }
          @if (n > 0 && allTags().length > 0) {
            <div style="height:1px;background:var(--hair);margin:5px 2px"></div>
          }
          <div class="scroll-y" style="max-height:280px;overflow-y:auto;display:flex;flex-direction:column;gap:2px">
            @for (t of allTags(); track t) {
              @let isOn = selected().includes(t);
              <button
                type="button"
                class="tag-row"
                (click)="toggle(t)"
                [style.color]="isOn ? 'var(--ink)' : null"
              >
                <span class="tag-check" [class.on]="isOn">
                  @if (isOn) { <app-icon name="check" size="sm" [px]="10" /> }
                </span>
                <span class="tnum">{{ t }}</span>
                <span class="tag-count">{{ countOf(t) }}</span>
              </button>
            }
          </div>
        </div>
      }
    </div>
  `,
})
export class TagFilterComponent {
  readonly allTags = input<string[]>([]);
  readonly counts = input<Record<string, number>>({});
  readonly selected = input<string[]>([]);

  readonly selectedChange = output<string[]>();

  private readonly host = inject(ElementRef<HTMLElement>);
  readonly open = signal(false);

  @HostListener("document:mousedown", ["$event"])
  onDocMouseDown(e: MouseEvent) {
    if (this.open() && !this.host.nativeElement.contains(e.target as Node)) {
      this.open.set(false);
    }
  }

  @HostListener("document:keydown.escape")
  onEscape() {
    if (this.open()) this.open.set(false);
  }

  readonly countMap = computed(() => this.counts());

  countOf(t: string): number {
    return this.countMap()[t] ?? 0;
  }

  toggle(t: string) {
    const sel = this.selected();
    this.selectedChange.emit(
      sel.includes(t) ? sel.filter((x) => x !== t) : [...sel, t],
    );
  }

  clear() {
    this.selectedChange.emit([]);
  }
}
