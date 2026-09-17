/**
 * Global cap on live CodeMirror instances (A0.6).
 *
 * Every mounted editor (unified-code, code-diff) registers here. When the cap
 * is exceeded, the OLDEST registered editor is "demoted": its owner destroys
 * the CodeMirror view and falls back to plain text, freeing the editor's DOM,
 * decorations and parse trees. Components re-render a real editor the next
 * time their inputs change (tab revisit re-mounts via @if/effect), so a demote
 * is never data loss — only a display downgrade of an off-screen-ish pane.
 */

/** Why 6: above any realistic split count, bounds webview heap. */
export const MAX_LIVE_EDITORS = 6;

interface LiveEditor {
  demote: () => void;
}

const live: LiveEditor[] = [];

/**
 * Register a freshly mounted editor. `demote` must destroy the owner's view
 * and render a plain-text fallback; it is called at most once, only when the
 * cap is exceeded. Returns an unregister function — call it whenever the view
 * is destroyed through the normal path (re-render, ngOnDestroy).
 */
export function registerEditor(demote: () => void): () => void {
  const entry: LiveEditor = { demote };
  live.push(entry);
  while (live.length > MAX_LIVE_EDITORS) {
    const oldest = live.shift();
    oldest?.demote();
  }
  return () => {
    const i = live.indexOf(entry);
    if (i >= 0) live.splice(i, 1);
  };
}

/** Test hook: number of currently registered live editors. */
export function liveEditorCount(): number {
  return live.length;
}

/** Test hook: drop all registrations (module state is process-global). */
export function resetEditorCap(): void {
  live.length = 0;
}
