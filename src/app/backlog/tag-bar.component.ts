import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  HostListener,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { TagChipComponent } from "./tag-chip.component";
import { TagPickerComponent } from "./tag-picker.component";

/**
 * Editable tag list: the attached chips (each removable) plus an "Add" trigger
 * that opens the search-or-create picker. Owns the popover open state and closes
 * it on outside-click / Escape. Emits the full new tag list on every change.
 */
@Component({
  selector: "app-tag-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, TagChipComponent, TagPickerComponent],
  template: `
    <div style="display:flex;flex-wrap:wrap;gap:6px;align-items:center">
      @for (t of tags(); track t) {
        <app-tag [name]="t" [removable]="true" (remove)="onRemove($event)" />
      }
      <div style="position:relative">
        <button
          type="button"
          class="btn ghost-hair"
          (click)="open.set(!open())"
          style="padding:3px 9px;font-size:10.5px;color:var(--ink-3);gap:5px"
        >
          <app-icon name="plus" size="sm" [px]="11" />{{ tags().length ? 'Add' : 'Add tag' }}
        </button>
        @if (open()) {
          <app-tag-picker
            [allTags]="allTags()"
            [attached]="tags()"
            [align]="align()"
            (changed)="onChanged($event)"
          />
        }
      </div>
    </div>
  `,
})
export class TagBarComponent {
  readonly allTags = input<string[]>([]);
  readonly tags = input<string[]>([]);
  readonly align = input<"left" | "right">("left");

  readonly tagsChange = output<string[]>();

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

  onChanged(next: string[]) {
    this.tagsChange.emit(next);
  }

  onRemove(name: string) {
    this.tagsChange.emit(this.tags().filter((x) => x !== name));
  }
}
