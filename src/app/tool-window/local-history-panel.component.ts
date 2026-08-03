import { ChangeDetectionStrategy, Component, input } from "@angular/core";
import { Agent } from "../models";
import { IconComponent } from "../shared/icon.component";

const B44 = "arrives with B4.4 (local-history snapshots)";

/**
 * Bottom-dock "Local History" panel (design git-panels.jsx LocalHistoryPanel).
 * Snapshot capture (agent turn / external edit / pre-command / commit points)
 * is the B4.4 watcher feature — until it lands the panel renders the
 * design-faithful two-column chrome with an honest empty state and disabled
 * actions. No synthesized snapshots.
 */
@Component({
  selector: "app-local-history-panel",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @if (!agent()) {
      <div style="padding:var(--sp-8);font-size:var(--fs-sm);color:var(--ink-4)">
        select an agent to see its worktree history
      </div>
    } @else {
      <div style="display:grid;grid-template-columns:280px 1fr;min-height:0;flex:1">
        <!-- snapshots -->
        <div style="display:flex;flex-direction:column;min-height:0;border-right:1px solid var(--hair)">
          <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
            <app-icon name="clock" size="sm" color="var(--accent)" />
            <span class="up" style="font-size:var(--fs-3xs);color:var(--ink-3)">Snapshots</span>
            <span class="tnum" style="margin-left:auto;font-size:var(--fs-2xs);color:var(--ink-4)">0</span>
          </div>
          <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">
            <div style="padding:var(--sp-6);font-size:var(--fs-sm);color:var(--ink-3)">no snapshots yet</div>
            <div style="padding:0 var(--sp-6);font-size:var(--fs-2xs);color:var(--ink-4);line-height:1.5;text-wrap:pretty">
              worktree snapshots — captured on agent turns, external edits, pre-command guards and commits — {{ b44 }}
            </div>
          </div>
          <div class="tnum" style="padding:var(--sp-3) var(--sp-6);border-top:1px solid var(--hair);font-size:var(--fs-3xs);color:var(--ink-4);flex:none">
            snapshot capture {{ b44 }}
          </div>
        </div>

        <!-- detail -->
        <div style="display:flex;flex-direction:column;min-height:0">
          <div style="display:flex;align-items:center;gap:var(--sp-4);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair);flex:none">
            <span style="font-size:var(--fs-sm);color:var(--ink-4)">{{ agent()!.name }} · {{ agent()!.branch }}</span>
            <div style="margin-left:auto;display:flex;gap:var(--sp-3)">
              <button class="btn ghost-hair" disabled [title]="b44" style="font-size:var(--fs-sm)">
                <app-icon name="diff" size="sm" />Show diff
              </button>
              <button class="btn ghost-hair" disabled [title]="b44" style="font-size:var(--fs-sm);color:var(--st-blocked)">
                <app-icon name="discard" size="sm" />Revert to this point
              </button>
            </div>
          </div>
          <div style="flex:1;display:grid;place-items:center;padding:var(--sp-8)">
            <div style="text-align:center">
              <app-icon name="clock" size="lg" color="var(--hair-2)" />
              <div style="font-size:var(--fs-sm);color:var(--ink-3);margin-top:var(--sp-4)">Local history {{ b44 }}</div>
              <div style="font-size:var(--fs-xs);color:var(--ink-4);margin-top:var(--sp-1);text-wrap:pretty">
                every agent turn and external edit will be revertable from here
              </div>
            </div>
          </div>
        </div>
      </div>
    }
  `,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
        min-width: 0;
      }
    `,
  ],
})
export class LocalHistoryPanelComponent {
  readonly agent = input<Agent | null>(null);
  readonly b44 = B44;
}
