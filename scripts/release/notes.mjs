import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

// Auto release notes: one `- subject` line per commit since the PREVIOUS
// release, merge commits skipped (--no-merges) and the `release: vX.Y.Z` bump
// commits filtered out. Shared by bump.mjs (tag message / preview) and the
// Release workflow (gh release body + latest.json notes).

/** Bullet list from raw subjects; drops release-bump subjects and blanks. */
export function buildNotes(subjects) {
  return subjects
    .filter((s) => s.trim() !== '' && !/^release: v\d/.test(s))
    .map((s) => `- ${s}`)
    .join('\n');
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const out = (...args) => execFileSync('git', args).toString().trim();
  const head = out('rev-parse', 'HEAD');
  // Previous release = latest v* tag that is NOT the current commit (on a
  // tag-triggered CI build HEAD itself carries the new tag), falling back to
  // the most recent `release: vX.Y.Z` bump commit. No boundary at all (very
  // first release) = whole history.
  const from =
    out('tag', '--list', 'v*', '--sort=-v:refname', '--merged', 'HEAD')
      .split('\n')
      .filter(Boolean)
      .find((t) => out('rev-list', '-n', '1', t) !== head) ??
    out('log', '--grep', '^release: v[0-9]', '--pretty=%H', 'HEAD')
      .split('\n')
      .filter(Boolean)
      .find((sha) => sha !== head);
  const range = from ? `${from}..HEAD` : 'HEAD';
  const subjects = out('log', '--no-merges', '--pretty=%s', range).split('\n');
  console.log(buildNotes(subjects));
}
