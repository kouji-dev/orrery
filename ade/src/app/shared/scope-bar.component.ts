import { ChangeDetectionStrategy, Component, input } from "@angular/core";
import { KjButtonComponent } from "@kouji-ui/components";
import { IconComponent } from "./icon.component";
import { ScopeSelection } from "./scope";
import { SelectComponent } from "./select.component";

/**
 * The "scope" cluster: project + worktree (+ optional breadth) selects over a
 * `ScopeSelection`, plus the follow-back button.
 *
 * It takes the model as an INPUT rather than injecting one, because the two
 * consumers disagree about lifetime on purpose — the dock owns a panel-local
 * scope, the lookup overlays share the root `LookupScopeStore` — and the bar
 * must not decide that for them.
 */
@Component({
  selector: "app-scope-bar",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, SelectComponent, KjButtonComponent],
  template: `
    @let s = scope();
    <span class="up" style="color:var(--ink-4);flex:none">scope</span>
    @if (s.project(); as p) {
      <span style="display:flex;align-items:center;gap:var(--sp-2);flex:none">
        <app-icon size="md" [name]="p.icon" [color]="p.color" />
        <app-select
          title="Project the panels read from"
          size="xs"
          [value]="p.id"
          [options]="s.projectOptions()"
          (valueChange)="s.pickProject($event)"
          style="width: round(calc(152px * var(--density)), 1px)"
        />
      </span>
    }
    <app-select
      title="Worktree the panel reads from"
      size="xs"
      [value]="s.agent()?.id ?? ''"
      [options]="s.agentOptions()"
      (valueChange)="s.pickAgent($event)"
      style="width: round(calc(150px * var(--density)), 1px);flex:none"
    />
    @if (showKind()) {
      <app-select
        title="How wide the search runs"
        size="xs"
        [value]="s.kind()"
        [options]="s.kindOptions"
        (valueChange)="s.kind.set($any($event))"
        style="width: round(calc(136px * var(--density)), 1px);flex:none"
      />
    }
    @if (showFollow() && s.outOfSync()) {
      <kj-button kjVariant="toolbar" [title]="'Follow the focused agent · ' + s.focus()!.name" (click)="s.follow()">
        <app-icon size="md" name="link" />{{ s.focus()!.name }}
      </kj-button>
    }
  `,
  host: {
    style: "display:flex;align-items:center;gap:var(--sp-3)",
  },
})
export class ScopeBarComponent {
  readonly scope = input.required<ScopeSelection>();
  /** Show the breadth select (worktree / project / all) — only the panels that
   *  actually search wider ask for it. */
  readonly showKind = input(false);
  readonly showFollow = input(true);
}
