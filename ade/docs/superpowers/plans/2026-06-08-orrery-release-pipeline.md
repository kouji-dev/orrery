# Orrery Release Pipeline + Auto-Updater Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A manual GitHub Actions workflow in `kouji-dev/orrery` that builds the Windows installer + signed updater artifacts and publishes a release into `kouji-dev/orrery-releases`, plus an in-app auto-updater that runs on launch through a routed loading screen.

**Architecture:** Two-phase workflow (build on `windows-latest` → publish to the other repo via a fine-grained PAT + hand-written `latest.json`). The boot splash moves out of `index.html` into a routed `LoadingComponent` that hosts an `UpdaterService`; `AppComponent` becomes a `<router-outlet />` and today's shell moves to `ShellComponent`.

**Tech Stack:** Tauri 2 (`tauri-plugin-updater`, `tauri-plugin-process`), Angular 20 (standalone + router + signals), Vitest (jsdom, no TestBed), Node ESM build scripts, GitHub Actions, `gh` CLI.

---

## File Structure

**Frontend (committed in `orrery`):**
- Create `src/app/updater/updater.ts` — `Updater` port interface, `UpdateHandle`, `UpdateOutcome`, `UPDATER` injection token.
- Create `src/app/updater/tauri-updater.ts` — real `Updater` impl over `@tauri-apps/plugin-updater` + `@tauri-apps/plugin-process`.
- Create `src/app/updater/updater.service.ts` — orchestrates check → download → relaunch; exposes `status`/`progress` signals.
- Create `src/app/updater/updater.service.spec.ts` — unit tests for the service.
- Create `src/app/shell/shell.component.ts` — today's `AppComponent` template + logic verbatim (the real app).
- Create `src/app/loading/loading.component.ts` — ported splash visuals + boot/navigation logic.
- Create `src/app/loading/loading.component.spec.ts` — navigation-decision tests.
- Modify `src/app/app.component.ts` — becomes `<router-outlet />` only.
- Modify `src/app/app.routes.ts` — loading (default) + shell (`/app`) routes.
- Modify `src/app/app.config.ts` — provide `UPDATER`.
- Modify `src/index.html` — strip inline splash; dark first-paint background.

**Backend / config (committed in `orrery`):**
- Modify `src-tauri/Cargo.toml` — add updater + process plugin crates.
- Modify `src-tauri/src/lib.rs` — register both plugins.
- Modify `src-tauri/tauri.conf.json` — `bundle.createUpdaterArtifacts`, `plugins.updater`.
- Modify `src-tauri/capabilities/default.json` — updater + process permissions.
- Modify `package.json` — add the two `@tauri-apps/plugin-*` JS deps.

**Release pipeline (committed in `orrery`):**
- Create `scripts/release/stamp-version.mjs` + `scripts/release/stamp-version.spec.ts`.
- Create `scripts/release/make-latest-json.mjs` + `scripts/release/make-latest-json.spec.ts`.
- Modify `vitest.config.ts` — also include `scripts/**/*.spec.ts`.
- Create `.github/workflows/release.yml`.

**Manual prerequisites (no code):** public `orrery-releases` with a `main` branch, fine-grained PAT secret `RELEASES_TOKEN`, signing-key secrets `TAURI_SIGNING_PRIVATE_KEY` + `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

---

## Task 1: Updater port + service (TDD)

**Files:**
- Create: `src/app/updater/updater.ts`
- Create: `src/app/updater/updater.service.ts`
- Test: `src/app/updater/updater.service.spec.ts`

- [ ] **Step 1: Define the port**

Create `src/app/updater/updater.ts`:

```ts
import { InjectionToken } from '@angular/core';

/** Outcome of a launch-time update check. `updating` means the app is about to
 *  relaunch into a new version — the caller must NOT navigate onward. */
export type UpdateOutcome = 'no-update' | 'updating';

/** A pending update. `downloadAndInstall` reports bytes via `onProgress`
 *  (`total` is null when the server sends no content-length). */
export interface UpdateHandle {
  version: string;
  downloadAndInstall(onProgress: (downloaded: number, total: number | null) => void): Promise<void>;
}

/** Thin, mockable boundary over the Tauri updater/process plugins. */
export interface Updater {
  /** True only when running inside the Tauri webview. */
  isAvailable(): boolean;
  /** Resolve an available update, or null. Rejects/throws on transport errors. */
  check(timeoutMs: number): Promise<UpdateHandle | null>;
  /** Restart the app (does not return in a real Tauri process). */
  relaunch(): Promise<void>;
}

export const UPDATER = new InjectionToken<Updater>('UPDATER');
```

- [ ] **Step 2: Write the failing test**

Create `src/app/updater/updater.service.spec.ts`:

```ts
import { Injector, runInInjectionContext } from '@angular/core';
import { describe, expect, it, vi } from 'vitest';
import { Updater, UpdateHandle, UPDATER } from './updater';
import { UpdaterService } from './updater.service';

