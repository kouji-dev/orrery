import {
  ChangeDetectionStrategy,
  Component,
  computed,
  EventEmitter,
  inject,
  Input,
  Output,
  signal,
} from "@angular/core";
import { AGENT_TOOLS, effortLevelsFor } from "../data";
import { AgentTool } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { modelDiscoveryHint, ModelCatalogService } from "../agents/model-catalog.service";
import { SelectComponent, SelectGroup, SelectOption } from "./select.component";
import { ToolBadgeComponent } from "./tool-badge.component";
import { mix } from "../utils";
import {
  KjComboboxComponent,
  KjComboboxEmptyComponent,
  KjComboboxOptionComponent,
  KjButtonComponent,
  KjFieldComponent,
  KjFieldLabelComponent,
  KjSpinnerComponent,
  KjTabComponent,
  KjTabListComponent,
  KjTabsComponent,
} from "@kouji-ui/components";

/**
 * Tile order: runnable agents first, the rest after. Two filters rather than a
 * sort — a partition is stable BY CONSTRUCTION, so each group keeps its
 * AGENT_TOOLS declaration order and the row stays predictable instead of riding
 * on a comparator's tie-breaking (V8's sort is stable, but nothing in an `a-b`
 * comparator says so at the call site).
 *
 * While ANY probe is still out the declaration order stands unchanged. The
 * backend sweep answers tool by tool, so ordering during that window would walk
 * tiles sideways one probe at a time — under a cursor already hovering a tile,
 * the click would land on a different agent than the one aimed at. Holding
 * until the sweep settles trades that shuffle for a single, visible move.
 * Selection is held by id, never by position, so no caller can be disturbed by
 * the reorder.
 */
export function orderedTools(runtime: AgentRuntimeService): AgentTool[] {
  if (AGENT_TOOLS.some((t) => runtime.detectionPending(t.id))) return AGENT_TOOLS;
  const runnable = (id: string) =>
    runtime.toolAvailable(id) && runtime.detection(id)?.status !== "error";
  return [...AGENT_TOOLS.filter((t) => runnable(t.id)), ...AGENT_TOOLS.filter((t) => !runnable(t.id))];
}

/** What the picker actually offers: the PROBE when it reported anything, else
 *  the tool's curated catalog. cursor keeps a curated list for exactly this
 *  fallback; pi's is empty by design, so its picker is free-text only. */
export function modelChoicesFor(
  tool: AgentTool,
  catalog: ModelCatalogService,
): { id: string; label: string }[] {
  if (tool.dynamicModels) {
    const probed = catalog.models(tool.id);
    if (probed.length) return probed.map((id) => ({ id, label: id }));
  }
  return tool.models;
}

/** The tool's curated models as grouped picker options ("Latest" aliases,
 *  "Pinned versions"…), labels human, values the exact `--model` id. */
export function modelOptionsFor(tool: AgentTool): (SelectOption | SelectGroup)[] {
  const groups: { label: string; options: SelectOption[] }[] = [];
  for (const m of tool.models) {
    const label = m.group ?? "";
    let g = groups.find((x) => x.label === label);
    if (!g) groups.push((g = { label, options: [] }));
    g.options.push({ value: m.id, label: m.label });
  }
  // a single unnamed group is just a flat list
  return groups.length === 1 && !groups[0].label ? groups[0].options : groups;
}

/** Empty-state copy for the dynamic picker. One empty combobox used to stand
 *  for four unrelated situations — still probing, CLI not installed, probe
 *  failed, CLI fine but signed out — so it said nothing useful about any of
 *  them. `modelDiscoveryHint` names which one it is (shared with Settings). */
export function discoveryHintFor(
  tool: AgentTool,
  catalog: ModelCatalogService,
  runtime: AgentRuntimeService,
): string {
  return modelDiscoveryHint(tool, {
    probed: catalog.isProbed(tool.id),
    error: catalog.error(tool.id),
    empty: !catalog.models(tool.id).length,
    detection: runtime.detection(tool.id),
  });
}

