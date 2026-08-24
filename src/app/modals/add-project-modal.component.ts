import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  effect,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { PROJECT_COLORS, PROJECT_ICONS } from "../data";
import { ProjectActionsService } from "../projects/project-actions.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { ProjectsStore } from "../stores/projects.store";
import { SettingsStore } from "../settings/settings.store";
import { mix } from "../utils";
import { KjButtonComponent, KjDialogComponent, KjInputComponent, KjRadioComponent, KjRadioGroupComponent, KjTabComponent, KjTabListComponent, KjTabsComponent } from "@kouji-ui/components";
import { KjDialog } from "@kouji-ui/core";

/**
 * Opened through `KjDialog` by the shell, so this component IS the overlay
 * panel: the backdrop, focus trap, scroll lock, Esc and outside-click all come
 * from the kj overlay and the markup below is only the panel body.
 */
@Component({
  selector: "app-add-project-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjButtonComponent, KjDialogComponent, KjInputComponent, KjTabsComponent, KjTabListComponent, KjTabComponent, KjRadioGroupComponent, KjRadioComponent],
  host: { role: "dialog", "aria-modal": "true", "aria-label": "Add project" },
  template: `
    <kj-dialog-shell>
      <div class="kj-dialog rise">
        <div class="pane-head" style="padding:var(--sp-6) var(--sp-7)">
          <span class="head-icon" [style.--head-accent]="color()">
            <app-icon [name]="icon()" [color]="color()" />
          </span>
          <h1 style="white-space:nowrap">Add project</h1>
        </div>

        <!-- the kj panel is height-capped, so the fields scroll between the
             pinned header and footer (the git source makes this form tall) -->
        <div class="scroll-y" style="padding:var(--sp-7);display:flex;flex-direction:column;gap:var(--sp-7);flex:1">
          <!-- source: local folder vs remote clone -->
          <kj-tabs variant="pills" [value]="source()" (valueChange)="source.set($any($event))">
            <kj-tab-list aria-label="Project source">
              @for (s of sources; track s.key) {
                <kj-tab [value]="s.key">
                  <app-icon [name]="s.icon" [color]="source() === s.key ? 'var(--ink)' : 'var(--ink-3)'" />{{ s.label }}
                </kj-tab>
              }
            </kj-tab-list>
          </kj-tabs>

          @if (source() === 'git') {
            <!-- repository url -->
            <div>
              <label class="field-label">Repository URL</label>
              <div style="display:flex;align-items:center;gap:var(--sp-4);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:0 var(--sp-5)">
                <app-icon name="git" color="var(--ink-4)" />
                <kj-input
                  #urlEl
                  class="bare-input"
                  [value]="url()"
                  (input)="url.set($any($event.target).value)"
                  placeholder="https://github.com/user/repo.git"
                />
              </div>
            </div>

            <!-- destination folder -->
            <div>
              <label class="field-label">Clone into</label>
              <div style="display:flex;gap:var(--sp-4)">
                <div style="flex:1;display:flex;align-items:center;gap:var(--sp-4);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:0 var(--sp-5)">
                  <app-icon name="folder" color="var(--ink-4)" />
                  <kj-input
                    class="bare-input"
                    [value]="dir()"
                    (input)="dir.set($any($event.target).value)"
                    placeholder="~/code"
                  />
                </div>
                <kj-button kjVariant="outline" (click)="browse()"><app-icon name="folderOpen" />Browse…</kj-button>
              </div>
              @if (destination()) {
                <small style="margin-top:var(--sp-3)">clones to → <code style="color:var(--ink-2)">{{ destination() }}</code></small>
              }
            </div>

            <!-- how the path is used -->
            <div style="display:flex;flex-direction:column;gap:var(--sp-3)">
              @for (m of cloneModes; track m.key) {
                <div
                  (click)="cloneMode.set(m.key)"
                  style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-5) var(--sp-6);border-radius:var(--r-md);cursor:pointer;background:var(--panel-2)"
                  [style.border]="'1px solid ' + (cloneMode() === m.key ? 'var(--ui-focus)' : 'var(--hair)')"
                >
                  <span
                    [style.border]="'1px solid ' + (cloneMode() === m.key ? 'var(--ui-focus)' : 'var(--hair-2)')"
                    [style.background]="cloneMode() === m.key ? 'var(--ui-fill)' : 'transparent'"
                    style="flex:none;width:var(--sp-7);height:var(--sp-7);border-radius:50%;display:grid;place-items:center"
                  >
                    @if (cloneMode() === m.key) { <app-icon size="md" name="check" color="var(--ui-on-fill)" /> }
                  </span>
                  <div style="flex:1">
                    <div style="color:var(--ink)">{{ m.label }}</div>
                    <small style="margin-top:var(--sp-1)">{{ m.hint }}</small>
                  </div>
                </div>
              }
            </div>

            <!-- shallow clone -->
            <div
              (click)="shallow.set(!shallow())"
              style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-5) var(--sp-6);border-radius:var(--r-md);cursor:pointer;background:var(--panel-2);border:1px solid var(--hair)"
            >
              <span
                [style.border]="'1px solid ' + (shallow() ? 'var(--ui-focus)' : 'var(--hair-2)')"
                [style.background]="shallow() ? 'var(--ui-fill)' : 'transparent'"
                style="flex:none;width:var(--sp-7);height:var(--sp-7);border-radius:4px;display:grid;place-items:center"
              >
                @if (shallow()) { <app-icon size="md" name="check" color="var(--ui-on-fill)" /> }
              </span>
              <div style="flex:1">
                <div style="color:var(--ink)">Shallow clone (depth 1)</div>
                <small style="margin-top:var(--sp-1)">fetches only the default branch at its tip — fastest</small>
              </div>
              <app-icon name="bolt" color="var(--ui-ink)" />
            </div>
          } @else {
          <!-- working directory -->
          <div>
            <label class="field-label">Working directory</label>
            <div style="display:flex;gap:var(--sp-4)">
              <div style="flex:1;display:flex;align-items:center;gap:var(--sp-4);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:0 var(--sp-5)">
                <app-icon name="folder" color="var(--ink-4)" />
                <kj-input
                  #dirEl
                  class="bare-input"
                  [value]="dir()"
                  (input)="dir.set($any($event.target).value)"
                  placeholder="~/code/my-repo"
                />
              </div>
              <kj-button kjVariant="outline" (click)="browse()"><app-icon name="folderOpen" />Browse…</kj-button>
            </div>
            @if (name()) {
              <small style="margin-top:var(--sp-3)">project name → <span style="color:var(--ink-2)">{{ name() }}</span></small>
            }
          </div>

          <!-- git: detected → static success alert, otherwise → init toggle -->
          @if (detectedGit()) {
            <div
              [style.background]="mix('var(--st-done)', 92)"
              [style.border]="'1px solid ' + mix('var(--st-done)', 70)"
              style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-5) var(--sp-6);border-radius:var(--r-md)"
            >
              <span style="flex:none;width:var(--sp-7);height:var(--sp-7);border-radius:4px;display:grid;place-items:center;background:var(--st-done)">
                <app-icon size="md" name="check" color="var(--ui-on-fill)" />
              </span>
              <div style="flex:1">
                <div style="color:var(--ink)">Git repository already exists</div>
                <small style="margin-top:var(--sp-1)">agents can branch + commit right away</small>
              </div>
              <app-icon name="git" color="var(--st-done)" />
            </div>
          } @else {
            <div
              (click)="gitInit.set(!gitInit())"
              style="display:flex;align-items:center;gap:var(--sp-5);padding:var(--sp-5) var(--sp-6);border-radius:var(--r-md);cursor:pointer;background:var(--panel-2);border:1px solid var(--hair)"
            >
              <span
                [style.border]="'1px solid ' + (gitInit() ? 'var(--ui-focus)' : 'var(--hair-2)')"
                [style.background]="gitInit() ? 'var(--ui-fill)' : 'transparent'"
                style="flex:none;width:var(--sp-7);height:var(--sp-7);border-radius:4px;display:grid;place-items:center"
              >
                @if (gitInit()) { <app-icon size="md" name="check" color="var(--ui-on-fill)" /> }
              </span>
              <div style="flex:1">
                <div style="color:var(--ink)">Run git init (no .git found)</div>
                <small style="margin-top:var(--sp-1)">initializes a repo so agents can branch + commit</small>
              </div>
              <app-icon name="bolt" color="var(--ui-ink)" />
            </div>
          }
          }

          <!-- icon -->
          <div>
            <label class="field-label">Icon</label>
            <!-- one choice out of a set: a radio group, not eight buttons.
                 role=radiogroup/radio, aria-checked and arrow-key selection all
                 come from kouji; the tile itself is the control, so the dot is
                 hidden in CSS below. -->
            <kj-radio-group class="ap-picker" orientation="horizontal" ariaLabel="Project icon"
                            [value]="icon()" (valueChange)="icon.set($any($event))">
              @for (ic of icons; track ic) {
                <kj-radio
                  [value]="ic"
                  [style.--ap-tile-ring]="icon() === ic ? 'var(--ui-focus)' : 'var(--hair)'"
                  [style.--ap-tile-bg]="icon() === ic ? 'var(--ui-sel)' : 'var(--panel-2)'"
                >
                  <app-icon [name]="ic" [color]="icon() === ic ? color() : 'var(--ink-3)'" />
                </kj-radio>
              }
            </kj-radio-group>
          </div>

          <!-- color -->
          <div>
            <label class="field-label">Color</label>
            <kj-radio-group class="ap-picker ap-swatches" orientation="horizontal" ariaLabel="Project colour"
                            [value]="color()" (valueChange)="color.set($any($event))">
              @for (c of colors; track c) {
                <kj-radio
                  [value]="c"
                  [style.--ap-swatch]="c"
                  [style.--ap-swatch-ring]="color() === c ? 'var(--ink)' : 'transparent'"
                  [style.--ap-swatch-shadow]="color() === c ? '0 0 0 2px var(--panel), 0 0 12px -2px ' + c : 'none'"
                ></kj-radio>
              }
            </kj-radio-group>
          </div>
        </div>

        <div style="padding:var(--sp-6) var(--sp-7);border-top:1px solid var(--hair);display:flex;justify-content:flex-end;gap:var(--sp-4);flex:none">
          <kj-button kjVariant="outline" (click)="ui.closeAddProject()">Cancel</kj-button>
          <kj-button kjVariant="default" [kjDisabled]="!canSubmit()" (click)="submit()"><app-icon name="plus" />Add project</kj-button>
        </div>
      </div>
    </kj-dialog-shell>
  `,
  styles: [
    `
      /* The panel box is the shared .kj-overlay-wrapper .kj-dialog recipe in
         styles.css; only this modal's width and height cap are per-instance.
         The field inputs wear .bare-input (also shared) — the surrounding box
         already draws the border/background/focus ring. */
      .kj-dialog {
        width: round(calc(480px * var(--density)), 1px);
        max-height: 90vh;
      }
      /* Both pickers are radio groups. kouji draws a dot plus a label slot;
         here the projected tile IS the control, so the dot is removed and the
         label slot becomes the hit target. Everything else — role=radio,
         aria-checked, arrow-key selection, roving tabindex — is the
         component's, not ours. ::ng-deep is required: kouji renders these
         spans with ViewEncapsulation.None, so they carry no scope attribute
         and an emulated rule from this component cannot reach them.

         Metrics are the mockup's (design/app.html:12917-12930): the icon tile
         is padding-sized, not a fixed box, and the swatch is a 26px circle. */
      .ap-picker { display: flex; flex-wrap: wrap; gap: var(--sp-3); }
      .ap-picker ::ng-deep .kj-radio-dot { display: none; }
      .ap-picker ::ng-deep .kj-radio-inner { display: block; }
      .ap-picker ::ng-deep .kj-radio-label {
        display: grid;
        place-items: center;
        padding: var(--sp-4);
        border-radius: var(--r-md);
        border: 1px solid var(--ap-tile-ring, var(--hair));
        background: var(--ap-tile-bg, var(--panel-2));
        cursor: pointer;
        transition: background .12s, border-color .12s;
      }
      .ap-picker ::ng-deep .kj-radio:focus-within .kj-radio-label { outline: 2px solid var(--ui-focus); outline-offset: 1px; }

      /* colour: a bare 26px circle — no tile around it */
      .ap-swatches { gap: var(--sp-4); }
      .ap-swatches ::ng-deep .kj-radio-label {
        padding: 0;
        width: round(calc(26px * var(--density)), 1px);
        height: round(calc(26px * var(--density)), 1px);
        border-radius: 50%;
        background: var(--ap-swatch);
        border: 2px solid var(--ap-swatch-ring, transparent);
        box-shadow: var(--ap-swatch-shadow, none);
      }
    `,
  ],
})
export class AddProjectModalComponent {
  readonly ui = inject(UiStore);
  private projectActions = inject(ProjectActionsService);
  private projects = inject(ProjectsStore);
  private settings = inject(SettingsStore);
  readonly icons = PROJECT_ICONS;
  readonly colors = PROJECT_COLORS;
  readonly mix = mix;

