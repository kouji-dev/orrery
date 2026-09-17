import { describe, it, expect } from 'vitest';
import {
  parseConventionalCommit,
  isNoiseCommit,
  formatDate,
  shortHash,
  deriveSummary,
  buildReleaseEntry,
  readReleases,
  injectRelease,
  START_LINE,
  END_LINE,
} from './gen-changelog.mjs';

describe('parseConventionalCommit', () => {
  it('parses type, scope and message', () => {
    expect(parseConventionalCommit('feat(workspace): add tabs'))
      .toEqual({ type: 'feat', scope: 'workspace', msg: 'add tabs' });
  });
  it('handles a missing scope', () => {
    expect(parseConventionalCommit('fix: crash on boot'))
      .toEqual({ type: 'fix', scope: '', msg: 'crash on boot' });
  });
  it('handles a breaking-change bang', () => {
    expect(parseConventionalCommit('feat(api)!: drop v1'))
      .toEqual({ type: 'feat', scope: 'api', msg: 'drop v1' });
  });
  it('falls back to chore for non-conventional messages', () => {
    expect(parseConventionalCommit('random message'))
      .toEqual({ type: 'chore', scope: '', msg: 'random message' });
  });
});

describe('isNoiseCommit', () => {
  it('flags release bumps', () => expect(isNoiseCommit('release: v0.2.6')).toBe(true));
  it('flags merge commits', () => expect(isNoiseCommit('Merge pull request #3')).toBe(true));
  it('keeps normal commits', () => expect(isNoiseCommit('feat: thing')).toBe(false));
});

describe('formatDate', () => {
  it('formats an ISO date to "Month D, YYYY" (UTC)', () => {
    expect(formatDate('2026-06-13T09:15:00Z')).toBe('June 13, 2026');
  });
});

describe('shortHash', () => {
  it('takes the first 7 chars', () => expect(shortHash('abcdef1234567')).toBe('abcdef1'));
});

describe('deriveSummary', () => {
  it('prefers the newest feat commit message', () => {
    const commits = [
      { type: 'fix', msg: 'a crash' },
      { type: 'feat', msg: 'add density modes' },
      { type: 'feat', msg: 'older feature' },
    ];
    expect(deriveSummary(commits, 'v0.3.2')).toBe('add density modes');
  });
  it('falls back to the newest commit when there is no feat', () => {
    expect(deriveSummary([{ type: 'fix', msg: 'patch a bug' }], 'v0.3.2'))
      .toBe('patch a bug');
  });
  it('falls back to the tag when there are no commits', () => {
    expect(deriveSummary([], 'v0.3.2')).toBe('Orrery v0.3.2');
  });
});

describe('buildReleaseEntry', () => {
  it('shapes an entry and shortens hashes', () => {
    expect(buildReleaseEntry({
      tag: 'v0.2.6', channel: 'dev', date: 'June 13, 2026',
      ref: '1234567890abcdef', summary: 'hi',
      commits: [{ type: 'feat', hash: 'abcdef1234', scope: 'ui', msg: 'x' }],
    })).toEqual({
      tag: 'v0.2.6', channel: 'dev', date: 'June 13, 2026', ref: '1234567',
      summary: 'hi',
      commits: [{ type: 'feat', hash: 'abcdef1', scope: 'ui', msg: 'x' }],
    });
  });
});

const SAMPLE = [
  START_LINE,
  '  const RELEASES = [',
  '    { "tag": "v0.2.5", "channel": "dev", "date": "June 13, 2026", "ref": "2a626c4", "summary": "old", "commits": [] }',
  '  ];',
  END_LINE,
].join('\n');

describe('readReleases / injectRelease', () => {
  it('reads existing entries', () => {
    expect(readReleases(SAMPLE).map((r) => r.tag)).toEqual(['v0.2.5']);
  });
  it('prepends a new entry (newest first)', () => {
    const out = injectRelease(SAMPLE, buildReleaseEntry({
      tag: 'v0.2.6', channel: 'dev', date: 'June 20, 2026', ref: 'aaaaaaa', summary: 'new', commits: [],
    }));
    expect(readReleases(out).map((r) => r.tag)).toEqual(['v0.2.6', 'v0.2.5']);
  });
  it('is idempotent for the same tag (replaces, never duplicates)', () => {
    const out = injectRelease(SAMPLE, buildReleaseEntry({
      tag: 'v0.2.5', channel: 'dev', date: 'June 13, 2026', ref: '2a626c4', summary: 'updated', commits: [],
    }));
    const rel = readReleases(out);
    expect(rel.map((r) => r.tag)).toEqual(['v0.2.5']);
    expect(rel[0].summary).toBe('updated');
  });
  it('preserves both markers', () => {
    const out = injectRelease(SAMPLE, buildReleaseEntry({
      tag: 'v0.3.0', channel: 'dev', date: 'July 1, 2026', ref: 'bbbbbbb', summary: 's', commits: [],
    }));
    expect(out).toContain(START_LINE);
    expect(out).toContain(END_LINE);
  });
});