/**
 * The agent's three launch choices — tool tiles, model picker, reasoning-effort
 * pills — as ONE controlled control group, shared by the spawn dialog and the
 * edit-agent dialog. It holds no state: every value comes in, every change goes
 * out, so each dialog keeps its own prefill rules (spawn resolves the settings
 * defaults, edit reads the agent's stored values) without either one having to
 * restate the markup that renders them.
 *
 * Decorator `@Input()`/`@Output()`, NOT the signal `input()`/`output()` forms
 * the rest of the app prefers: these dialogs' specs run under raw vitest, which
 * JIT-compiles the templates, and a JIT-compiled signal input is never bound
 * (NG0950) — the group would render with no tool and no model in every spec
 * that mounts a dialog. A setter into a signal gives the same reactivity.
 *
 * `:host { display: contents }` so the two blocks below land directly in the
 * dialog's own flex column and keep its gap; a box here would collapse the
 * spacing between the tiles and the Model row into one.
 *
 * The class names stay `spawn-*`: the spawn dialog is where this recipe was
 * written and where its rules are pinned, and renaming them would be churn
 * across two sheets and a spec for no behavioural gain.
 */
@Component({
  selector: "app-agent-tool-controls",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    SelectComponent,
    ToolBadgeComponent,
    KjButtonComponent,
    KjComboboxComponent,
    KjComboboxEmptyComponent,
    KjComboboxOptionComponent,
    KjFieldComponent,
    KjFieldLabelComponent,
    KjSpinnerComponent,
    KjTabComponent,
    KjTabListComponent,
    KjTabsComponent,
  ],
  template: `
    <!-- agent tool -->
    <kj-field class="spawn-field">
      <kj-field-label>Agent</kj-field-label>
      <!-- the shared .tool-tiles / .tool-tile recipe, same control as
           Settings → Default agent; only the selected tile's accent tint
           is per-instance, since it is the TOOL's colour, not the app's -->
      <div class="tool-tiles spawn-tools">
        @for (tl of tools(); track tl.id) {
          @let on = toolId() === tl.id;
          @let det = runtime.detection(tl.id);
          @let checking = runtime.detectionPending(tl.id);
          <kj-button
            kjVariant="ghost"
            class="tool-tile"
            (click)="pickTool(tl.id)"
            [style.--tile-border]="on ? mix(tl.accent, 45) : null"
            [style.--tile-bg]="on ? mix(tl.accent, 88) : null"
            [style.--tile-fg]="on ? 'var(--ink)' : null"
          >
            <app-tool-badge [tool]="tl.id" [size]="20" />
            <span class="tn">{{ tl.name }}</span>
            <!-- Order matters: the pending probe wins. A tool whose probe is
                 still out has NO verdict, and stamping it "not found"
                 before the answer lands is a guess the user can't tell
                 from a fact. "can’t run" carries the backend's reason in
                 the tooltip — Settings is where you act on it. -->
            @if (checking) {
              <span class="ts tchk"><kj-spinner kjSize="xs" [kjAriaLabel]="'Checking whether ' + tl.name + ' is installed'" />checking…</span>
            } @else if (det?.status === 'error') {
              <span class="ts tnum" style="color:var(--sem-attn)" [title]="det?.reason ?? ''">can’t run</span>
            } @else if (!runtime.toolAvailable(tl.id)) {
              <span class="ts tnum" style="color:var(--st-blocked)">not found</span>
            }
          </kj-button>
        }
      </div>
    </kj-field>

    <!-- model + effort -->
    <div class="spawn-split">
      <kj-field class="spawn-field">
        <kj-field-label>Model</kj-field-label>
        @if (currentTool().dynamicModels) {
          <!-- A tool that enumerates its own models (pi --list-models):
               the options ARE the probe result, and the same kouji
               combobox Settings uses stays free-text so a BYOK
               provider/model id can always be typed. -->
          <kj-combobox [freeText]="true" placeholder="provider/model…"
            [value]="modelId()" (valueChange)="onComboModel($event)">
            @for (m of modelChoices(); track m.id) {
              <kj-combobox-option [value]="m.id">{{ m.label }}</kj-combobox-option>
            }
            <kj-combobox-empty>{{ discoveryHint() }}</kj-combobox-empty>
          </kj-combobox>
        } @else {
          <app-select [value]="modelId()" [options]="modelOptions()" (valueChange)="pickModel($event)" />
        }
      </kj-field>
      @if (effortLevels(); as levels) {
        <kj-field class="spawn-field spawn-effort">
          <kj-field-label>Reasoning effort</kj-field-label>
          <!-- pills tabs, the same segmented control Settings uses for
               this very choice. As four outline kj-buttons the selection
               never showed: kouji declares --kj-button-bg / -fg /
               -border-color ON the inner .kj-button for [data-variant],
               and an element-level declaration beats the inherited value
               a host [style.--kj-button-*] sets — so every state binding
               was silently dropped. Tabs carry aria-selected and paint
               the chip themselves. -->
          <kj-tabs
            class="spawn-seg"
            variant="pills"
            [value]="effortLevel() ?? ''"
            (valueChange)="effortChange.emit($any($event))"
          >
            <kj-tab-list aria-label="Reasoning effort">
              @for (ef of levels; track ef) {
                <kj-tab [value]="ef">{{ ef }}</kj-tab>
              }
            </kj-tab-list>
          </kj-tabs>
        </kj-field>
      }
    </div>
  `,
  styles: [
    `
      :host {
        display: contents;
      }
      /* kj-field label/help mapped onto the app's micro-label vocabulary.
         Restated here rather than shared from the dialog's sheet: emulated
         encapsulation stamps a per-component attribute onto every selector, so
         a rule written in the parent's sheet cannot reach these nodes. */
      .spawn-field { --kj-field-gap: var(--sp-3); }
      .spawn-field ::ng-deep .kj-field-label {
        font: var(--fw-normal) var(--fs-badge) / 1.4 var(--font-ui);
        color: var(--ink-3);
        text-transform: uppercase;
        letter-spacing: 0.12em;
      }
      /* EVERY agent stays on ONE row here — the dialog is a fixed width and the
         picker reads as a single choice set, so it must not reflow into a grid
         at a larger --fs-scale. Column auto-flow instead of a repeat(N) track
         list: N was pinned at 4 and adding pi silently wrapped the 5th tile onto
         a second row. This cannot go stale.

         The tracks are floored, NOT minmax(0, 1fr): dividing a fixed width by an
         ever-growing N shaved every label a little further with each new adapter
         — at five the names already ellipsized, and the tiles would have gone on
         shrinking past legibility. The floor is the shared --tile-floor (in ch,
         so it grows with the type), 1fr only spends the slack while the row
         still fits; past that the overflow goes sideways under the app's own
         scrollbar. Deliberately NOT .scroll-hide — the bar is the only cue that
         there are more agents off-screen.

         padding-block: overflow-x makes overflow-y a scrollport too, so a
         focused tile's ring would be clipped (and would bounce the row
         vertically) without a gutter to draw into. scroll-padding stops the
         browser from parking a Tab-focused tile flush against the clipped edge. */
      .spawn-tools {
        --tile-cols: none;
        grid-auto-flow: column;
        grid-auto-columns: minmax(var(--tile-floor, 14ch), 1fr);
        overflow-x: auto;
        padding-block: var(--sp-2);
        scroll-padding-inline: var(--sp-5);
      }
      /* the in-progress probe line: the spinner sits inline with its label, so
         the tile keeps the same single-row status slot the verdicts use and
         nothing reflows when the answer lands */
      .tool-tile .ts.tchk {
        display: inline-flex;
        align-items: center;
        gap: var(--sp-2);
        color: var(--ink-4);
      }
      /* The dialog's two-up row: ONE line while both halves fit, each dropping
         to a full-width line of its own when they don't.

         min-width:0 is the load-bearing part. A flex item's min-width defaults
         to auto = its min-content width, and the kj pill tray is a nowrap flex
         row, so its min-content is the SUM of every pill. pi declares seven
         levels against claude's five and codex's four, so its tray measured
         wider than half the dialog and took the difference out of the Model
         field — the worst possible pairing, since pi is also the tool whose
         model ids are longest (a full provider/model slug). Without the floor
         the effort tray simply wins that negotiation.

         The two flex-bases are what decide where the row breaks: the Model
         column asks for the share it gets on claude/codex (in ch, so it tracks
         the type ramp like --tile-floor does), the tray asks for its one-line
         width, and the line wraps the moment the pair exceeds the dialog.
         Wrapped, each is alone on its line and grows to the full width. */
      .spawn-split { display: flex; flex-wrap: wrap; gap: var(--sp-6); }
      .spawn-split > * { flex: 1 1 22ch; min-width: 0; }
      .spawn-split > .spawn-effort { flex-basis: auto; }
      /* the effort tray fills its column, same size step as Settings' .set-seg.
         display:block: kouji makes the pills variant inline-block, i.e.
         shrink-to-fit, so the tray sized itself off its content and the
         width:100% below resolved against that same content width — the tray
         never actually measured the column it is supposed to wrap inside. */
      .spawn-seg { --kj-tab-padding-x: var(--sp-6); --kj-tab-font-size: var(--fs-meta); display: block; }
      /* WRAP is the overflow strategy at both levels: the field wraps out of
         the Model row above, and here the pills themselves wrap as the last
         resort (a full row still too narrow — a large --fs-scale, a future
         tool with more levels than pi). Not shrink: past ~35px "minimal" and
         "medium" clip to stubs that can't be told apart, and the levels ARE
         the control. Not scroll: a segmented control is read as a set, and a
         scroller hides levels behind a gesture the tray gives no hint of and
         can park the SELECTED pill off-screen, leaving the field showing no
         answer at all. A second line only costs height, which a dialog's
         scroll column already has.

         flex:1 1 auto, not flex:1 — a 0 basis never overflows, so the line
         would never break and the pills would go back to squeezing. auto
         keeps each pill at its label's width, grow spends the last line's
         slack, and the flex default min-width:auto is the floor that stops a
         label clipping. */
      .spawn-seg ::ng-deep .kj-tab-list { width: 100%; flex-wrap: wrap; }
      .spawn-seg ::ng-deep .kj-tab { flex: 1 1 auto; justify-content: center; text-transform: capitalize; }
    `,
  ],
})
export class AgentToolControlsComponent {
  readonly runtime = inject(AgentRuntimeService);
  private readonly catalog = inject(ModelCatalogService);
  readonly mix = mix;