  readonly sources = [
    { key: "local" as const, label: "Local folder", icon: "folder" },
    { key: "git" as const, label: "From Git URL", icon: "git" },
  ];
  readonly cloneModes = [
    {
      key: "root" as const,
      label: "Use path as root",
      hint: "the clone lands in a repo-named subfolder",
    },
    {
      key: "project" as const,
      label: "Use path as the project",
      hint: 'repo content lands directly in this folder (clone into ".")',
    },
  ];

  readonly dir = signal("");
  readonly icon = signal(PROJECT_ICONS[0]);
  readonly color = signal(PROJECT_COLORS[0]);
  readonly gitInit = signal(true);
  readonly source = signal<"local" | "git">("local");
  readonly url = signal("");
  readonly cloneMode = signal<"root" | "project">("root");
  readonly shallow = signal(true);

  /** Last URL path segment, ".git" stripped — mirrors the backend derivation. */
  readonly repoName = computed(() => {
    const u = this.url().trim().replace(/\/+$/, "");
    const last = u.split(/[/:]/).pop() || "";
    return last.replace(/\.git$/, "");
  });
  private readonly dirName = computed(() => {
    const d = this.dir().replace(/[/\\]+$/, "");
    return d ? d.split(/[/\\]/).pop() || "" : "";
  });
  readonly name = computed(() =>
    this.source() === "git" && this.cloneMode() === "root" ? this.repoName() : this.dirName(),
  );
  /** Where the repo content will land, previewed under the destination field. */
  readonly destination = computed(() => {
    const d = this.dir().trim().replace(/[/\\]+$/, "");
    if (!d) return "";
    if (this.cloneMode() === "project") return d;
    const sep = d.includes("\\") ? "\\" : "/";
    return this.repoName() ? d + sep + this.repoName() : "";
  });
  readonly canSubmit = computed(() =>
    this.source() === "git"
      ? !!this.url().trim() && !!this.destination()
      : !!this.dir().trim(),
  );
  readonly detectedGit = signal(false);

