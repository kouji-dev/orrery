import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { VersionService } from "./version.service";

/** Version + channel tag, reading the singleton {@link VersionService}.
 *
 *  - `pill` (top bar, beside the logo): a rounded badge — `vX.Y.Z` · `DEV`/`BETA`
 *    with a hairline divider, tinted by the channel color.
 *  - `chip` (footer): the bare version followed by a small bordered tag.
 *
 *  When the version is unknown (plain `ng serve`, no Tauri) only the tag shows. */
@Component({
  selector: "app-version-badge",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let v = ver.version();
    @if (variant() === "pill") {
      <span
        [title]="tooltip()"
        [style.border]="'1px solid color-mix(in oklch, ' + ver.color + ', transparent 58%)'"
        [style.background]="'color-mix(in oklch, ' + ver.color + ', transparent 88%)'"
        style="display:inline-flex;align-items:center;gap:5px;height:17px;padding:0 7px;border-radius:999px;font-family:var(--font-mono);font-weight:500;letter-spacing:0.02em;white-space:nowrap"
      >
        @if (v) {
          <span style="font-size:10px;color:var(--ink-2)">v{{ v }}</span>
          <span [style.background]="'color-mix(in oklch, ' + ver.color + ', transparent 50%)'" style="width:1px;height:9px"></span>
        }
        <span class="up" [style.color]="ver.color" style="font-size:8.5px;font-weight:700;letter-spacing:0.12em">{{ ver.label }}</span>
      </span>
    } @else {
      <span [title]="tooltip()" style="display:flex;gap:6px;align-items:center">
        @if (v) {
          <span>v{{ v }}</span>
        }
        <span
          class="up"
          [style.color]="ver.color"
          [style.border]="'1px solid color-mix(in oklch, ' + ver.color + ', transparent 58%)'"
          style="font-size:8.5px;font-weight:700;letter-spacing:0.1em;border-radius:4px;padding:0 4px"
          >{{ ver.label }}</span
        >
      </span>
    }
  `,
})
export class VersionBadgeComponent {
  readonly ver = inject(VersionService);
  readonly variant = input<"pill" | "chip">("pill");

  readonly tooltip = computed(() => {
    const v = this.ver.version();
    return "Orrery" + (v ? " v" + v : "") + " · " + (this.ver.isDev ? "development build" : "production (beta)");
  });
}
