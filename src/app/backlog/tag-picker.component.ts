import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  computed,
  input,
  output,
  signal,
  viewChild,
} from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { snakeTag } from "./tags.util";

/**
 * Presentational popover: search existing tags or create a new one. Parent owns
 * the open/close + outside-click; this just renders the input + rows and emits
 * the new attached list each time a tag is added.
 */
@Component({
  selector: "app-tag-picker",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div
      class="surface"
      (click)="$event.stopPropagation()"
      [style.left]="align() === 'right' ? null : '0'"
      [style.right]="align() === 'right' ? '0' : null"
      style="position:absolute;top:100%;margin-top:var(--sp-3);z-index:40;width:244px;padding:var(--sp-4);box-shadow:var(--shadow);display:flex;flex-direction:column;gap:var(--sp-4)"
    >
      <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-4);border-radius:var(--r-sm);background:var(--panel-2);border:1px solid var(--hair)">
        <app-icon name="search" size="sm" color="var(--ink-4)" style="flex:none" />
        <input
          #input
          [value]="q()"
          (input)="q.set($any($event.target).value)"
          (keydown.enter)="onEnter($event)"
          placeholder="Search or create a tag…"
          style="flex:1;min-width:0;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-sm)"
        />
      </div>

      @if (q() && norm() !== q().trim().toLowerCase()) {
        <div style="font-size:var(--fs-2xs);color:var(--ink-4);padding:0 var(--sp-1)">
          saves as <span class="tnum" style="color:var(--ink)">{{ norm() || '…' }}</span>
        </div>
      }

      <div class="scroll-y" style="display:flex;flex-direction:column;gap:var(--sp-1);max-height:208px;overflow-y:auto">
        @if (canCreate()) {
          <button type="button" class="tag-row" (click)="attach(norm())">
            <app-icon name="plus" size="sm" color="var(--ui-ink)" style="flex:none" />
            <span>Create</span><span class="tnum" style="color:var(--ui-ink)">{{ norm() }}</span>
          </button>
        }
        @for (t of matches(); track t) {
          <button type="button" class="tag-row" (click)="attach(t)">
            <app-icon name="tag" size="sm" color="var(--ink-4)" style="flex:none" />
            <span class="tnum">{{ t }}</span>
          </button>
        }
        @if (!matches().length && !canCreate()) {
          <div style="padding:var(--sp-4) var(--sp-4);font-size:var(--fs-xs);color:var(--ink-4)">
            {{ emptyHint() }}
          </div>
        }
      </div>
    </div>
  `,
})
export class TagPickerComponent implements AfterViewInit {
  readonly allTags = input<string[]>([]);
  readonly attached = input<string[]>([]);
  readonly align = input<"left" | "right">("left");

  readonly changed = output<string[]>();

  private readonly inputEl = viewChild<ElementRef<HTMLInputElement>>("input");
  readonly q = signal("");

  readonly norm = computed(() => snakeTag(this.q()));
  readonly available = computed(() =>
    this.allTags().filter((t) => !this.attached().includes(t)),
  );
  readonly matches = computed(() => {
    const n = this.norm();
    return n ? this.available().filter((t) => t.includes(n)) : this.available();
  });
  readonly canCreate = computed(() => {
    const n = this.norm();
    const exact = this.allTags().includes(n) || this.attached().includes(n);
    return !!n && !exact;
  });
  readonly emptyHint = computed(() => {
    if (this.available().length) return "no matches";
    return this.attached().length ? "all tags attached" : "no tags yet — type to create one";
  });

  ngAfterViewInit() {
    this.inputEl()?.nativeElement.focus();
  }

  attach(t: string) {
    if (!t || this.attached().includes(t)) return;
    this.changed.emit([...this.attached(), t]);
    this.q.set("");
    this.inputEl()?.nativeElement.focus();
  }

  onEnter(e: Event) {
    e.preventDefault();
    const n = this.norm();
    const m = this.matches();
    if (m.includes(n)) this.attach(n);
    else if (this.canCreate()) this.attach(n);
    else if (m.length) this.attach(m[0]);
  }
}
