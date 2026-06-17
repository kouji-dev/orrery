import {
  ChangeDetectionStrategy,
  Component,
  computed,
  input,
} from "@angular/core";

/**
 * Initials chip for a commit author. Derives a 2-char abbreviation and a
 * stable accent hue from the author name/email — no global author registry.
 */
@Component({
  selector: "app-author-avatar",
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span
      [title]="author()"
      [style.width.px]="size()"
      [style.height.px]="size()"
      [style.font-size.px]="size() * 0.44"
      [style.color]="color()"
      [style.background]="bg()"
      [style.border]="'1px solid ' + borderColor()"
      style="
        flex: none;
        border-radius: 50%;
        display: grid;
        place-items: center;
        font-weight: 700;
        line-height: 1;
        user-select: none;
      "
    >{{ initials() }}</span>
  `,
})
export class AuthorAvatarComponent {
  readonly author = input.required<string>();
  /** Diameter in px — uses a density-token-driven default of var(--ctl-h-sm). */
  readonly size = input<number>(20);

  readonly initials = computed(() => deriveInitials(this.author()));
  readonly color = computed(() => deriveColor(this.author()));
  readonly bg = computed(() => `color-mix(in oklch, ${this.color()}, transparent 84%)`);
  readonly borderColor = computed(() => `color-mix(in oklch, ${this.color()}, transparent 60%)`);
}

/** Build 2-char initials: first letter of first + last word, uppercased. */
function deriveInitials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return "?";
  const first = parts[0][0] ?? "";
  const last = parts.length > 1 ? (parts[parts.length - 1][0] ?? "") : "";
  return (first + last).toUpperCase() || "?";
}

/** Derive a stable hsl color from a hash of the author string. */
function deriveColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = (hash * 31 + name.charCodeAt(i)) >>> 0;
  }
  // Map hash to a perceptually distinct hue while avoiding reds near status colors.
  const hue = (hash % 300 + 30) % 360; // 30–330° avoids pure reds
  return `hsl(${hue}, 65%, 62%)`;
}
