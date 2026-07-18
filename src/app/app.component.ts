import { ChangeDetectionStrategy, Component, effect, inject } from '@angular/core';
import { RouterOutlet } from '@angular/router';
import { BRIDGE, Commands } from './data-source/bridge';
import { UiStore } from './ui/ui.store';
import { FileDropService } from './shared/file-drop.service';

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterOutlet],
  template: `<router-outlet />`,
})
export class AppComponent {
  private readonly ui = inject(UiStore);
  private readonly bridge = inject(BRIDGE);
  private readonly fileDrop = inject(FileDropService);

  constructor() {
    // OS file/image drops → focused agent's terminal (prints the path).
    void this.fileDrop.init();

    // Mirror the app theme onto the live window/taskbar icon (Tauri set_icon via
    // the set_window_icon command). Runs on init and every theme toggle.
    // Best-effort: no-ops in a plain browser (no Tauri backend) — the rejected
    // invoke is swallowed.
    effect(() => {
      const dark = this.ui.tweaks().theme === 'dark';
      this.bridge.invoke(Commands.SetWindowIcon, { dark }).catch(() => {});
    });
  }
}
