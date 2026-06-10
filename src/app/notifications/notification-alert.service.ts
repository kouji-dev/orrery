import { inject, Injectable } from "@angular/core";
import { AgentNotification, NotificationKind, SettingsEvents } from "../models";
import { SettingsStore } from "../settings/settings.store";
import { NotificationStore } from "../stores/notifications.store";
import { UiStore } from "../ui/ui.store";
import { playNotificationSound, primeNotificationAudioOnGesture } from "./notification-sound";

type NotificationPlugin = typeof import("@tauri-apps/plugin-notification");

/** Which settings `events` toggle gates each notification kind. NOTE: nothing
 *  raises an "error" notification today (NotificationKind has no error member),
 *  so `events.error` has no raise path to gate yet — wire it here the day one
 *  exists. */
const KIND_EVENT: Record<NotificationKind, keyof SettingsEvents> = {
  done: "finished",
  question: "question",
  permission: "permission",
};

export type RaiseInput = Parameters<NotificationStore["push"]>[0];

/**
 * The single gate every agent notification passes through on its way to the
 * user. Applies the notification settings:
 *
 *  • per-event toggles gate raising ENTIRELY — off means not even the in-app
 *    feed records it (for hook-driven tools, turning `permission` off therefore
 *    also clears the needs-input indicator, which is derived from the pending
 *    permission notification);
 *  • `osNotifications` master: ON also fires a native OS toast and the sound
 *    cue; OFF = in-app feed only;
 *  • focus: a focused window skips the toast (the in-app feed is already in
 *    view) — EXCEPT permission prompts while `remoteApproval` is on, which
 *    toast regardless of focus;
 *  • `sound`/`soundName`/`volume`: a WebAudio cue per raised notification.
 *
 * OS permission is requested lazily on the first toast and cached for the run.
 */
@Injectable({ providedIn: "root" })
export class NotificationAlertService {
  private readonly settings = inject(SettingsStore);
  private readonly store = inject(NotificationStore);
  private readonly ui = inject(UiStore);

  /** Lazy one-time plugin import (shared by concurrent toasts). */
  private plugin: Promise<NotificationPlugin> | null = null;
  /** Lazy one-time OS-notification permission request (cached per app run). */
  private permission: Promise<boolean> | null = null;
  private actionsWired = false;

  constructor() {
    // WebView2 autoplay policy: an off-gesture AudioContext stays suspended and
    // the first cue would be silent — arm it on the user's first interaction.
    primeNotificationAudioOnGesture();
  }

  /** Gate + raise. Returns the pushed notification, or null when the event is
   *  toggled off or the store de-duped it (same agent+kind still pending) — in
   *  both cases NOTHING fires (no toast, no sound). */
  raise(input: RaiseInput): AgentNotification | null {
    const s = this.settings.settings();
    if (!s.events[KIND_EVENT[input.kind]]) return null; // event off → not raised at all
    const note = this.store.push(input);
    if (!note) return null; // de-duped — don't re-toast / re-cue
    if (s.osNotifications) {
      if (s.sound) playNotificationSound(s.soundName, s.volume);
      void this.toast(note);
    }
    return note;
  }

  /** Settings-dialog "play" button: simulate a real alert with the CURRENT
   *  settings — cue at the set volume plus a native toast. Skips the in-app
   *  feed (no fake pending entries) and the focused-window suppression (the
   *  user is by definition focused while clicking; they want to SEE it). */
  preview(): void {
    const s = this.settings.settings();
    if (!s.osNotifications) return; // master off = in-app only; nothing to demo
    if (s.sound) playNotificationSound(s.soundName, s.volume);
    void this.toast(
      {
        agentId: "",
        title: "Test notification",
        detail: "This is how an agent alert will look (and sound).",
        kind: "done",
      } as unknown as AgentNotification,
      { bypassFocus: true },
    );
  }

  private async toast(note: AgentNotification, opts?: { bypassFocus?: boolean }): Promise<void> {
    const s = this.settings.settings();
    const focused = typeof document !== "undefined" && document.hasFocus();
    if (!opts?.bypassFocus && focused && !(note.kind === "permission" && s.remoteApproval)) return;
    try {
      const m = await this.loadPlugin();
      if (!(await this.ensurePermission(m))) return;
      void this.wireToastClick(m);
      m.sendNotification({
        title: note.title,
        body: note.detail || "tap to open",
        extra: { agentId: note.agentId },
      });
    } catch {
      // not under Tauri / plugin unavailable — the in-app feed still has it
    }
  }

  private loadPlugin(): Promise<NotificationPlugin> {
    this.plugin ??= import("@tauri-apps/plugin-notification");
    return this.plugin;
  }

  private ensurePermission(m: NotificationPlugin): Promise<boolean> {
    this.permission ??= (async () => {
      let granted = await m.isPermissionGranted();
      if (!granted) granted = (await m.requestPermission()) === "granted";
      return granted;
    })();
    return this.permission;
  }

  /** Toast interaction → focus the window + open the agent's terminal.
   *  HONEST LIMITATION: the v2 notification plugin only DELIVERS `onAction` on
   *  mobile today — on Windows desktop clicking a toast just activates the app
   *  (the OS default) and this listener stays silent. It is registered
   *  best-effort so the deep-link lights up if the desktop plugin gains click
   *  events. */
  private async wireToastClick(m: NotificationPlugin): Promise<void> {
    if (this.actionsWired) return;
    this.actionsWired = true;
    try {
      await m.onAction((n) => {
        void this.focusWindow();
        const agentId = n.extra?.["agentId"];
        if (typeof agentId === "string" && agentId) this.ui.openAgent(agentId, "terminal");
      });
    } catch {
      this.actionsWired = false; // registration failed — retry with the next toast
    }
  }

  private async focusWindow(): Promise<void> {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      const win = getCurrentWindow();
      await win.unminimize();
      await win.setFocus();
    } catch {
      // not under Tauri
    }
  }
}
