import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  ElementRef,
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

@Component({
  selector: "app-add-project-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div
      (click)="ui.closeAddProject()"
      style="position:fixed;inset:0;z-index:60;display:grid;place-items:center;padding:var(--sp-9);background:var(--scrim);backdrop-filter:blur(3px)"
    >
      <div
        class="surface rise"
        (click)="$event.stopPropagation()"
        style="width:480px;padding:0;overflow:hidden;box-shadow:var(--shadow)"
      >
        <div style="padding:var(--sp-6) var(--sp-7);border-bottom:1px solid var(--hair);display:flex;align-items:center;gap:var(--sp-4)">
          <span
            [style.background]="mix(color(), 82)"
            [style.border]="'1px solid ' + mix(color(), 55)"
            style="flex:none;width:var(--sp-9);height:var(--sp-9);border-radius:7px;display:grid;place-items:center"
          >
            <app-icon [name]="icon()" size="sm" [color]="color()" />
          </span>
          <span class="disp" style="font-size:var(--fs-lg);font-weight:600;white-space:nowrap">Add project</span>
          <span class="chip" style="margin-left:auto;font-size:var(--fs-2xs)">git repository</span>
        </div>

        <div style="padding:var(--sp-7);display:flex;flex-direction:column;gap:var(--sp-7)">
          <!-- source: local folder vs remote clone -->
          <div style="display:flex;gap:var(--sp-3);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:var(--sp-2)">
            @for (s of sources; track s.key) {
              <button
                class="btn"
                (click)="source.set(s.key)"
                [style.background]="source() === s.key ? 'var(--ui-sel)' : 'transparent'"
                [style.border]="'1px solid ' + (source() === s.key ? 'var(--ui-focus)' : 'transparent')"
                style="flex:1;justify-content:center;border-radius:var(--r-sm);padding:var(--sp-4)"
              >
                <app-icon [name]="s.icon" size="sm" [color]="source() === s.key ? 'var(--ink)' : 'var(--ink-3)'" />{{ s.label }}
              </button>
            }
          </div>

          @if (source() === 'git') {
            <!-- repository url -->
            <div>
              <label class="field-label">Repository URL</label>
              <div style="display:flex;align-items:center;gap:var(--sp-4);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:0 var(--sp-5)">
                <app-icon name="git" size="sm" color="var(--ink-4)" />
                <input
                  #urlEl
                  [value]="url()"
                  (input)="url.set($any($event.target).value)"
                  placeholder="https://github.com/user/repo.git"
                  style="flex:1;min-width:0;background:transparent;border:none;outline:none;padding:var(--sp-5) 0;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-ui)"
                />
              </div>
            </div>

            <!-- destination folder -->
            <div>
              <label class="field-label">Clone into</label>
              <div style="display:flex;gap:var(--sp-4)">
                <div style="flex:1;display:flex;align-items:center;gap:var(--sp-4);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:0 var(--sp-5)">
                  <app-icon name="folder" size="sm" color="var(--ink-4)" />
                  <input
                    [value]="dir()"
                    (input)="dir.set($any($event.target).value)"
                    placeholder="~/code"
                    style="flex:1;min-width:0;background:transparent;border:none;outline:none;padding:var(--sp-5) 0;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-ui)"
                  />
                </div>
                <button class="btn ghost-hair" (click)="browse()"><app-icon name="folderOpen" size="sm" />Browse…</button>
              </div>
              @if (destination()) {
                <div style="font-size:var(--fs-xs);color:var(--ink-4);margin-top:var(--sp-3)">clones to → <span style="color:var(--ink-2);font-family:var(--font-mono)">{{ destination() }}</span></div>
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
                    @if (cloneMode() === m.key) { <app-icon name="check" size="sm" [px]="11" color="var(--ui-on-fill)" /> }
                  </span>
                  <div style="flex:1">
                    <div style="font-size:var(--fs-sm);color:var(--ink)">{{ m.label }}</div>
                    <div style="font-size:var(--fs-2xs);color:var(--ink-4);margin-top:var(--sp-1)">{{ m.hint }}</div>
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
                @if (shallow()) { <app-icon name="check" size="sm" [px]="11" color="var(--ui-on-fill)" /> }
              </span>
              <div style="flex:1">
                <div style="font-size:var(--fs-sm);color:var(--ink)">Shallow clone (depth 1)</div>
                <div style="font-size:var(--fs-2xs);color:var(--ink-4);margin-top:var(--sp-1)">fetches only the default branch at its tip — fastest</div>
              </div>
              <app-icon name="bolt" size="sm" color="var(--ui-ink)" />
            </div>
          } @else {
          <!-- working directory -->
          <div>
            <label class="field-label">Working directory</label>
            <div style="display:flex;gap:var(--sp-4)">
              <div style="flex:1;display:flex;align-items:center;gap:var(--sp-4);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:0 var(--sp-5)">
                <app-icon name="folder" size="sm" color="var(--ink-4)" />
                <input
                  #dirEl
                  [value]="dir()"
                  (input)="dir.set($any($event.target).value)"
                  placeholder="~/code/my-repo"
                  style="flex:1;min-width:0;background:transparent;border:none;outline:none;padding:var(--sp-5) 0;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-ui)"
                />
              </div>
              <button class="btn ghost-hair" (click)="browse()"><app-icon name="folderOpen" size="sm" />Browse…</button>
            </div>
            @if (name()) {
              <div style="font-size:var(--fs-xs);color:var(--ink-4);margin-top:var(--sp-3)">project name → <span style="color:var(--ink-2)">{{ name() }}</span></div>
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
                <app-icon name="check" size="sm" [px]="11" color="var(--ui-on-fill)" />
              </span>
              <div style="flex:1">
                <div style="font-size:var(--fs-sm);color:var(--ink)">Git repository already exists</div>
                <div style="font-size:var(--fs-2xs);color:var(--ink-4);margin-top:var(--sp-1)">agents can branch + commit right away</div>
              </div>
              <app-icon name="git" size="sm" color="var(--st-done)" />
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
                @if (gitInit()) { <app-icon name="check" size="sm" [px]="11" color="var(--ui-on-fill)" /> }
              </span>
              <div style="flex:1">
                <div style="font-size:var(--fs-sm);color:var(--ink)">Run git init (no .git found)</div>
                <div style="font-size:var(--fs-2xs);color:var(--ink-4);margin-top:var(--sp-1)">initializes a repo so agents can branch + commit</div>
              </div>
              <app-icon name="bolt" size="sm" color="var(--ui-ink)" />
            </div>
          }
          }

          <!-- icon -->
          <div>
            <label class="field-label">Icon</label>
            <div style="display:flex;gap:var(--sp-3);flex-wrap:wrap">
              @for (ic of icons; track ic) {
                <button
                  class="btn"
                  (click)="icon.set(ic)"
                  [style.border]="'1px solid ' + (icon() === ic ? 'var(--ui-focus)' : 'var(--hair)')"
                  [style.background]="icon() === ic ? 'var(--ui-sel)' : 'var(--panel-2)'"
                  style="padding:var(--sp-4);border-radius:var(--r-md)"
                >
                  <app-icon [name]="ic" size="sm" [color]="icon() === ic ? color() : 'var(--ink-3)'" />
                </button>
              }
            </div>
          </div>

          <!-- color -->
          <div>
            <label class="field-label">Color</label>
            <div style="display:flex;gap:var(--sp-4)">
              @for (c of colors; track c) {
                <button
                  (click)="color.set(c)"
                  [style.background]="c"
                  [style.border]="'2px solid ' + (color() === c ? 'var(--ink)' : 'transparent')"
                  [style.box-shadow]="color() === c ? '0 0 0 2px var(--panel), 0 0 12px -2px ' + c : 'none'"
                  style="width:var(--ctl-h);height:var(--ctl-h);border-radius:50%;cursor:pointer"
                ></button>
              }
            </div>
          </div>
        </div>

        <div style="padding:var(--sp-6) var(--sp-7);border-top:1px solid var(--hair);display:flex;justify-content:flex-end;gap:var(--sp-4)">
          <button class="btn ghost-hair" (click)="ui.closeAddProject()">Cancel</button>
          <button class="btn primary" [disabled]="!canSubmit()" (click)="submit()"><app-icon name="plus" size="sm" />Add project</button>
        </div>
      </div>
    </div>
  `,
})
export class AddProjectModalComponent implements AfterViewInit {
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

  private dirEl = viewChild<ElementRef<HTMLInputElement>>("dirEl");

  constructor() {
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

  ngAfterViewInit() {
    this.dirEl()?.nativeElement.focus();
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
