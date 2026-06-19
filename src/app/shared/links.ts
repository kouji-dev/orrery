// External links the app opens in the user's browser (via the opener plugin).

/** GitHub releases page — the canonical full changelog / release history. */
export const RELEASES_URL = "https://github.com/kouji-dev/orrery-releases/releases";

/** Single-source release history, committed to the PUBLIC orrery-releases repo
 *  (orrery itself is private). Both the app and the landing changelog page read
 *  this one file via the raw URL (raw.githubusercontent.com sends CORS `*`). */
export const CHANGELOG_JSON_URL =
  "https://raw.githubusercontent.com/kouji-dev/orrery-releases/main/changelog.json";
