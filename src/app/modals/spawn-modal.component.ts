import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  ElementRef,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { AGENT_TOOLS, defaultEffortFor, effortLevelsFor, modelOption } from "../data";
import { Agent, AgentTool, Project, Ticket } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { modelDiscoveryHint, ModelCatalogService } from "../agents/model-catalog.service";
import { ProjectActionsService } from "../projects/project-actions.service";
import { effectiveModel, SettingsStore, worktreeRootLabel } from "../settings/settings.store";
import { TicketsStore } from "../stores/tickets.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { SelectComponent, SelectGroup, SelectOption } from "../shared/select.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { mix } from "../utils";
import {
  KjButtonComponent,
  KjComboboxComponent,
  KjComboboxEmptyComponent,
  KjComboboxOptionComponent,
  KjDialogComponent,
  KjFieldComponent,
  KjFieldHelpComponent,
  KjFieldLabelComponent,
  KjInputComponent,
  KjInputGroupAddonComponent,
  KjInputGroupComponent,
  KjSpinnerComponent,
  KjTabComponent,
  KjTabListComponent,
  KjTabsComponent,
  KjTextareaComponent,
} from "@kouji-ui/components";
import { KjDialog } from "@kouji-ui/core";

/** Strip HTML tags to plain text (no DOM dependency — regex-based). */
function stripHtml(html: string): string {
  return html
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<\/p>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/&nbsp;/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/**
 * The value carried by the Ticket picker's "None" row.
 *
 * `""` cannot be it: kouji reads an empty value as "nothing selected" and
 * paints the placeholder over the row's own label, so the option would read
 * "Select…" instead of "None — start from scratch". The component keeps using
 * `""` internally for "no ticket"; this sentinel only ever exists between
 * `ticketSelection()` and `selectTicket()`.
 */
const NO_TICKET = "__none__";

/** Convert a title to a slug (mirrors the backend worktree slug logic). */
function slugName(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 50);
}

/**
 * Opened through `KjDialog` by the shell, so this component IS the overlay
 * panel: the backdrop, focus trap, scroll lock, Esc and outside-click all come
 * from the kj overlay and the markup below is only the panel body.
 */
