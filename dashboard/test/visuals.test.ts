import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import type { RawComment } from '../src/server/raw.ts';
import { fromLinks, fromManifest, plainText, safePath, safeRef, visualPost } from '../src/server/visuals.ts';

const REPO = 'tariq-98/aldermoor-rs';
const fixture = (name: string) => readFileSync(new URL(`./fixtures/${name}`, import.meta.url), 'utf8');

function comment(body: string): RawComment {
  return {
    id: 7,
    html_url: `https://github.com/${REPO}/pull/54#issuecomment-7`,
    issue_url: `https://api.github.com/repos/${REPO}/issues/54`,
    body,
    created_at: '2026-10-24T10:00:00Z',
    updated_at: '2026-10-24T10:00:00Z',
  };
}

const onPr = (headSha: string | null) => ({ repo: REPO, number: 54, kind: 'pr' as const, headSha });

test('reads the comment attach_visuals.py writes', () => {
  const post = fromManifest(comment(fixture('attach-visuals.md')), onPr('5f5e4d7c0ffee'));
  assert.ok(post);
  assert.equal(post.title, 'Mac check -- limestone repeat', 'the escaped -- comes back');
  assert.equal(post.verdict, 'pass');
  assert.equal(post.head, '5f5e4d7');
  assert.equal(post.base, '7286aa4');
  assert.equal(post.by, 'thread: Run Mac tasks from home folder');
  assert.equal(post.current, true);
  assert.equal(post.legacy, false);
  assert.deepEqual(post.items.map((i) => [i.type, i.caption, i.src, i.thumb]), [
    ['image', 'East cliff, west face. Left main, right this branch', '/api/media?ref=DRY-RUN&path=pr54%2Feast-along.jpg',
      '/api/media?ref=DRY-RUN&path=pr54%2Feast-along.thumb.jpg'],
    ['video', 'Wolf fight with the new effects', '/api/media?ref=DRY-RUN&path=pr54%2Fclip.mp4',
      '/api/media?ref=DRY-RUN&path=pr54%2Fclip.thumb.jpg'],
  ]);
  assert.equal(post.items[0]!.github, `https://github.com/${REPO}/blob/DRY-RUN/pr54/east-along.jpg`);
});

test('says whether captures show the PR as it is now', () => {
  const body = fixture('attach-visuals.md');
  assert.equal(fromManifest(comment(body), onPr('0123456789abcdef'))?.current, false);
  assert.equal(fromManifest(comment(body), onPr(null))?.current, null, 'a closed PR has no head to compare');
});

test('drops items it must not serve and posts left with none', () => {
  const manifest = (items: unknown[]) =>
    `<!-- aldermoor-visuals ${JSON.stringify({ v: 1, title: 'x', ref: 'abc', items })} -->`;
  const post = fromManifest(comment(manifest([
    { path: '../secrets.png', caption: 'escape' },
    { path: '/etc/passwd.png', caption: 'absolute' },
    { path: 'pr1/notes.txt', caption: 'not a capture' },
    { path: 'pr1/ok.png', caption: 'fine', thumb: 'pr1/ok.mp4' },
  ])), onPr(null));
  assert.deepEqual(post?.items.map((i) => [i.caption, i.thumb]), [['fine', null]]);
  assert.equal(fromManifest(comment(manifest([{ path: '../x.png' }])), onPr(null)), null);
});

test('falls back to the links when the manifest is broken', () => {
  const body = `<!-- aldermoor-visuals {"v":1, nope} -->\n**Captures**\n\n![a](https://github.com/${REPO}/blob/captures/pr54/a.png?raw=true)`;
  const post = visualPost(comment(body), onPr(null));
  assert.equal(post?.legacy, true);
  assert.equal(post?.items.length, 1);
});

test('reads captions from older comments that only link images', () => {
  const post = fromLinks(comment(fixture('legacy-mac-check.md')), onPr('5f5e4d7abc'));
  assert.ok(post);
  assert.equal(post.title, 'Mac check: PASS (head 5f5e4d7) on the Apple M1, built from this branch.');
  assert.equal(post.verdict, 'pass');
  assert.equal(post.head, '5f5e4d7');
  assert.equal(post.current, true);
  assert.equal(post.legacy, true);
  assert.deepEqual(post.items.map((i) => i.caption), [
    'Main',
    'This branch',
    'Gorge, from the bridge',
    'Gorge, from the bank',
    'Wolf den at dusk',
    'The haze now reads right at 60 m (#48)',
  ], 'images in other repos are left out');
  assert.equal(post.items[5]!.src, '/api/media?ref=captures&path=pr54%2Fhaze-far.png');
});

test('leaves out links whose file name has a space', () => {
  const body = `Captures\n\n![a](https://github.com/${REPO}/blob/captures/pr54/haze%20far.png?raw=true)`;
  assert.equal(fromLinks(comment(body), onPr(null)), null);
});

test('comments without images are not visuals', () => {
  assert.equal(visualPost(comment('Claimed by codex. Plan: timers.'), onPr(null)), null);
});

test('plainText keeps the words and issue numbers', () => {
  assert.equal(plainText('**Bold** and [a link](https://x) with `code`'), 'Bold and a link with code');
  assert.equal(plainText('- a list item'), 'a list item');
  assert.equal(plainText('2. second step'), 'second step');
  assert.equal(plainText('## Heading'), 'Heading');
  assert.equal(plainText('#22 still needs a call'), '#22 still needs a call');
  assert.equal(plainText('2.84 ms for the step'), '2.84 ms for the step');
  assert.equal(plainText('-5 degrees'), '-5 degrees');
});

test('only plain capture paths and refs are served', () => {
  assert.ok(safePath('pr54/east-along.jpg'));
  assert.equal(safePath('issue8/haze far.png'), false, 'no spaces');
  for (const bad of ['../x.png', '/x.png', 'a//b.png', 'a/./b.png', '', 'a/..b/c.png']) assert.equal(safePath(bad), false, bad);
  assert.ok(safeRef('captures'));
  assert.ok(safeRef('0f28db9c3d1e2f'));
  assert.equal(safeRef('a..b'), false);
  assert.equal(safeRef('main?x=1'), false);
});
