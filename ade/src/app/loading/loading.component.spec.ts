import { ApplicationRef, Injector, provideZonelessChangeDetection, runInInjectionContext } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import { describe, expect, it, vi } from 'vitest';
import { UpdateOutcome } from '../updater/updater';
import { UpdaterService } from '../updater/updater.service';
import { WorkspaceStore } from '../stores/workspace.store';
import { LoadingComponent } from './loading.component';

function make(outcome: UpdateOutcome | 'pending') {
  const navigateByUrl = vi.fn(async () => true);
  const run = vi.fn(
    (): Promise<UpdateOutcome> =>
      outcome === 'pending' ? new Promise<UpdateOutcome>(() => {}) : Promise.resolve(outcome),
  );
  // The component registers afterNextRender() in its constructor, which needs
  // the render pipeline (AfterRenderManager) — a bare Injector.create cannot
  // provide it. TestBed's injector can, and still lets us construct the
  // component directly instead of rendering its canvas template under jsdom.
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: Router, useValue: { navigateByUrl } },
      { provide: UpdaterService, useValue: { run, status: () => '', progress: () => 0 } },
      { provide: WorkspaceStore, useValue: { ready: () => Promise.resolve() } },
    ],
  });
  const injector = TestBed.inject(Injector);
  TestBed.inject(ApplicationRef); // materialise the render pipeline
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