@Component({
  selector: "app-spawn-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    SelectComponent,
    KjComboboxComponent,
    KjComboboxEmptyComponent,
    KjComboboxOptionComponent,
    ToolBadgeComponent,
    KjButtonComponent,
    KjDialogComponent,
    KjFieldComponent,
    KjFieldHelpComponent,
    KjFieldLabelComponent,
    KjInputComponent,
    KjInputGroupAddonComponent,
    KjInputGroupComponent,
    KjSpinnerComponent,
    KjTabComponent,
    KjTabListComponent,
    KjTabsComponent,
    KjTextareaComponent,
  ],
  host: { role: "dialog", "aria-modal": "true", "aria-label": "Spawn agent" },
  template: `
    @let proj = project();
    @let linked = !!ticketId();
    <kj-dialog-shell>
      <div class="kj-dialog rise">
        <div class="pane-head" style="padding:var(--sp-6) var(--sp-7)">
          <!-- the shared .head-icon square, same as Add project's -->
          <span class="head-icon"><app-icon name="agent" color="var(--ui-ink)" /></span>
          <h1 style="white-space:nowrap">Spawn agent</h1>
        </div>

        <div class="scroll-y" style="padding:var(--sp-7);display:flex;flex-direction:column;gap:var(--sp-7);flex:1">
          <!-- project + branch -->
          <div style="display:flex;gap:var(--sp-6)">
            <!-- min-width:0 on both columns: a flex item defaults to
                 min-width:auto, so a long branch name inside the select would
                 widen the column past the card instead of being clipped -->
            <div style="flex:1;min-width:0">
              <label class="field-label">Project</label>
              <app-select [value]="projectId()" [options]="projectOptions()" (valueChange)="setProject($event)" />
              <div class="trunc" style="font-size:var(--fs-meta);color:var(--ink-4);margin-top:var(--sp-3)">{{ proj.path }}</div>
            </div>
            <div style="flex:1;min-width:0">
              <div style="display:flex;align-items:center;gap:var(--sp-3);margin-bottom:var(--sp-3)">
                <label class="field-label" style="margin-bottom:0">Source branch</label>
                <button
                  class="pane-btn"
                  title="Refresh branches from disk"
                  [disabled]="branchesBusy()"
                  (click)="refreshBranches()"
                  style="margin-left:auto"
                >
                  <app-icon name="refresh" size="sm" />
                </button>
              </div>
              <app-select [value]="branch()" [options]="proj.branches ?? []" (valueChange)="branch.set($event)" />
              @if (!proj.branches?.length) {
                <div style="font-size:var(--fs-meta);color:var(--st-blocked);margin-top:var(--sp-3)">no branch found — project git is not initialized</div>
              } @else {
                <div style="font-size:var(--fs-meta);color:var(--ink-4);margin-top:var(--sp-3)">base · {{ proj.head }}</div>
              }
            </div>
          </div>

          <!-- ticket + name — paired because picking the ticket IS what fills
               the name: the prefill, the "Name linked" line and the focus-ring
               tint all cross between these two, and stacked they sat far enough
               apart that the link read as a coincidence. Both columns are the
               same kj-field wrapper (the Ticket half used to be a bare div +
               .field-label): one wrapper means one label type step, one
               label→control gap, and therefore one baseline per row, which is
               what the old pairing could not give. -->
          <div class="spawn-split">
            <!-- Same <app-select> as Project / Source branch / Model: the To do /
                 In progress split rides in as option GROUPS, which is what the
                 native <optgroup> was here for. -->
            <kj-field class="spawn-field">
              <kj-field-label>Ticket</kj-field-label>
              <app-select
                [value]="ticketSelection()"
                [options]="ticketOptions()"
                (valueChange)="selectTicket($event)"
                [style.--kj-border-default]="linked ? 'var(--ui-line)' : null"
              />
              <!-- only the LINKED state gets a line. The idle copy said the
                   field was optional, which the None row already says from
                   inside the picker, and it held a line of height under every
                   dialog open to do it. -->
              @if (linked) {
                <kj-field-help class="spawn-linked"><app-icon name="link" size="sm" />Name linked · the ticket is prepended to the prompt on spawn</kj-field-help>
              }
            </kj-field>

            <!-- name (drives the worktree, unique per project) -->
            <kj-field class="spawn-field">
              <kj-field-label>Name</kj-field-label>
              <kj-input-group class="spawn-name" [style.--kj-input-group-border-color]="linked ? 'var(--ui-focus)' : null">
                <kj-input-group-addon><app-icon name="agent" size="sm" color="var(--ink-4)" /></kj-input-group-addon>
                <kj-input
                  [value]="name()"
                  (input)="onNameInput($any($event.target).value)"
                  placeholder="e.g. fix-login-bug"
                />
              </kj-input-group>
              <!-- no help line: the footer already prints the full destination
                   (worktreeDest), so the preview here only restated its tail -->
            </kj-field>
          </div>

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
                  (click)="setTool(tl.id)"
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
                  [value]="model()" (valueChange)="onComboModel($event)">
                  @for (m of modelChoices(); track m.id) {
                    <kj-combobox-option [value]="m.id">{{ m.label }}</kj-combobox-option>
                  }
                  <kj-combobox-empty>{{ discoveryHint() }}</kj-combobox-empty>
                </kj-combobox>
              } @else {
                <app-select [value]="model()" [options]="modelOptions()" (valueChange)="setModel($event)" />
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
                  [value]="effort() ?? ''"
                  (valueChange)="effort.set($any($event))"
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

          <!-- initial prompt — the agent's own instructions. NOT prefilled from
               the ticket; when a ticket is linked its content is prepended
               ("Implement …") at spawn time (see composePrompt). -->
          <kj-field class="spawn-field">
            <kj-field-label>Initial prompt</kj-field-label>
            <kj-textarea
              #promptEl
              class="spawn-textarea"
              [kjValue]="prompt()"
              (input)="onPromptInput($any($event.target).value)"
              kjRows="3"
              kjResize="none"
              [kjPlaceholder]="linked ? 'Add extra instructions — the ticket is included automatically…' : 'Describe what this agent should do…'"
            />
          </kj-field>
        </div>

        <div style="padding:var(--sp-6) var(--sp-7);border-top:1px solid var(--hair);display:flex;align-items:center;gap:var(--sp-4);flex:none">
          <span class="trunc" style="color:var(--ink-4)">→ {{ worktreeDest() }}</span>
          <kj-button class="spawn-cancel" kjVariant="outline" (click)="ui.closeSpawn()">Cancel</kj-button>
          <kj-button kjVariant="outline" [kjDisabled]="!name().trim() || !branch()" (click)="submit(false)"><app-icon name="plus" size="sm" />Create</kj-button>
          <kj-button kjVariant="default" [kjDisabled]="!name().trim() || !branch()" (click)="submit(true)"><app-icon name="bolt" size="sm" />Spawn</kj-button>
        </div>
      </div>
    </kj-dialog-shell>
  `,
  styles: [
    `
      /* The panel box is the shared .kj-overlay-wrapper .kj-dialog recipe in
         styles.css; only this modal's width and height cap are per-instance —
         kouji exposes no dialog-width knob, the class IS the surface, and the
         density-scaled round(calc(Npx * --density)) is the app's own width
         convention (Add project 480, Delete worktree 440).

         600, up from 540: this dialog carries three side-by-side pairs (project
         / branch, ticket / name, model / effort) where the others carry stacked
         fields, and
         at 540 the model+effort pair broke onto two rows for every tool with
         more than five effort levels. The extra 60 buys claude's and codex's
         trays their row back; pi's seven still wrap, by design. */
      .kj-dialog {
        width: round(calc(600px * var(--density)), 1px);
        max-height: 90vh;
      }
      /* kj-field label/help mapped onto the app's micro-label vocabulary */
      .spawn-field { --kj-field-gap: var(--sp-3); }
      /* the field label is the same micro-label role as .up; kouji owns the
         element so the recipe is restated here rather than classed */
      .spawn-field ::ng-deep .kj-field-label {
        font: var(--fw-normal) var(--fs-badge) / 1.4 var(--font-ui);
        color: var(--ink-3);
        text-transform: uppercase;
        letter-spacing: 0.12em;
      }
      .spawn-field ::ng-deep .kj-field-help { font-size: var(--fs-meta); color: var(--ink-4); }
      /* The linked confirmation is the row's only helper line, and the one that
         is NOT muted meta — it reports a state the user just caused, so it
         keeps --ink-2. It has to be painted on the SPAN: kj-field-help's host
         is display:contents, so a display or box rule set on the host lands on
         nothing. flex-start + flex:none: at half width the sentence wraps, and
         centring would float the glyph against the middle of a two-line block
         while letting it squeeze. */
      .spawn-linked ::ng-deep .kj-field-help {
        display: flex;
        align-items: flex-start;
        gap: var(--sp-2);
        color: var(--ink-2);
      }
      .spawn-linked ::ng-deep app-icon { flex: none; }
      /* name input group: one panel-2 box, addon + input share the (linkable) border */
      .spawn-name { width: 100%; border-radius: var(--r-md); }
      .spawn-name ::ng-deep .kj-input-group__addon {
        background: var(--panel-2);
        border-color: var(--kj-input-group-border-color, var(--hair));
        border-right: none;
        color: var(--ink-4);
      }
      .spawn-name ::ng-deep .kj-input {
        /* kouji defaults a group to data-size="md", and the app-wide
           kj-input-group cascade types an md group's field at --fs-badge — a
           rule written for the compact search box. This is a full-width form
           field, so it takes the same baseline as the Initial prompt below it;
           without this the dialog's two text inputs disagree by two steps. */
        --kj-input-font-size: var(--fs-body);
        background: var(--panel-2);
        border-color: var(--kj-input-group-border-color, var(--hair));
        border-left: none;
        box-shadow: none;
        color: var(--ink);
        font-family: var(--font-mono);
        padding: var(--sp-5) var(--sp-5) var(--sp-5) 0;
      }
      .spawn-name ::ng-deep .kj-input:focus-visible { outline: none; }
      /* Only the DELTAS from the app-wide kj-textarea defaults in styles.css.
         This used to restate the whole box on the raw <textarea>, which set
         every property EXCEPT font-size — so the knob pack's size was the one
         thing still in force and the prompt typed a step larger than the Name
         field above it. Going through the knobs keeps the two on one ramp, and
         ::placeholder rides along (kouji only recolours it). */
      .spawn-textarea ::ng-deep .kj-textarea {
        --kj-textarea-radius: var(--r-md);
        --kj-textarea-padding-x: var(--sp-6);
        --kj-textarea-padding-y: var(--sp-5);
      }
      .spawn-textarea ::ng-deep .kj-textarea:focus-visible {
        outline: none;
        --kj-textarea-border-color: var(--ui-focus);
      }
      /* EVERY agent stays on ONE row here — the dialog is a fixed 540px and the
         picker reads as a single choice set, so it must not reflow into a grid
         at a larger --fs-scale. Column auto-flow instead of a repeat(N) track
         list: N was pinned at 4 and adding pi silently wrapped the 5th tile onto
         a second row. This cannot go stale.

         The tracks are floored, NOT minmax(0, 1fr): dividing a fixed 540px by an
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
         to a full-width line of its own when they don't. Shared by Ticket |
         Name and Model | Reasoning effort — a second, non-wrapping convention
         for the other pair would break at a large --fs-scale exactly where this
         one was written not to. Pure CSS, so it holds for any tool declaring
         any number of levels rather than for the five ids we happen to ship
         today.

         min-width:0 is the load-bearing part. A flex item's min-width defaults
         to auto = its min-content width, and the kj pill tray is a nowrap flex
         row, so its min-content is the SUM of every pill. pi declares seven
         levels against claude's five and codex's four, so its tray measured
         wider than half the dialog and took the difference out of the Model
         field — the worst possible pairing, since pi is also the tool whose
         model ids are longest (a full provider/model slug). Without the floor
         the effort tray simply wins that negotiation. On Ticket | Name it is a
         ticket title or a worktree preview that measures wider than the column
         and would push the pair past the card.

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
         answer at all. A second line only costs height, which this dialog's
         scroll column already has.

         flex:1 1 auto, not flex:1 — a 0 basis never overflows, so the line
         would never break and the pills would go back to squeezing. auto
         keeps each pill at its label's width, grow spends the last line's
         slack, and the flex default min-width:auto is the floor that stops a
         label clipping. */
      .spawn-seg ::ng-deep .kj-tab-list { width: 100%; flex-wrap: wrap; }
      .spawn-seg ::ng-deep .kj-tab { flex: 1 1 auto; justify-content: center; text-transform: capitalize; }
      /* footer: the path label ellipsizes, the first button pushes the trio right */
      .spawn-cancel ::ng-deep .kj-button { margin-left: auto; }
    `,
  ],
})
export class SpawnModalComponent {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
  readonly runtime = inject(AgentRuntimeService);
  readonly agentActions = inject(AgentActionsService);
  private readonly settingsStore = inject(SettingsStore);
  private readonly catalog = inject(ModelCatalogService);
  private readonly ticketsStore = inject(TicketsStore);
  readonly mix = mix;