function make(updater: Partial<Updater>): UpdaterService {
  const full: Updater = {
    isAvailable: () => true,
    check: async () => null,
    relaunch: async () => {},
    ...updater,
  };
  const injector = Injector.create({ providers: [{ provide: UPDATER, useValue: full }] });
  return runInInjectionContext(injector, () => new UpdaterService());
}

describe('UpdaterService.run', () => {
  it('skips when not running under Tauri', async () => {
    const relaunch = vi.fn(async () => {});
    const svc = make({ isAvailable: () => false, relaunch });
    expect(await svc.run()).toBe('no-update');
    expect(relaunch).not.toHaveBeenCalled();
  });

  it('returns no-update when no update is available', async () => {
    expect(await make({ check: async () => null }).run()).toBe('no-update');
  });

  it('downloads, tracks progress, relaunches, returns updating', async () => {
    const relaunch = vi.fn(async () => {});
    const handle: UpdateHandle = {
      version: '1.2.0',
      downloadAndInstall: async (onProgress) => {
        onProgress(50, 100);
        onProgress(100, 100);
      },
    };
    const svc = make({ check: async () => handle, relaunch });
    const outcome = await svc.run();
    expect(outcome).toBe('updating');
    expect(svc.progress()).toBe(1);
    expect(svc.status()).toContain('1.2.0');
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it('swallows check errors and returns no-update', async () => {
    const svc = make({ check: async () => { throw new Error('offline'); } });
    expect(await svc.run()).toBe('no-update');
  });
});
```

- [ ] **Step 3: Run test to verify it fails**

Run: `pnpm test -- updater.service`
Expected: FAIL — `Cannot find module './updater.service'`.

- [ ] **Step 4: Implement the service**

Create `src/app/updater/updater.service.ts`:

```ts
import { inject, Injectable, signal } from '@angular/core';
import { UPDATER, UpdateOutcome } from './updater';

const CHECK_TIMEOUT_MS = 10_000;

@Injectable({ providedIn: 'root' })
export class UpdaterService {
  private readonly updater = inject(UPDATER);

  /** Human-readable phase shown on the loading screen. */
  readonly status = signal('');
  /** Download progress 0..1 (0 while indeterminate). */
  readonly progress = signal(0);

  /** Best-effort: any failure resolves `no-update` so boot is never blocked. */
  async run(): Promise<UpdateOutcome> {
    if (!this.updater.isAvailable()) return 'no-update';
    try {
      const update = await this.updater.check(CHECK_TIMEOUT_MS);
      if (!update) return 'no-update';
      this.status.set(`downloading update · ${update.version}`);
      await update.downloadAndInstall((downloaded, total) => {
        this.progress.set(total && total > 0 ? downloaded / total : 0);
      });
      this.status.set('restarting');
      await this.updater.relaunch();
      return 'updating';
    } catch {
      return 'no-update';
    }
  }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm test -- updater.service`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add src/app/updater/updater.ts src/app/updater/updater.service.ts src/app/updater/updater.service.spec.ts
git commit -m "feat(updater): launch-time update service over a mockable port"
```

---

## Task 2: Real Tauri updater implementation

No unit test — it is the thin adapter the tests mock out. Verified by the production build later.

**Files:**
- Create: `src/app/updater/tauri-updater.ts`

- [ ] **Step 1: Implement the adapter**

Create `src/app/updater/tauri-updater.ts`:

```ts
import { check, type DownloadEvent } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { Updater, UpdateHandle } from './updater';

/** Real boundary over the Tauri plugins. `isAvailable` checks the injected
 *  Tauri internals so the app still boots under `ng serve` (plain browser). */
export class TauriUpdater implements Updater {
  isAvailable(): boolean {
    return typeof (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined';
  }

  async check(timeoutMs: number): Promise<UpdateHandle | null> {
    const update = await check({ timeout: timeoutMs });
    if (!update) return null;
    return {
      version: update.version,
      downloadAndInstall: (onProgress) => {
        let downloaded = 0;
        let total: number | null = null;
        return update.downloadAndInstall((e: DownloadEvent) => {
          if (e.event === 'Started') total = e.data.contentLength ?? null;
          else if (e.event === 'Progress') {
            downloaded += e.data.chunkLength;
            onProgress(downloaded, total);
          }
        });
      },
    };
  }

  relaunch(): Promise<void> {
    return relaunch();
  }
}
```

- [ ] **Step 2: Add the JS deps**

Run: `pnpm add @tauri-apps/plugin-updater@^2 @tauri-apps/plugin-process@^2`
Expected: both appear under `dependencies` in `package.json`.

- [ ] **Step 3: Verify it type-checks via the existing build**

Run: `pnpm build`
Expected: build succeeds (the file is not yet imported anywhere, so this only confirms it compiles once referenced in Task 6 — re-run there). Commit now.

- [ ] **Step 4: Commit**

```bash
git add src/app/updater/tauri-updater.ts package.json pnpm-lock.yaml
git commit -m "feat(updater): real Tauri updater/process adapter"
```

---

## Task 3: Extract today's shell into ShellComponent

`AppComponent`'s current template/logic moves verbatim into `ShellComponent`, **minus** the `__orreryAppReady()` call (the loading screen now owns readiness).

**Files:**
- Create: `src/app/shell/shell.component.ts`

- [ ] **Step 1: Create the shell component**

Create `src/app/shell/shell.component.ts` (selector `app-shell`, copied from the current `app.component.ts`, with `ngAfterViewInit`/`AfterViewInit` removed):

```ts
import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ContextMenuComponent } from '../context-menu/context-menu.component';
import { AddProjectModalComponent } from '../modals/add-project-modal.component';
import { SpawnModalComponent } from '../modals/spawn-modal.component';
import { UiStore } from '../ui/ui.store';
import { OverviewComponent } from '../overview/overview.component';
import { RightPanelComponent } from '../right-panel/right-panel.component';
import { SidebarComponent } from '../sidebar/sidebar.component';
import { CompactRailComponent } from '../sidebar/compact-rail.component';
import { StatusBarComponent } from '../status-bar/status-bar.component';
import { TopBarComponent } from '../top-bar/top-bar.component';
import { TweaksPanelComponent } from '../tweaks/tweaks-panel.component';
import { DevPanelComponent } from '../dev-tools/dev-panel.component';
import { PaneManagerComponent } from '../workspace/pane-manager.component';

declare const ngDevMode: boolean | undefined;

@Component({
  selector: 'app-shell',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    TopBarComponent,
    SidebarComponent,
    CompactRailComponent,
    OverviewComponent,
    PaneManagerComponent,
    RightPanelComponent,
    StatusBarComponent,
    SpawnModalComponent,
    AddProjectModalComponent,
    ContextMenuComponent,
    TweaksPanelComponent,
    DevPanelComponent,
  ],
  template: `
    <div class="bg-texture"></div>
    <div class="bg-glow"></div>
    <div class="shell">
      <app-top-bar />

      <div
        class="workspace"
        [class.no-right]="!ui.tweaks().rightPanel"
        [class.compact]="ui.sidebarCompact()"
      >
        @if (ui.sidebarCompact()) {
          <app-compact-rail />
        } @else {
          <app-sidebar />
        }

        @if (ui.activeTab() === 'orchestrator') {
          <app-overview />
        } @else {
          <app-pane-manager [tabId]="ui.activeTab()" />
        }

        @if (ui.tweaks().rightPanel) {
          <app-right-panel />
        }
      </div>

      <app-status-bar />
    </div>

    @if (ui.spawning()) {
      <app-spawn-modal />
    }
    @if (ui.addingProject()) {
      <app-add-project-modal />
    }
    <app-context-menu />
    <app-tweaks-panel />
    @if (dev) {
      <app-dev-panel />
    }
  `,
})
export class ShellComponent {
  readonly ui = inject(UiStore);
  readonly dev = typeof ngDevMode !== 'undefined' && !!ngDevMode;
}
```

- [ ] **Step 2: Verify it compiles**

Run: `pnpm build`
Expected: PASS (the component is valid; wired into routes in Task 5).

- [ ] **Step 3: Commit**

```bash
git add src/app/shell/shell.component.ts
git commit -m "refactor(shell): extract app shell into ShellComponent"
```

---

## Task 4: LoadingComponent (TDD on navigation)

**Files:**
- Create: `src/app/loading/loading.component.ts`
- Test: `src/app/loading/loading.component.spec.ts`

- [ ] **Step 1: Write the failing navigation test**

Create `src/app/loading/loading.component.spec.ts`:

```ts
import { Injector, runInInjectionContext } from '@angular/core';
import { Router } from '@angular/router';
import { describe, expect, it, vi } from 'vitest';
import { UpdateOutcome } from '../updater/updater';
import { UpdaterService } from '../updater/updater.service';
import { LoadingComponent } from './loading.component';

function make(outcome: UpdateOutcome | 'pending') {
  const navigateByUrl = vi.fn(async () => true);
  const run = vi.fn(
    (): Promise<UpdateOutcome> =>
      outcome === 'pending' ? new Promise<UpdateOutcome>(() => {}) : Promise.resolve(outcome),
  );
  const injector = Injector.create({
    providers: [
      { provide: Router, useValue: { navigateByUrl } },
      { provide: UpdaterService, useValue: { run, status: () => '', progress: () => 0 } },
    ],
  });
  const cmp = runInInjectionContext(injector, () => new LoadingComponent());
  cmp.minMs = 0;
  cmp.safetyMs = 50_000;
  return { cmp, navigateByUrl };
}

describe('LoadingComponent.boot', () => {
  it('navigates to /app when there is no update', async () => {
    const { cmp, navigateByUrl } = make('no-update');
    await cmp.boot();
    expect(navigateByUrl).toHaveBeenCalledWith('/app');
  });

  it('does NOT navigate when an update is installing (it will relaunch)', async () => {
    const { cmp, navigateByUrl } = make('updating');
    await cmp.boot();
    expect(navigateByUrl).not.toHaveBeenCalled();
  });

  it('navigates anyway if the check hangs past the safety timeout', async () => {
    const { cmp, navigateByUrl } = make('pending');
    cmp.safetyMs = 0; // timeout fires immediately
    await cmp.boot();
    expect(navigateByUrl).toHaveBeenCalledWith('/app');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- loading.component`
Expected: FAIL — `Cannot find module './loading.component'`.

- [ ] **Step 3: Implement the component**

Create `src/app/loading/loading.component.ts`. The epicycle draw is ported from the old `index.html` splash and runs in `ngAfterViewInit`; `boot()` holds the testable navigation logic.

```ts
import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  inject,
  viewChild,
} from '@angular/core';
import { Router } from '@angular/router';
import { UpdaterService } from '../updater/updater.service';
import { UpdateOutcome } from '../updater/updater';

@Component({
  selector: 'app-loading',
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [`
    :host {
      position: fixed;
      inset: 0;
      z-index: 99999;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      background: #07080d;
      color: #e8ebf2;
      font-family: 'JetBrains Mono', ui-monospace, monospace;
      background-image:
        linear-gradient(rgba(255, 255, 255, 0.022) 1px, transparent 1px),
        linear-gradient(90deg, rgba(255, 255, 255, 0.022) 1px, transparent 1px),
        radial-gradient(46% 38% at 50% 43%, rgba(168, 85, 247, 0.16), transparent 72%);
      background-size: 34px 34px, 34px 34px, 100% 100%, 100% 100%;
    }
    .ob-mark { filter: drop-shadow(0 0 26px rgba(168, 85, 247, 0.4)); }
    .ob-word {
      font-family: 'Space Grotesk', sans-serif;
      font-weight: 600;
      font-size: 36px;
      letter-spacing: 0.01em;
      margin-top: 28px;
    }
    .ob-word .o { color: #a855f7; }
    .ob-status {
      font-size: 10.5px;
      letter-spacing: 0.2em;
      text-transform: uppercase;
      color: #6b7488;
      margin-top: 15px;
      height: 13px;
    }
    .ob-bar {
      margin-top: 20px;
      width: 188px;
      height: 2px;
      border-radius: 2px;
      background: rgba(255, 255, 255, 0.08);
      overflow: hidden;
    }
    .ob-bar > i {
      display: block;
      height: 100%;
      width: 0;
      border-radius: 2px;
      background: linear-gradient(90deg, #ff5d9e, #a855f7, #22d3ee);
    }
  `],
  template: `
    <div class="ob-mark"><svg #svg width="168" height="168" viewBox="0 0 100 100" fill="none"></svg></div>
    <div class="ob-word"><span class="o">O</span>rrery</div>
    <div class="ob-status">{{ updater.status() || 'initializing orchestrator' }}</div>
    <div class="ob-bar"><i #bar></i></div>
  `,
})
export class LoadingComponent implements AfterViewInit {
  readonly updater = inject(UpdaterService);
  private readonly router = inject(Router);

  /** Cosmetic minimum the splash stays up (ms). */
  minMs = 1600;
  /** Hard backstop: navigate onward even if the updater hangs (ms). */
  safetyMs = 12_000;

  private readonly svg = viewChild<ElementRef<SVGSVGElement>>('svg');
  private readonly bar = viewChild<ElementRef<HTMLElement>>('bar');

  ngAfterViewInit() {
    this.draw();
    void this.boot();
  }

  /** Testable boot flow: run the updater (with a cosmetic floor), and unless an
   *  update is installing, route into the app. A safety timeout guarantees we
   *  always move on. */
  async boot(): Promise<void> {
    const outcome = await Promise.race<UpdateOutcome>([
      this.runWithFloor(),
      this.delay(this.safetyMs).then(() => 'no-update' as const),
    ]);
    if (outcome !== 'updating') {
      await this.router.navigateByUrl('/app');
    }
  }

  private async runWithFloor(): Promise<UpdateOutcome> {
    const [outcome] = await Promise.all([this.updater.run(), this.delay(this.minMs)]);
    return outcome;
  }

  private delay(ms: number): Promise<void> {
    return new Promise((r) => setTimeout(r, ms));
  }

  /** Ported epicycle draw from the old index.html splash; drives the bar 0→100%
   *  unless the updater takes it over via the status/progress signals. */
  private draw(): void {
    const svg = this.svg()?.nativeElement;
    const bar = this.bar()?.nativeElement;
    if (!svg || !bar) return;
    const A = 22, B = 13, K = -4, TAU = Math.PI * 2;
    const epi = (t: number): [number, number] => [
      50 + A * Math.cos(t) + B * Math.cos(K * t),
      50 + A * Math.sin(t) + B * Math.sin(K * t),
    ];
    const N = 260;
    let d = '';
    for (let i = 0; i <= N; i++) {
      const t = (i / N) * TAU, p = epi(t);
      d += (i ? 'L' : 'M') + p[0].toFixed(2) + ' ' + p[1].toFixed(2) + ' ';
    }
    d += 'Z';
    svg.innerHTML =
      '<defs>' +
        '<linearGradient id="ob-g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff5d9e"/><stop offset=".5" stop-color="#a855f7"/><stop offset="1" stop-color="#22d3ee"/></linearGradient>' +
      '</defs>' +
      '<circle cx="50" cy="50" r="22" fill="none" stroke="#6b7488" stroke-width=".6" stroke-opacity=".3"/>' +
      '<path id="ob-path" d="' + d + '" fill="none" stroke="url(#ob-g)" stroke-width="2.4" stroke-linejoin="round" stroke-linecap="round"/>' +
      '<circle cx="50" cy="50" r="8.5" fill="#a855f7" fill-opacity=".25"/>';
    const path = svg.querySelector('#ob-path') as SVGPathElement;
    const reduce = window.matchMedia?.('(prefers-reduced-motion:reduce)').matches;
    const L = path.getTotalLength();
    path.style.strokeDasharray = String(L);
    if (reduce) {
      path.style.strokeDashoffset = '0';
      bar.style.width = '100%';
      return;
    }
    path.style.strokeDashoffset = String(L);
    const DUR = 2200;
    const ease = (x: number) => 1 - Math.pow(1 - x, 3);
    let start: number | null = null;
    const frame = (now: number) => {
      if (start === null) start = now;
      const p = Math.min(1, (now - start) / DUR), e = ease(p);
      path.style.strokeDashoffset = String(L * (1 - e));
      // Once the updater is downloading, its progress signal owns the bar.
      const dl = this.updater.progress();
      bar.style.width = (dl > 0 ? dl * 100 : e * 100).toFixed(1) + '%';
      if (p < 1) requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- loading.component`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/app/loading/loading.component.ts src/app/loading/loading.component.spec.ts
git commit -m "feat(loading): routed loading screen hosting the updater flow"
```

---

## Task 5: Wire routing, root component, providers, index.html

**Files:**
- Modify: `src/app/app.routes.ts`
- Modify: `src/app/app.component.ts`
- Modify: `src/app/app.config.ts`
- Modify: `src/index.html`

- [ ] **Step 1: Define routes (loading first)**

Replace the contents of `src/app/app.routes.ts`:

```ts
import { Routes } from '@angular/router';
import { LoadingComponent } from './loading/loading.component';
import { ShellComponent } from './shell/shell.component';

export const routes: Routes = [
  { path: '', component: LoadingComponent },
  { path: 'app', component: ShellComponent },
  { path: '**', redirectTo: '' },
];
```

- [ ] **Step 2: Reduce AppComponent to a router outlet**

Replace the contents of `src/app/app.component.ts`:

```ts
import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RouterOutlet } from '@angular/router';

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterOutlet],
  template: `<router-outlet />`,
})
export class AppComponent {}
```

- [ ] **Step 3: Provide the real updater**

In `src/app/app.config.ts`, add the import and provider. Add near the other imports:

```ts
import { UPDATER } from './updater/updater';
import { TauriUpdater } from './updater/tauri-updater';
```

Add to the `providers` array (after the `BRIDGE` provider):

```ts
    { provide: UPDATER, useFactory: () => new TauriUpdater() },
```

- [ ] **Step 4: Slim index.html and set the first-paint background**

In `src/index.html`: delete the entire `<style id="orrery-boot-style">…</style>` block, the `<div id="orrery-boot">…</div>` markup, and the `<script>(function () { … })();</script>` splash script. Replace the `<style id="orrery-boot-style">` block with a minimal first-paint background so there is no white flash before Angular paints:

```html
    <style>
      /* First-paint background — matches the dark theme --bg so the window is
         never white before Angular renders the loading screen. */
      html, body { background: #090a0f; margin: 0; }
    </style>
```

The `<body>` should contain only:

```html
  <body>
    <app-root></app-root>
  </body>
```

- [ ] **Step 5: Run the test suite and build**

Run: `pnpm test`
Expected: PASS (existing + new specs).

Run: `pnpm build`
Expected: PASS — `TauriUpdater`, `LoadingComponent`, `ShellComponent` all resolve.

- [ ] **Step 6: Commit**

```bash
git add src/app/app.routes.ts src/app/app.component.ts src/app/app.config.ts src/index.html
git commit -m "feat(boot): route through LoadingComponent; app-root is a router outlet"
```

---

## Task 6: Generate the signing keypair (manual) + set secrets

This produces the keypair the build signs with and the public key baked into config (Task 7). Run on your machine.

- [ ] **Step 1: Generate the keypair**

Run (PowerShell):

```powershell
pnpm tauri signer generate -w "$env:USERPROFILE\.orrery\signing.key"
```

When prompted, set a password (or pass `--password "<pw>"`). The command prints a **public key** (base64) and writes the **private key** to the file. Copy both somewhere temporary.

- [ ] **Step 2: Store CI secrets in `orrery`**

Run (PowerShell), substituting the password:

```powershell
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo kouji-dev/orrery --body (Get-Content -Raw "$env:USERPROFILE\.orrery\signing.key")
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo kouji-dev/orrery --body "<the password you chose>"
```

Expected: `gh` confirms `✓ Set secret …`.

- [ ] **Step 3: Record the public key**

Save the printed public key — Task 7 pastes it into `tauri.conf.json`. Do NOT commit the private key file. No git commit in this task.

---

## Task 7: Updater backend wiring (Cargo, plugins, config, capabilities)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Add the plugin crates**

In `src-tauri/Cargo.toml`, under `[dependencies]` (after `tauri-plugin-notification`):

```toml
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```

- [ ] **Step 2: Register the plugins**

In `src-tauri/src/lib.rs`, in the `.plugin(...)` chain (after `.plugin(tauri_plugin_notification::init())`):

```rust
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
```

- [ ] **Step 3: Configure the updater and updater artifacts**

In `src-tauri/tauri.conf.json`, set `bundle.createUpdaterArtifacts` to `true` (add the key inside the existing `"bundle"` object):

```json
  "bundle": {
    "active": true,
    "createUpdaterArtifacts": true,
    "targets": "all",
```

Add a top-level `"plugins"` key (sibling of `"bundle"`), pasting the public key from Task 6:

```json
  "plugins": {
    "updater": {
      "endpoints": [
        "https://github.com/kouji-dev/orrery-releases/releases/latest/download/latest.json"
      ],
      "pubkey": "<PASTE PUBLIC KEY FROM TASK 6>"
    }
  }
```

- [ ] **Step 4: Grant updater + process permissions**

In `src-tauri/capabilities/default.json`, add to the `"permissions"` array:

```json
    "updater:default",
    "process:allow-restart"
```

- [ ] **Step 5: Verify the Rust side compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS (downloads the two crates, compiles clean).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat(tauri): wire updater + process plugins, updater config, permissions"
```

---

## Task 8: Version-stamp script (TDD)

**Files:**
- Modify: `vitest.config.ts`
- Create: `scripts/release/stamp-version.mjs`
- Test: `scripts/release/stamp-version.spec.ts`

- [ ] **Step 1: Let vitest see the scripts**

In `vitest.config.ts`, change the `include` line to:

```ts
    include: ['src/**/*.spec.ts', 'scripts/**/*.spec.ts'],
```

- [ ] **Step 2: Write the failing test**

Create `scripts/release/stamp-version.spec.ts`:

```ts
import { describe, expect, it } from 'vitest';
// @ts-expect-error — plain ESM build script, no type declarations
import { setJsonVersion, setCargoVersion } from './stamp-version.mjs';

describe('setJsonVersion', () => {
  it('replaces only the first top-level version field', () => {
    const out = setJsonVersion('{\n  "name": "orrery",\n  "version": "0.1.0",\n  "deps": { "version": "9" }\n}', '0.2.0');
    expect(out).toContain('"version": "0.2.0"');
    expect(out).toContain('{ "version": "9" }'); // untouched
  });
});

describe('setCargoVersion', () => {
  it('replaces the package version, not dependency versions', () => {
    const cargo = '[package]\nname = "orrery"\nversion = "0.1.0"\n\n[dependencies]\ntauri = { version = "2" }\n';
    const out = setCargoVersion(cargo, '0.2.0');
    expect(out).toContain('version = "0.2.0"');
    expect(out).toContain('tauri = { version = "2" }'); // untouched
  });
});
```

- [ ] **Step 3: Run test to verify it fails**

Run: `pnpm test -- stamp-version`
Expected: FAIL — cannot resolve `./stamp-version.mjs`.

- [ ] **Step 4: Implement the script**

Create `scripts/release/stamp-version.mjs`:

```js
import { readFileSync, writeFileSync } from 'node:fs';

/** Replace the FIRST top-level "version": "x" in a JSON string (regex, so file
 *  formatting/key order is preserved). */
export function setJsonVersion(content, version) {
  return content.replace(/"version":\s*"[^"]*"/, `"version": "${version}"`);
}

/** Replace the [package] `version = "x"` line in Cargo.toml. Anchored to line
 *  start so inline dependency `version = "2"` entries are not matched. */
export function setCargoVersion(content, version) {
  return content.replace(/^version = "[^"]*"/m, `version = "${version}"`);
}

// CLI: node scripts/release/stamp-version.mjs <version>
if (import.meta.url === `file://${process.argv[1]}`) {
  const version = process.argv[2];
  if (!version) {
    console.error('usage: stamp-version.mjs <version>');
    process.exit(1);
  }
  for (const f of ['package.json', 'src-tauri/tauri.conf.json']) {
    writeFileSync(f, setJsonVersion(readFileSync(f, 'utf8'), version));
  }
  writeFileSync('src-tauri/Cargo.toml', setCargoVersion(readFileSync('src-tauri/Cargo.toml', 'utf8'), version));
  console.log(`stamped version ${version}`);
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm test -- stamp-version`
Expected: PASS (2 tests).

- [ ] **Step 6: Commit**

```bash
git add vitest.config.ts scripts/release/stamp-version.mjs scripts/release/stamp-version.spec.ts
git commit -m "feat(release): build-time version stamping script"
```

---

## Task 9: latest.json builder (TDD)

**Files:**
- Create: `scripts/release/make-latest-json.mjs`
- Test: `scripts/release/make-latest-json.spec.ts`

- [ ] **Step 1: Write the failing test**

Create `scripts/release/make-latest-json.spec.ts`:

```ts
import { describe, expect, it } from 'vitest';
// @ts-expect-error — plain ESM build script, no type declarations
import { buildLatestJson, assetUrl } from './make-latest-json.mjs';

describe('assetUrl', () => {
  it('builds a release download URL from repo, tag, and filename', () => {
    expect(assetUrl('kouji-dev/orrery-releases', 'v0.2.0', 'Orrery_0.2.0_x64-setup.exe')).toBe(
      'https://github.com/kouji-dev/orrery-releases/releases/download/v0.2.0/Orrery_0.2.0_x64-setup.exe',
    );
  });
});

describe('buildLatestJson', () => {
  it('shapes the Tauri v2 updater manifest for windows-x86_64', () => {
    const manifest = buildLatestJson({
      version: '0.2.0',
      notes: 'hi',
      pubDate: '2026-06-08T00:00:00.000Z',
      signature: 'SIG==',
      url: 'https://example/Orrery_0.2.0_x64-setup.exe',
    });
    expect(manifest).toEqual({
      version: '0.2.0',
      notes: 'hi',
      pub_date: '2026-06-08T00:00:00.000Z',
      platforms: {
        'windows-x86_64': { signature: 'SIG==', url: 'https://example/Orrery_0.2.0_x64-setup.exe' },
      },
    });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm test -- make-latest-json`
Expected: FAIL — cannot resolve `./make-latest-json.mjs`.

- [ ] **Step 3: Implement the script**

Create `scripts/release/make-latest-json.mjs`:

```js
import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

export function assetUrl(repo, tag, filename) {
  return `https://github.com/${repo}/releases/download/${tag}/${filename}`;
}

export function buildLatestJson({ version, notes, pubDate, signature, url }) {
  return {
    version,
    notes,
    pub_date: pubDate,
    platforms: {
      'windows-x86_64': { signature, url },
    },
  };
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) out[argv[i].replace(/^--/, '')] = argv[i + 1];
  return out;
}

// CLI: node make-latest-json.mjs --version X --dir DIR --out FILE --repo O/R [--notes "..."]
if (import.meta.url === `file://${process.argv[1]}`) {
  const a = parseArgs(process.argv.slice(2));
  const tag = `v${a.version}`;
  const setup = readdirSync(a.dir).find((f) => f.endsWith('-setup.exe'));
  if (!setup) {
    console.error(`no *-setup.exe in ${a.dir}`);
    process.exit(1);
  }
  const signature = readFileSync(join(a.dir, `${setup}.sig`), 'utf8').trim();
  const manifest = buildLatestJson({
    version: a.version,
    notes: a.notes || `Orrery v${a.version}`,
    pubDate: new Date().toISOString(),
    signature,
    url: assetUrl(a.repo, tag, setup),
  });
  writeFileSync(a.out, JSON.stringify(manifest, null, 2));
  console.log(`wrote ${a.out} for ${setup}`);
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm test -- make-latest-json`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add scripts/release/make-latest-json.mjs scripts/release/make-latest-json.spec.ts
git commit -m "feat(release): cross-repo latest.json builder"
```

---

## Task 10: Release workflow

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  workflow_dispatch:
    inputs:
      version:
        description: "Release version, e.g. 0.2.0 (no leading v)"
        required: true
        type: string
      notes:
        description: "Release notes (optional)"
        required: false
        type: string

permissions:
  contents: read

jobs:
  build:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
        with:
          version: 9
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
      - uses: dtolnay/rust-toolchain@stable
      - uses: swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
      - run: pnpm install --frozen-lockfile
      - name: Stamp version (build-only, not committed)
        run: node scripts/release/stamp-version.mjs ${{ inputs.version }}
      - name: Build Tauri app with signed updater artifacts
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        run: pnpm tauri build
      - name: Stage release files (flat)
        shell: pwsh
        run: |
          New-Item -ItemType Directory -Force dist-release | Out-Null
          Copy-Item src-tauri/target/release/bundle/nsis/*-setup.exe dist-release/
          Copy-Item src-tauri/target/release/bundle/nsis/*-setup.exe.sig dist-release/
          Copy-Item src-tauri/target/release/bundle/msi/*.msi dist-release/
      - uses: actions/upload-artifact@v4
        with:
          name: release-files
          path: dist-release/

  publish:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
      - uses: actions/download-artifact@v4
        with:
          name: release-files
          path: artifacts
      - name: Build latest.json
        run: >
          node scripts/release/make-latest-json.mjs
          --version ${{ inputs.version }}
          --dir artifacts
          --out artifacts/latest.json
          --repo kouji-dev/orrery-releases
          --notes "${{ inputs.notes }}"
      - name: Create release in orrery-releases
        env:
          GH_TOKEN: ${{ secrets.RELEASES_TOKEN }}
        run: >
          gh release create "v${{ inputs.version }}"
          --repo kouji-dev/orrery-releases
          --target main
          --title "v${{ inputs.version }}"
          --notes "${{ inputs.notes != '' && inputs.notes || format('Orrery v{0}', inputs.version) }}"
          artifacts/*-setup.exe artifacts/*.msi artifacts/latest.json
```

- [ ] **Step 2: Lint the YAML**

Run: `pnpm dlx @action-validator/cli@latest .github/workflows/release.yml` (or eyeball it — must be valid YAML, two jobs, `publish` `needs: build`).
Expected: no syntax errors.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: manual Windows release workflow publishing to orrery-releases"
```

---

## Task 11: Manual prerequisites checklist

No code. Complete these before the first run (Task 12). Tick each off.

- [ ] **`orrery-releases` is public and has a `main` branch.** If empty, initialize it:

```powershell
gh repo edit kouji-dev/orrery-releases --visibility public
# If it has no commits yet, add a README so `main` exists and --target main resolves:
gh api repos/kouji-dev/orrery-releases/contents/README.md -X PUT -f message="init" -f content=$([Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes("# Orrery Releases`n")))
```

- [ ] **Fine-grained PAT created** at GitHub → Settings → Developer settings → Fine-grained tokens: *Resource owner* `kouji-dev`, *Only select repositories* → `orrery-releases`, *Repository permissions* → **Contents: Read and write**. Copy the token.

- [ ] **PAT stored as a secret in `orrery`:**

```powershell
gh secret set RELEASES_TOKEN --repo kouji-dev/orrery --body "<paste fine-grained PAT>"
```

- [ ] **Signing secrets present** (from Task 6): confirm with `gh secret list --repo kouji-dev/orrery` — expect `RELEASES_TOKEN`, `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

---

## Task 12: End-to-end verification (per global "E2E after every feature" rule)

The release workflow's only real test is a live run. The updater's real test is upgrading across two releases. Per the spec, the version is **typed at dispatch and stamped at build time** — you do NOT commit version bumps; the repo's committed `0.1.0` stays as-is.

- [ ] **Step 1: Merge the branch to `main`** (workflow_dispatch only sees workflows on the default branch). Push `feat/release-pipeline` and merge via PR.

- [ ] **Step 2: Dispatch the first release.** Any version works for a fresh install; use `0.2.0`.

```powershell
gh workflow run release.yml --repo kouji-dev/orrery -f version=0.2.0 -f notes="First automated release"
gh run watch --repo kouji-dev/orrery
```

Expected: both jobs green.

- [ ] **Step 3: Verify the release exists with all assets.**

```powershell
gh release view v0.2.0 --repo kouji-dev/orrery-releases
```

Expected assets: `*-setup.exe`, `*.msi`, `latest.json`. Confirm `latest.json` opens at
`https://github.com/kouji-dev/orrery-releases/releases/latest/download/latest.json`
and its `platforms.windows-x86_64.url` points at the `-setup.exe`.

- [ ] **Step 4: Install + verify no-update path.** Download and install `0.2.0`. Launch it: the loading screen shows, no update is found (it is the latest), it routes into the app normally.

- [ ] **Step 5: Verify the update path.** Dispatch again with `version=0.3.0` (just type it — no commit needed; CI stamps it). With `0.2.0` still installed, launch it: the loading screen should switch to `downloading update · 0.3.0`, drive the progress bar, then relaunch into `0.3.0`.

- [ ] **Step 6: Done.** If all steps pass, the pipeline + updater are verified end-to-end.

---

## Notes & accepted limitations

- **Unsigned builds:** first install shows a Windows SmartScreen warning. The minisign updater is unaffected. OS code-signing is deferred.
- **Re-running the same version fails** at `gh release create` (tag exists). Bump the version per release, or `gh release delete vX.Y.Z --repo kouji-dev/orrery-releases --cleanup-tag --yes` first.
- **macOS/Linux** are out of scope; the `build` job is structured so a future `strategy.matrix` can add them, with extra `platforms.*` entries in `make-latest-json.mjs`.
- **Build uses `pnpm tauri build` directly** rather than `tauri-apps/tauri-action` (which the spec mentioned). The direct call gives deterministic bundle paths for the flat-staging step, and the signing env (`TAURI_SIGNING_PRIVATE_KEY*`) still produces the `.sig` files. tauri-action's auto-`latest.json` is same-repo only, so we hand-write it regardless.