  readonly toolId = signal<AgentTool["id"]>(AGENT_TOOLS[0].id);
  readonly modelId = signal<string>("");
  readonly effortLevel = signal<string | null>(null);

  @Input({ required: true }) set tool(v: AgentTool["id"]) {
    this.toolId.set(v);
  }
  @Input({ required: true }) set model(v: string) {
    this.modelId.set(v);
  }
  @Input() set effort(v: string | null) {
    this.effortLevel.set(v);
  }

  @Output() readonly toolChange = new EventEmitter<AgentTool["id"]>();
  @Output() readonly modelChange = new EventEmitter<string>();
  @Output() readonly effortChange = new EventEmitter<string | null>();

  readonly tools = computed(() => orderedTools(this.runtime));
  readonly currentTool = computed(
    () => AGENT_TOOLS.find((t) => t.id === this.toolId()) ?? AGENT_TOOLS[0],
  );
  readonly modelOptions = computed(() => modelOptionsFor(this.currentTool()));
  readonly modelChoices = computed(() => modelChoicesFor(this.currentTool(), this.catalog));
  readonly discoveryHint = computed(() =>
    discoveryHintFor(this.currentTool(), this.catalog, this.runtime),
  );
  readonly effortLevels = computed(() => effortLevelsFor(this.currentTool(), this.modelId()));