  private defaultProject = this.ui.spawning()?.project ?? null;

  readonly projectId = signal<string>(this.defaultProject || this.projects.all()[0].id);
  readonly toolId = signal<Agent["tool"]>(this.initialTool());
  readonly name = signal("");
  readonly prompt = signal("");

  /** "" = None (no ticket linked) */
  readonly ticketId = signal<string>("");

  /** Track whether the user has manually typed in the Name field, so a ticket
   *  selection won't overwrite a name they've already customized. */
  private nameUserEdited = false;

  /** Runnable as the TILE reports it: detected AND without an `error` verdict.
   *  An errored tool still answers `toolAvailable` optimistically in some
   *  paths, but it renders "can’t run" — offering it up front would promote the
   *  one agent the user cannot actually spawn. */
  private runnable(id: string): boolean {
    return this.runtime.toolAvailable(id) && this.runtime.detection(id)?.status !== "error";
  }

  /**
   * Tile order: runnable agents first, the rest after. Two filters rather than
   * a sort — a partition is stable BY CONSTRUCTION, so each group keeps its
   * AGENT_TOOLS declaration order and the row stays predictable instead of
   * riding on a comparator's tie-breaking (V8's sort is stable, but nothing in
   * a `a-b` comparator says so at the call site).
   *
   * While ANY probe is still out the declaration order stands unchanged. The
   * backend sweep answers tool by tool, so ordering during that window would
   * walk tiles sideways one probe at a time — under a cursor already hovering
   * a tile, the click would land on a different agent than the one aimed at.
   * Holding until the sweep settles trades that shuffle for a single, visible
   * move. Selection is held by id (`toolId`), never by position, so neither
   * this nor `initialTool()` / `setTool()` can be disturbed by the reorder.
   */
  readonly tools = computed<AgentTool[]>(() => {
    if (AGENT_TOOLS.some((t) => this.runtime.detectionPending(t.id))) return AGENT_TOOLS;
    return [
      ...AGENT_TOOLS.filter((t) => this.runnable(t.id)),
      ...AGENT_TOOLS.filter((t) => !this.runnable(t.id)),
    ];
  });

