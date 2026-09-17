import { readFileSync, writeFileSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { resolve, dirname } from "node:path";
import {
  parseConventionalCommit,
  isNoiseCommit,
  deriveSummary,
  buildReleaseEntry,
  formatDate,
} from "../landing/gen-changelog.mjs";

// Maintains changelog.json — the single source of truth for the release history,
// committed to the PUBLIC orrery-releases repo (orrery is private). Consumed by
// BOTH the landing changelog page and the Orrery app's in-app "What's new" digest
// (both fetch the raw URL). Run against an orrery-releases checkout via --out.
//
//   migrate            — bootstrap: build the full history from every v* git tag
//                        (self-contained on local git; re-runnable).
//   append --tag vX.Y.Z [--summary S] [--channel C]
//                      — per-release: build the entry for `tag` from the commits
//                        since the previous tag (local git) and prepend it.
//
// The data shaping is pure + unit-tested; main() does the git/file IO.

const HERE = dirname(fileURLToPath(import.meta.url));
const DEFAULT_OUT = resolve(HERE, "../../changelog.json");
const UNIT_SEP = String.fromCharCode(31); // ASCII US — separates sha from subject

// ── pure helpers (unit-tested) ───────────────────────────────────────────────

/** Build a release entry from raw `{ hash, subject }` commit pairs (display
 *  order is whatever is passed in — newest-first by convention). */
export function entryFromCommits({ tag, channel, date, ref, commits, summary }) {
  const shaped = commits
    .filter((c) => c && c.subject && c.subject.trim() && !isNoiseCommit(c.subject))
    .map((c) => {
      const p = parseConventionalCommit(c.subject);
      return { type: p.type, hash: c.hash, scope: p.scope, msg: p.msg };
    });
  return buildReleaseEntry({
    tag,
    channel,
    date,
    ref,
    summary: summary || deriveSummary(shaped, tag),
    commits: shaped,
  });
}

/** Prepend `entry`, replacing any existing entry for the same tag (idempotent
 *  re-runs). Newest-first is preserved. */
export function upsertRelease(releases, entry) {
  return [entry, ...releases.filter((r) => r.tag !== entry.tag)];
}

// ── git IO ───────────────────────────────────────────────────────────────────

function git(...args) {
  return execFileSync("git", args).toString();
}

/** The tag immediately preceding `tag` in semver order (null if it's the first). */
function prevTagOf(tag) {
  const tags = git("tag", "--list", "v*", "--sort=v:refname")
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
  const i = tags.indexOf(tag);
  return i > 0 ? tags[i - 1] : null;
}

/** `{ hash, subject }` for each commit in `range`, newest-first. */
function commitsInRange(range) {
  const out = git("log", "--no-merges", "--pretty=%H%x1f%s", range).trim();
  if (!out) return [];
  return out.split("\n").map((line) => {
    const idx = line.indexOf(UNIT_SEP);
    return { hash: line.slice(0, idx), subject: line.slice(idx + 1) };
  });
}

// ── CLI ──────────────────────────────────────────────────────────────────────

function parseArgs(argv) {
  const out = { mode: argv[0], channel: "beta", out: DEFAULT_OUT };
  for (let i = 1; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--tag") out.tag = argv[++i];
    else if (a === "--summary") out.summary = argv[++i];
    else if (a === "--channel") out.channel = argv[++i];
    else if (a === "--out") out.out = resolve(process.cwd(), argv[++i]);
  }
  return out;
}

function readJson(path) {
  if (!existsSync(path)) return [];
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return [];
  }
}

function main() {
  const args = parseArgs(process.argv.slice(2));

  if (args.mode === "migrate") {
    // Build the full history from every v* tag (newest first) — self-contained
    // on local git, so it can bootstrap orrery-releases/changelog.json from
    // scratch. Re-runnable; per-release `append` maintains it afterward.
    const tags = git("tag", "--list", "v*", "--sort=-v:refname")
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
    const releases = tags.map((tag) => {
      const prev = prevTagOf(tag);
      const range = prev ? `${prev}..${tag}` : tag;
      const commits = commitsInRange(range);
      const headSha = commits[0]?.hash ?? git("rev-list", "-n", "1", tag).trim();
      const isoDate = git("log", "-1", "--pretty=%cI", headSha).trim();
      return entryFromCommits({ tag, channel: args.channel, date: formatDate(isoDate), ref: headSha, commits });
    });
    writeFileSync(args.out, JSON.stringify(releases, null, 2) + "\n");
    console.log(`wrote ${args.out}: ${releases.length} releases (migrated from git tags).`);
    return;
  }

  if (args.mode === "append") {
    const tag =
      args.tag ||
      git("tag", "--list", "v*", "--sort=-v:refname")
        .split("\n")
        .map((s) => s.trim())
        .find(Boolean);
    if (!tag) throw new Error("no v* tag found and none given via --tag");
    const prev = prevTagOf(tag);
    const range = prev ? `${prev}..${tag}` : tag;
    const commits = commitsInRange(range);
    if (!commits.length) throw new Error(`no commits found for ${tag} (${range})`);
    const headSha = commits[0].hash;
    const isoDate = git("log", "-1", "--pretty=%cI", headSha).trim();
    const entry = entryFromCommits({
      tag,
      channel: args.channel,
      date: formatDate(isoDate),
      ref: headSha,
      commits,
      summary: args.summary,
    });
    const next = upsertRelease(readJson(args.out), entry);
    writeFileSync(args.out, JSON.stringify(next, null, 2) + "\n");
    console.log(`updated ${args.out} for ${tag} (${entry.commits.length} commits).`);
    return;
  }

  throw new Error(`unknown mode "${args.mode}". usage: changelog-json.mjs migrate | append [--tag vX.Y.Z]`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    main();
  } catch (err) {
    console.error(err.message || err);
    process.exit(1);
  }
}
