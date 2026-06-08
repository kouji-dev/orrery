import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import { mix, TOOL_GLYPH, toolMeta } from "../utils";
import { TOOL_ICON } from "./tool-icons";

/** Square brand badge for an agent tool (claude / codex / cursor / gemini).
 *  Shows the real brand logo in the tool's accent color; falls back to a
 *  monogram glyph for unknown tools. */
@Component({
  selector: "app-tool-badge",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span [title]="meta().name" [style]="boxStyle()">
      @if (iconPath(); as d) {
        <svg [attr.width]="size() * 0.7" [attr.height]="size() * 0.7" viewBox="0 0 24 24" fill="currentColor" style="display:block">
          <path [attr.d]="d" />
        </svg>
      } @else {
        {{ glyph() }}
      }
    </span>
  `,
})
export class ToolBadgeComponent {
  readonly tool = input.required<string>();
  readonly size = input<number>(16);

  readonly meta = computed(() => toolMeta(this.tool()));
  readonly iconPath = computed(() => TOOL_ICON[this.tool()] ?? null);
  readonly glyph = computed(() => TOOL_GLYPH[this.tool()] ?? this.meta().short[0].toUpperCase());
  readonly boxStyle = computed(() => {
    const s = this.size();
    const accent = this.meta().accent;
    return {
      width: s + "px",
      height: s + "px",
      flex: "none",
      "border-radius": "4px",
      display: "grid",
      "place-items": "center",
      "font-size": s * 0.62 + "px",
      "line-height": "1",
      color: accent,
      background: mix(accent, 84),
      border: "1px solid " + mix(accent, 64),
    };
  });
}
