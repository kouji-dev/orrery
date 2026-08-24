import {
  ChangeDetectionStrategy,
  Component,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { DismissDirective } from "../shared/dismiss.directive";
import { IconComponent } from "../shared/icon.component";
import { TagChipComponent } from "./tag-chip.component";
import { TagPickerComponent } from "./tag-picker.component";
import { KjButtonComponent } from "@kouji-ui/components";

/**
 * Editable tag list: the attached chips (each removable) plus an "Add" trigger
 * that opens the search-or-create picker. Owns the popover open state and closes
 * it on outside-click / Escape. Emits the full new tag list on every change.
 */
@Component({
  selector: "app-tag-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  hostDirectives: [DismissDirective],
  imports: [IconComponent, TagChipComponent, TagPickerComponent, KjButtonComponent],
  template: `
    <div style="display:flex;flex-wrap:wrap;gap:var(--sp-3);align-items:center">
      @for (t of tags(); track t) {
        <app-tag [name]="t" [removable]="true" (remove)="onRemove($event)" />
      }
      <div style="position:relative">
        <kj-button kjVariant="outline" style="--kj-button-fg: var(--ink-3)" (click)="open.set(!open())">
          <app-icon size="md" name="plus" />{{ tags().length ? 'Add' : 'Add tag' }}
        </kj-button>
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

  readonly open = signal(false);

  constructor() {
    // outside mousedown / Escape → close (shared DismissDirective on the host)
    inject(DismissDirective).appDismiss.subscribe(() => {
      if (this.open()) this.open.set(false);
    });
  }

  onChanged(next: string[]) {
    this.tagsChange.emit(next);
  }

  onRemove(name: string) {
    this.tagsChange.emit(this.tags().filter((x) => x !== name));
  }
}
