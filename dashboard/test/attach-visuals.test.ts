// The script agents post captures with, and the dashboard reading its comment back.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { crc32, deflateSync } from 'node:zlib';
import { fromLinks, fromManifest } from '../src/server/visuals.ts';

const SCRIPT = fileURLToPath(new URL('../../scripts/attach_visuals.py', import.meta.url));
const REPO = 'tariq-98/aldermoor-rs';
const python = spawnSync('python3', ['--version']).status === 0;

/** A real PNG of one colour, so any thumbnailer the machine has can read it. */
function png(width: number, height: number): Buffer {
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0);
    body.copy(out, 4);
    out.writeUInt32BE(crc32(body), body.length + 4);
    return out;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8);
  const row = Buffer.concat([Buffer.from([0]), Buffer.alloc(width * 3, 0x5a)]);
  return Buffer.concat([
    Buffer.from('89504e470d0a1a0a', 'hex'),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(Buffer.concat(Array.from({ length: height }, () => row)))),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

function post(args: string[]): string {
  const run = spawnSync('python3', [SCRIPT, ...args, '--dry-run'], { encoding: 'utf8', env: { ...process.env, ALDERMOOR_REPO: REPO } });
  assert.equal(run.status, 0, run.stderr);
  return run.stdout;
}

const comment = (body: string) => ({
  id: 1,
  html_url: `https://github.com/${REPO}/issues/8#issuecomment-1`,
  issue_url: `https://api.github.com/repos/${REPO}/issues/8`,
  body,
  created_at: '2026-10-25T10:00:00Z',
  updated_at: '2026-10-25T10:00:00Z',
});

test('the dashboard reads back what attach_visuals.py posts', { skip: !python && 'needs python3' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'attach-visuals-'));
  mkdirSync(join(dir, 'main'));
  writeFileSync(join(dir, 'East Cliff--1.PNG'), png(4, 3));
  writeFileSync(join(dir, 'main', 'east cliff 1.png'), png(4, 3));
  writeFileSync(join(dir, 'fight.mp4'), Buffer.alloc(2048));
  const title = 'Before -- after: <!-- not a comment --> [fog]';
  const captions = ['Café at dusk, "west" -- face [branch]', 'Same view on *main*', 'Wolf fight'];
  const body = post([
    '--issue', '8', '--title', title, '--verdict', 'fail', '--by', 'thread: Run Mac tasks from home folder',
    '--head', 'abc1234def', '--item', join(dir, 'East Cliff--1.PNG'), captions[0]!,
    '--item', join(dir, 'main', 'east cliff 1.png'), captions[1]!, '--item', join(dir, 'fight.mp4'), captions[2]!,
  ]);

  const read = fromManifest(comment(body), { repo: REPO, number: 8, kind: 'issue', headSha: null });
  assert.ok(read, body);
  assert.equal(read.title, title);
  assert.equal(read.verdict, 'fail');
  assert.equal(read.by, 'thread: Run Mac tasks from home folder');
  assert.equal(read.head, 'abc1234def');
  assert.deepEqual(read.items.map((i) => [i.type, i.caption]), [['image', captions[0]], ['image', captions[1]], ['video', captions[2]]]);
  assert.deepEqual(read.items.map((i) => i.github.slice(i.github.indexOf('/DRY-RUN/') + 9)),
    ['issue8/east-cliff-1.png', 'issue8/east-cliff-1-2.png', 'issue8/fight.mp4'], 'file names made safe and kept apart');

  const visible = body.replace(/<!--\s*aldermoor-visuals[\s\S]*?-->/, '');
  assert.doesNotMatch(visible, /(?:^|[^\\])<!--/, 'a title cannot start a comment that hides the rest of the post on GitHub');
  assert.match(visible, /\*\*Visuals: Before -- after: \\<!-- not a comment --\\> \\\[fog\\\]\*\* \(FAIL\)/);
});

test('a post with no manifest still shows its images through the links', { skip: !python && 'needs python3' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'attach-visuals-'));
  writeFileSync(join(dir, 'den.png'), png(2, 2));
  const body = post(['--pr', '54', '--title', 'Den', '--item', join(dir, 'den.png'), 'Wolf den at dusk']);
  const stripped = body.replace(/<!--[\s\S]*?-->/, '');
  const read = fromLinks(comment(stripped), { repo: REPO, number: 54, kind: 'pr', headSha: null });
  assert.deepEqual(read?.items.map((i) => i.caption), ['Wolf den at dusk']);
});