  readonly currentTool = computed(() => AGENT_TOOLS.find((t) => t.id === this.toolId())!);
  readonly project = computed(
    () => this.projects.all().find((p) => p.id === this.projectId()) || this.projects.all()[0],
  );
  /** Projects as app-select options (id → display name). */
  readonly projectOptions = computed(() => this.projects.all().map((p) => ({ value: p.id, label: p.name })));
  // mirror the backend slug so the user sees the worktree name they'll get
  readonly worktreePreview = computed(
    () => this.name().toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/^_+|_+$/g, "") || "—",
  );
  /** Full destination path preview: effective root (settings) + name slug —
   *  mirrors the backend's flat `<root>/<slug>` layout. */
  readonly worktreeDest = computed(
    () => `${worktreeRootLabel(this.settingsStore.settings())}/${this.worktreePreview()}`,
  );

  readonly model = signal<string>(this.prefillModel(this.currentTool()));
  readonly effort = signal<string | null>(this.prefillEffort(this.currentTool()));
  /** The tool's curated models as grouped picker options ("Latest" aliases,
   *  "Pinned versions"…), labels human, values the exact `--model` id. */
  readonly modelOptions = computed<(SelectOption | SelectGroup)[]>(() => {
    const groups: { label: string; options: SelectOption[] }[] = [];
    for (const m of this.currentTool().models) {
      const label = m.group ?? "";
      let g = groups.find((x) => x.label === label);
      if (!g) groups.push((g = { label, options: [] }));
      g.options.push({ value: m.id, label: m.label });
    }
    // a single unnamed group is just a flat list
    return groups.length === 1 && !groups[0].label ? groups[0].options : groups;
  });
  /** Raw probe output for the current tool (pi `--list-models`, `cursor-agent
   *  models`) — the ids exactly as its `--model` flag takes them. Empty until
   *  the probe lands, and legitimately empty when the CLI is absent, signed out
   *  or has no API keys. */
  readonly discoveredModels = computed(() =>
    this.currentTool().dynamicModels ? this.catalog.models(this.toolId()) : [],
  );
  /** What the picker actually offers: the PROBE when it reported anything, else
   *  the tool's curated catalog. cursor keeps a curated list for exactly this
   *  fallback; pi's is empty by design, so its picker is free-text only. */
  readonly modelChoices = computed(() => this.modelChoicesFor(this.currentTool()));
  private modelChoicesFor(tool: AgentTool): { id: string; label: string }[] {
    if (tool.dynamicModels) {
      const probed = this.catalog.models(tool.id);
      if (probed.length) return probed.map((id) => ({ id, label: id }));
    }
    return tool.models;
  }
  /** Empty-state copy for the dynamic picker. One empty combobox used to stand
   *  for four unrelated situations — still probing, CLI not installed, probe
   *  failed, CLI fine but signed out — so it said nothing useful about any of
   *  them. `modelDiscoveryHint` names which one it is (shared with Settings). */
  readonly discoveryHint = computed(() => {
    const tool = this.currentTool();
    return modelDiscoveryHint(tool, {
      probed: this.catalog.isProbed(tool.id),
      error: this.catalog.error(tool.id),
      empty: !this.catalog.models(tool.id).length,
      detection: this.runtime.detection(tool.id),
    });
  });
  /** Effort levels the PICKED model accepts (per model: Haiku none, Opus 4.6
   *  no `xhigh`); false hides the tray. */
  readonly effortLevels = computed(() => effortLevelsFor(this.currentTool(), this.model()));
  // Pre-select the repo's default branch (origin/HEAD → main/master → HEAD);
  // fall back to the first branch in the list when no default resolves.
  readonly branch = signal<string>(this.defaultBranchFor(this.project()));

  /** A branch refresh is in flight (opening the dialog runs one too). */
  readonly branchesBusy = signal(false);

  /** Re-read the projects from disk so the branch list reflects branches
   *  created since the last load; keeps the selection when it still exists. */
  async refreshBranches(): Promise<void> {
    if (this.branchesBusy()) return;
    this.branchesBusy.set(true);
    try {
      await this.projects.refresh();
      const proj = this.project();
      if (!proj.branches?.includes(this.branch())) {
        this.branch.set(this.defaultBranchFor(proj));
      }
    } catch {
      /* backend unavailable — keep the stale list */
    } finally {
      this.branchesBusy.set(false);
    }
  }

  /** Open tickets (todo + inprogress), same-project first, then rest. */
  private readonly openTickets = computed<Ticket[]>(() => {
    const all = this.ticketsStore.all();
    const open = all.filter((t) => t.status === "todo" || t.status === "inprogress");
    const pid = this.projectId();
    const sameProject = open.filter((t) => t.projectId === pid);
    const others = open.filter((t) => t.projectId !== pid);
    return [...sameProject, ...others];
  });

  readonly openTicketsTodo = computed<Ticket[]>(() =>
    this.openTickets().filter((t) => t.status === "todo"),
  );
  readonly openTicketsInProgress = computed<Ticket[]>(() =>
    this.openTickets().filter((t) => t.status === "inprogress"),
  );

  /** Tickets as app-select options: the "None" escape hatch loose at the top,
   *  then one group per open status. An empty status contributes no group, so
   *  the listbox never shows a heading with nothing under it. */
  readonly ticketOptions = computed<(SelectOption | SelectGroup)[]>(() => {
    const opts: (SelectOption | SelectGroup)[] = [
      { value: NO_TICKET, label: "None — start from scratch" },
    ];
    const todo = this.openTicketsTodo();
    if (todo.length) {
      opts.push({ label: "To do", options: todo.map((t) => ({ value: t.id, label: t.title })) });
    }
    const inProgress = this.openTicketsInProgress();
    if (inProgress.length) {
      opts.push({
        label: "In progress",
        options: inProgress.map((t) => ({ value: t.id, label: t.title })),
      });
    }
    return opts;
  });

  // ---- settings prefill (defaultTool / toolModel / toolEffort) ----
  /** The settings defaultTool when it names a DETECTED tool; the hardcoded
   *  default ("claude") otherwise — "" means nothing was ever saved. */
  private initialTool(): Agent["tool"] {
    const id = this.settingsStore.settings().defaultTool;
    const known = AGENT_TOOLS.some((t) => t.id === id);
    return known && this.runtime.toolAvailable(id) ? (id as Agent["tool"]) : "claude";
  }
  /** Per-tool settings model override while it's still in the curated list
   *  (a stale persisted id must not produce an unselectable <option>);
   *  otherwise the tool's first curated model — the old hardcoded default. */
  private prefillModel(tool: AgentTool): string {
    const s = this.settingsStore.settings();
    const choices = this.modelChoicesFor(tool);
    // An EXPLICIT per-tool override (not `effectiveModel`, which already folds
    // in the curated default) survives even when it is in neither the probe nor
    // the curated list — a probe-backed picker is free-text, so a BYOK id the
    // user typed must not be silently rewritten.
    const stored = s.toolModel[tool.id];
    if (stored && (tool.dynamicModels || choices.some((c) => c.id === stored))) return stored;
    const eff = effectiveModel(s, tool.id);
    if (choices.some((c) => c.id === eff)) return eff;
    // else the first id actually on offer (the probe's, or the curated list's);
    // a probe-backed tool with nothing at all launches on the CLI's own default.
    return choices[0]?.id ?? (tool.dynamicModels ? "" : tool.models[0].id);
  }
  /** The user's explicit settings effort when the prefilled MODEL accepts it,
   *  else that model's own default; null when it takes no effort (Haiku). The
   *  raw override, not `effectiveEffort`: that resolves against the SETTINGS
   *  model, which may be a custom id the dialog just fell back from. */
  private prefillEffort(tool: AgentTool): string | null {
    const model = this.prefillModel(tool);
    const levels = effortLevelsFor(tool, model);
    if (!levels) return null;
    const override = this.settingsStore.settings().toolEffort[tool.id];
    return override && levels.includes(override) ? override : defaultEffortFor(tool, model);
  }
  /** Combobox commit (probed pick or free-text Enter). kouji's combobox emits
   *  `unknown` — it carries whatever an option's [value] held — so the id is
   *  narrowed here rather than with `$any` in the template, which would switch
   *  template checking off for the whole binding. Mirrors the settings modal. */
  onComboModel(v: unknown): void {
    const id = String(v ?? "").trim();
    if (id) this.setModel(id);
  }

  /** Picking a model re-validates the effort against what IT accepts: keep
   *  the level when offered, else fall to the model's default (or none). */
  setModel(id: string) {
    this.model.set(id);
    const tool = this.currentTool();
    const levels = effortLevelsFor(tool, id);
    const cur = this.effort();
    this.effort.set(!levels ? null : cur && levels.includes(cur) ? cur : defaultEffortFor(tool, id));
  }

  private promptEl = viewChild("promptEl", { read: ElementRef });

  constructor() {
    // Esc / outside-click close the overlay, not the store — clear the flag on
    // teardown so the two can never drift.
    inject(DestroyRef).onDestroy(() => this.ui.closeSpawn());

    // The tiles are the first screen in the app that needs a detection verdict,
    // and the sweep no longer runs at boot — demand it here, not from tools()/
    // runnable(), which change detection re-reads on every pass.
    this.runtime.ensureDetections();

    // Opening the dialog refreshes the branch lists — branches created since
    // the last project load (by agents, or outside the app) must be offerable.
    void this.refreshBranches();

    // …and RE-asks every model-enumerating CLI (pi, cursor-agent) for its
    // models on every open: an account pool or an API key can change under us,
    // so a per-session cache would go stale. Stale-while-revalidate — the last
    // known list keeps rendering until the fresh one lands.
    for (const t of AGENT_TOOLS) if (t.dynamicModels) this.catalog.refresh(t.id);

    // Consume the dispatch ticket id (set by ui.dispatchTicket) and clear it
    // so it doesn't leak into subsequent manual spawns.
    const dispatched = this.ui.spawnTicketId();
    if (dispatched) {
      this.ui.clearSpawnTicket();
      this.applyTicket(dispatched);
    }

    // The Initial prompt starts empty (even when a ticket is linked), so focus
    // it so the user can add instructions immediately (focus-only, no hook).
    afterNextRender(() => {
      (this.promptEl()?.nativeElement as HTMLElement | undefined)?.querySelector("textarea")?.focus();
    });
  }

  /** Called when the user manually edits the Name field. */
  onNameInput(value: string) {
    this.nameUserEdited = true;
    this.name.set(value);
  }

  /** Called when the user manually edits the Initial prompt field. */
  onPromptInput(value: string) {
    this.prompt.set(value);
  }

  /**
   * Apply a ticket selection: prefill the Name (unless the user already typed
   * one) and update the linked ticket id. The Initial prompt is intentionally
   * left untouched — the ticket's content is composed into the effective prompt
   * only at spawn time (see composePrompt). Passing "" clears the selection.
   */
  /** The Ticket picker's bound value: the linked id, or the None sentinel. */
  readonly ticketSelection = computed(() => this.ticketId() || NO_TICKET);

  /** Ticket picker -> component: unwrap the None sentinel back to "". */
  selectTicket(value: string) {
    this.applyTicket(value === NO_TICKET ? "" : value);
  }

  applyTicket(id: string) {
    this.ticketId.set(id);
    if (!id) return;

    const tk = this.ticketsStore.byId(id);
    if (!tk) return;

    // Switch to the ticket's project if it has one
    if (tk.projectId) {
      this.projectId.set(tk.projectId);
      this.branch.set(this.defaultBranchFor(this.project()));
    }

    // Prefill name only if the user hasn't manually typed anything
    if (!this.nameUserEdited || !this.name().trim()) {
      this.name.set(slugName(tk.title));
      this.nameUserEdited = false; // reset so next ticket selection can prefill again
    }
  }

  setProject(id: string) {
    this.projectId.set(id);
    this.branch.set(this.defaultBranchFor(this.project()));
  }

  /** The pre-selected source branch for a project: its resolved default branch,
   *  else the first branch in the list, else "" (non-git project). */
  private defaultBranchFor(project: Project): string {
    return project.defaultBranch ?? project.branches?.[0] ?? "";
  }
  setTool(id: Agent["tool"]) {
    this.toolId.set(id);
    const tool = this.currentTool();
    // a tool that owns its model list: ask its CLI (cached per session)
    if (tool.dynamicModels) this.catalog.load(tool.id);
    // switching tool applies THAT tool's settings defaults (or the curated ones)
    this.model.set(this.prefillModel(tool));
    this.effort.set(this.prefillEffort(tool));
  }
  submit(start: boolean) {
    if (!this.name().trim() || !this.branch()) return;
    this.agentActions.spawn({
      projectId: this.projectId(),
      branch: this.branch(),
      toolId: this.toolId(),
      model: this.model(),
      effort: this.effort(),
      name: this.name().trim(),
      prompt: this.composePrompt(),
      ticketId: this.ticketId() || undefined,
      start,
    });
  }

  /**
   * The agent's effective initial prompt. With a linked ticket the ticket's
   * content leads — `Implement <ticket title + notes>` — and the user's own
   * prompt (if any) is appended after it: `Implement <ticket>, <user prompt>`.
   * Without a ticket it's just the user's prompt.
   */
  private composePrompt(): string {
    const userPrompt = this.prompt().trim();
    const id = this.ticketId();
    const tk = id ? this.ticketsStore.byId(id) : undefined;
    if (!tk) return userPrompt;
    const plain = stripHtml(tk.notes ?? "");
    const ticketPrompt = tk.title + (plain ? "\n\n" + plain : "");
    return userPrompt ? `Implement ${ticketPrompt}, ${userPrompt}` : `Implement ${ticketPrompt}`;
  }
}