  private dirEl = viewChild<KjInputComponent>("dirEl");

  constructor() {
    // Esc / outside-click close the overlay, not the store — clear the flag on
    // teardown so the two can never drift.
    inject(DestroyRef).onDestroy(() => this.ui.closeAddProject());
    // Focus the directory field once the view exists. The rAF matters: the
    // dialog runs a focus trap that pulls focus to the first tabbable element
    // on open, and since the source picker became a tab strip that element is
    // now a TAB — so focusing in the same frame gets overridden and the user
    // lands on the picker instead of the field they came here to fill in.
    afterNextRender(() => requestAnimationFrame(() => this.dirEl()?.focus()));
    effect(() => {
      const d = this.dir().trim();
      if (!d || this.source() !== "local") {
        this.detectedGit.set(false);
        this.gitInit.set(true);
        return;
      }
      void this.projects.detectGit(d).then((found) => {
        this.detectedGit.set(found);
        this.gitInit.set(!found);
      });
    });
  }

  async browse() {
    // Start where the user already is (typed/picked dir), else the configured
    // projects folder, else the OS default.
    const start = this.dir().trim() || this.settings.settings().projectsRoot || undefined;
    const dir = await this.projects.pickDirectory(start);
    if (dir) this.dir.set(dir);
  }

  submit() {
    if (!this.canSubmit()) return;
    const git = this.source() === "git";
    this.projectActions.addProject({
      path: this.dir().trim(),
      name: this.name(),
      icon: this.icon(),
      color: this.color(),
      gitInit: this.gitInit(),
      // clone params ride along; the backend decides what to do with them
      sourceUrl: git ? this.url().trim() : undefined,
      sourceMode: git ? this.cloneMode() : undefined,
      depth: git && this.shallow() ? 1 : undefined,
    });
  }
}