  constructor() {
    // The tiles are the first screen in the app that needs a detection verdict,
    // and the sweep no longer runs at boot — demand it here, not from tools(),
    // which change detection re-reads on every pass.
    this.runtime.ensureDetections();
    // …and RE-ask every model-enumerating CLI (pi, cursor-agent) for its models
    // on every open: an account pool or an API key can change under us, so a
    // per-session cache would go stale. Stale-while-revalidate — the last known
    // list keeps rendering until the fresh one lands.
    for (const t of AGENT_TOOLS) if (t.dynamicModels) this.catalog.refresh(t.id);
  }

  pickTool(id: AgentTool["id"]): void {
    // a tool that owns its model list: ask its CLI (cached per session)
    const tool = AGENT_TOOLS.find((t) => t.id === id);
    if (tool?.dynamicModels) this.catalog.load(id);
    this.toolChange.emit(id);
  }

  pickModel(id: string): void {
    this.modelChange.emit(id);
  }

  /** Combobox commit (probed pick or free-text Enter). kouji's combobox emits
   *  `unknown` — it carries whatever an option's [value] held — so the id is
   *  narrowed here rather than with `$any` in the template, which would switch
   *  template checking off for the whole binding. */
  onComboModel(v: unknown): void {
    const id = String(v ?? "").trim();
    if (id) this.pickModel(id);
  }
}
