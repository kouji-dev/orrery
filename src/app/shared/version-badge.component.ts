import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { KjBadgeComponent } from "@kouji-ui/components";
import { mix } from "../utils";
import { VersionService } from "./version.service";

/** Version + channel tag, reading the singleton {@link VersionService}.
 *
 *  - `pill` (top bar, beside the logo): a rounded badge — `vX.Y.Z` · `DEV`/`BETA`
 *    with a hairline divider, tinted by the channel color.
 *  - `chip` (footer): the bare version followed by a small bordered tag.
 *
 *  Both forms are kouji `<kj-badge>`s. The channel colour is per-build data,
 *  so it rides the badge's `bg`/`fg` inputs; the hairline border has no input,
 *  so the host publishes the colour as `--vb-color` and the skin in styles.css
 *  derives the border from it (kouji declares its knobs ON the inner
 *  `.kj-badge`, which component styles cannot reach).
 *
 *  When the version is unknown (plain `ng serve`, no Tauri) only the tag shows. */
@Component({
  selector: "app-version-badge",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjBadgeComponent],
  host: { "[style.--vb-color]": "ver.color" },
  template: `
    @let v = ver.version();
    @if (variant() === "pill") {
      <!-- design/app.html VersionBadge 1:1: mono pill tinted from the channel
           colour (sem-attn for DEV, ink-2 for BETA) — never the accent.
           Tint/border/typeface come from the --vb-color skin in styles.css. -->
      <kj-badge class="pill" [title]="tooltip()"
        style="--kj-badge-padding-x:7px;--kj-badge-padding-y:0">
        @if (v) {
          <span style="font-size:var(--fs-micro);color:var(--ink-2)">v{{ v }}</span>
          <span style="width:1px;height:9px;background:color-mix(in oklch, var(--vb-color), transparent 50%)"></span>
        }
        <span class="up" style="color:var(--vb-color);font-size:var(--fs-micro);font-weight:var(--fw-strong);">{{ ver.label }}</span>
      </kj-badge>
    } @else {
      <span [title]="tooltip()" style="display:flex;gap:var(--sp-3);align-items:center">
        @if (v) {
          <span>v{{ v }}</span>
        }
        <kj-badge class="tag" bg="transparent" [fg]="ver.color">
          <span class="up" style="font-weight:var(--fw-strong);">{{ ver.label }}</span>
        </kj-badge>
      </span>
    }
  `,
})
export class VersionBadgeComponent {
  readonly ver = inject(VersionService);
  readonly variant = input<"pill" | "chip">("pill");

  /** color-mix helper for the channel tint (exposed to the template). */
  protected readonly mix = mix;

  readonly tooltip = computed(() => {
    const v = this.ver.version();
    return "Orrery" + (v ? " v" + v : "") + " · " + (this.ver.isDev ? "development build" : "production (beta)");
  });
}
