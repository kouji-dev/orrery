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
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";
import { ProjectsStore } from "../stores/projects.store";
import { mix } from "../utils";

@Component({
  selector: "app-add-project-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div
      (click)="store.closeAddProject()"
      style="position:fixed;inset:0;z-index:60;display:grid;place-items:center;padding:24px;background:rgba(0,0,0,0.5);backdrop-filter:blur(3px)"
    >
      <div
        class="surface rise"
        (click)="$event.stopPropagation()"
        style="width:480px;padding:0;overflow:hidden;box-shadow:var(--shadow)"
      >
        <div style="padding:14px 18px;border-bottom:1px solid var(--hair);display:flex;align-items:center;gap:9px">
          <span
            [style.background]="mix(color(), 82)"
            [style.border]="'1px solid ' + mix(color(), 55)"
            style="flex:none;width:24px;height:24px;border-radius:7px;display:grid;place-items:center"
          >
            <app-icon [name]="icon()" size="sm" [color]="color()" />
          </span>
          <span class="disp" style="font-size:14px;font-weight:600;white-space:nowrap">Add project</span>
          <span class="chip" style="margin-left:auto;font-size:9.5px">git repository</span>
        </div>

        <div style="padding:18px;display:flex;flex-direction:column;gap:16px">
          <!-- working directory -->
          <div>
            <label class="field-label">Working directory</label>
            <div style="display:flex;gap:8px">
              <div style="flex:1;display:flex;align-items:center;gap:8px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md);padding:0 10px">
                <app-icon name="folder" size="sm" color="var(--ink-4)" />
                <input
                  #dirEl
                  [value]="dir()"
                  (input)="dir.set($any($event.target).value)"
                  placeholder="~/code/my-repo"
                  style="flex:1;min-width:0;background:transparent;border:none;outline:none;padding:10px 0;color:var(--ink);font-family:var(--font-mono);font-size:12.5px"
                />
              </div>
              <button class="btn ghost-hair" (click)="picker.click()"><app-icon name="folderOpen" size="sm" />Browse…</button>
              <input #picker type="file" [attr.webkitdirectory]="''" [attr.directory]="''" multiple style="display:none" (change)="onPick($event)" />
            </div>
            @if (name()) {
              <div style="font-size:10px;color:var(--ink-4);margin-top:6px">project name → <span style="color:var(--ink-2)">{{ name() }}</span></div>
            }
          </div>

          <!-- git detection + init -->
          <div
            (click)="gitInit.set(!gitInit())"
            [style.background]="detectedGit() ? mix('var(--st-done)', 92) : 'var(--panel-2)'"
            [style.border]="'1px solid ' + (detectedGit() ? mix('var(--st-done)', 70) : 'var(--hair)')"
            style="display:flex;align-items:center;gap:10px;padding:10px 12px;border-radius:var(--r-md);cursor:pointer"
          >
            <span
              [style.border]="'1px solid ' + (gitInit() ? 'var(--accent)' : 'var(--hair-2)')"
              [style.background]="gitInit() ? 'var(--accent)' : 'transparent'"
              style="flex:none;width:16px;height:16px;border-radius:4px;display:grid;place-items:center"
            >
              @if (gitInit()) { <app-icon name="check" size="sm" [px]="11" color="#06070b" /> }
            </span>
            <div style="flex:1">
              <div style="font-size:11.5px;color:var(--ink)">{{ detectedGit() ? 'Existing git repository detected' : 'Run git init (no .git found)' }}</div>
              <div style="font-size:9.5px;color:var(--ink-4);margin-top:2px">{{ detectedGit() ? 'you can still re-initialize' : 'initializes a repo so agents can branch + commit' }}</div>
            </div>
            <app-icon [name]="detectedGit() ? 'git' : 'bolt'" size="sm" [color]="detectedGit() ? 'var(--st-done)' : 'var(--accent)'" />
          </div>

          <!-- icon -->
          <div>
            <label class="field-label">Icon</label>
            <div style="display:flex;gap:6px;flex-wrap:wrap">
              @for (ic of icons; track ic) {
                <button
                  class="btn"
                  (click)="icon.set(ic)"
                  [style.border]="'1px solid ' + (icon() === ic ? 'var(--accent)' : 'var(--hair)')"
                  [style.background]="icon() === ic ? mix('var(--accent)', 90) : 'var(--panel-2)'"
                  style="padding:8px;border-radius:var(--r-md)"
                >
                  <app-icon [name]="ic" size="sm" [color]="icon() === ic ? color() : 'var(--ink-3)'" />
                </button>
              }
            </div>
          </div>

          <!-- color -->
          <div>
            <label class="field-label">Color</label>
            <div style="display:flex;gap:8px">
              @for (c of colors; track c) {
                <button
                  (click)="color.set(c)"
                  [style.background]="c"
                  [style.border]="'2px solid ' + (color() === c ? 'var(--ink)' : 'transparent')"
                  [style.box-shadow]="color() === c ? '0 0 0 2px var(--panel), 0 0 12px -2px ' + c : 'none'"
                  style="width:26px;height:26px;border-radius:50%;cursor:pointer"
                ></button>
              }
            </div>
          </div>
        </div>

        <div style="padding:12px 18px;border-top:1px solid var(--hair);display:flex;justify-content:flex-end;gap:8px">
          <button class="btn ghost-hair" (click)="store.closeAddProject()">Cancel</button>
          <button class="btn primary" [disabled]="!dir().trim()" (click)="submit()"><app-icon name="plus" size="sm" />Add project</button>
        </div>
      </div>
    </div>
  `,
})
export class AddProjectModalComponent implements AfterViewInit {
  readonly store = inject(OrchestraStore);
  private projects = inject(ProjectsStore);
  readonly icons = PROJECT_ICONS;
  readonly colors = PROJECT_COLORS;
  readonly mix = mix;

  readonly dir = signal("");
  readonly icon = signal(PROJECT_ICONS[0]);
  readonly color = signal(PROJECT_COLORS[0]);
  readonly gitInit = signal(true);

  readonly name = computed(() => {
    const d = this.dir();
    return d ? d.replace(/\/+$/, "").split("/").pop() || "" : "";
  });
  readonly detectedGit = signal(false);

  private dirEl = viewChild<ElementRef<HTMLInputElement>>("dirEl");

  constructor() {
    effect(() => {
      const d = this.dir().trim();
      if (!d) {
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

  onPick(e: Event) {
    const files = (e.target as HTMLInputElement).files;
    if (files && files.length) {
      const f = files[0] as File & { webkitRelativePath?: string };
      const rel = f.webkitRelativePath || f.name;
      const folder = rel.split("/")[0];
      this.dir.set("~/" + folder);
    }
  }

  submit() {
    const path = this.dir().trim();
    if (!path) return;
    this.store.addProject({
      path,
      name: this.name(),
      icon: this.icon(),
      color: this.color(),
      gitInit: this.gitInit(),
    });
  }
}
