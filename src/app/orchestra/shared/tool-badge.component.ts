import { ChangeDetectionStrategy, Component, computed, input } from "@angular/core";
import { mix, TOOL_GLYPH, toolMeta } from "../utils";

/** Square monogram glyph for an agent tool (claude/codex/cursor/gemini). */
@Component({
  selector: "app-tool-badge",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span [title]="meta().name" [style]="boxStyle()">{{ glyph() }}</span>
  `,
})
export class ToolBadgeComponent {
  readonly tool = input.required<string>();
  readonly size = input<number>(16);

  readonly meta = computed(() => toolMeta(this.tool()));
  readonly glyph = computed(
    () => TOOL_GLYPH[this.tool()] ?? this.meta().short[0].toUpperCase(),
  );
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
