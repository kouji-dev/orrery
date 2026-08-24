import {
  ChangeDetectionStrategy,
  Component,
  computed,
  input,
} from "@angular/core";
import { KjAvatarComponent } from "@kouji-ui/components";

/**
 * Initials chip for a commit author. Derives a 2-char abbreviation and a
 * stable accent hue from the author name/email — no global author registry.
 *
 * Built on kouji's `<kj-avatar>`, which owns the circle, the centring and the
 * image/fallback swap. Unlike most kouji components the avatar host IS the
 * painted box (it carries `.kj-avatar` itself, not `display: contents`), so the
 * per-author hue and the caller's pixel diameter can be set as inline styles
 * here instead of leaking into styles.css.
 *
 * `size` stays a px number — call sites pass 13–20px, far below kouji's
 * t-shirt scale, so `--kj-avatar-size` is driven directly rather than via the
 * `size` input.
 */
@Component({
  selector: "app-author-avatar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjAvatarComponent],
  template: `
    <kj-avatar
      [alt]="author()"
      [content]="initials()"
      [style.--kj-avatar-size]="px()"
      [style.--kj-avatar-bg]="bg()"
      [style.--kj-avatar-fg]="color()"
      [style.font-size.px]="size() * 0.44"
      [style.border]="'1px solid ' + borderColor()"
      style="flex: none; font-weight: var(--fw-strong); user-select: none"
    />
  `,
})
export class AuthorAvatarComponent {
  readonly author = input.required<string>();
  /** Diameter in px — uses a density-token-driven default of var(--ctl-h-sm). */
  readonly size = input<number>(20);

  readonly px = computed(() => this.size() + "px");
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
