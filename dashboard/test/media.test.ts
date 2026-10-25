import assert from 'node:assert/strict';
import { test } from 'node:test';
import { GitHub } from '../src/server/github.ts';
import { CHUNK, contentType, lfsPointer, parseRange, serveMedia } from '../src/server/media.ts';
import { fakeFetch, json, request } from './fake.ts';

test('knows the type of each capture format', () => {
  assert.equal(contentType('pr54/east.JPG'), 'image/jpeg');
  assert.equal(contentType('pr54/clip.mov'), 'video/quicktime');
  assert.equal(contentType('pr54/notes.txt'), null);
  assert.equal(contentType('README'), null);
});

test('reads one byte range, or none', () => {
  assert.deepEqual(parseRange('bytes=0-99', 1000), { start: 0, end: 99 });
  assert.deepEqual(parseRange('bytes=500-', 1000), { start: 500, end: 999 });
  assert.deepEqual(parseRange('bytes=-100', 1000), { start: 900, end: 999 });
  assert.deepEqual(parseRange('bytes=-5000', 1000), { start: 0, end: 999 });
  assert.deepEqual(parseRange('bytes=0-5000', 1000), { start: 0, end: 999 });
  assert.equal(parseRange('bytes=1000-', 1000), 'invalid');
  assert.equal(parseRange('bytes=9-3', 1000), 'invalid');
  assert.equal(parseRange(undefined, 1000), null);
  assert.equal(parseRange('bytes=-', 1000), null);
  assert.equal(parseRange('bytes=0-1,5-6', 1000), null, 'several ranges get the whole file');
});

test('spots a Git LFS pointer', () => {
  const oid = 'f'.repeat(64);
  const pointer = Buffer.from(`version https://git-lfs.github.com/spec/v1\noid sha256:${oid}\nsize 123456\n`);
  assert.deepEqual(lfsPointer(pointer), { oid, size: 123456 });
  assert.equal(lfsPointer(Buffer.from([0x89, 0x50, 0x4e, 0x47])), null);
  assert.equal(lfsPointer(null), null);
});

const PNG = Buffer.from('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c489', 'hex');

/** GitHub with one small file at captures:pr54/a.png and one too big to pass through. */
function repo(token: string) {
  const { fetcher, calls } = fakeFetch((call) => {
    if (call.path.endsWith('/contents/pr54/a.png')) {
      return json({ type: 'file', sha: 'abc123', size: PNG.length, download_url: 'https://raw.example/a.png', content: PNG.toString('base64'), encoding: 'base64' });
    }
    if (call.path.endsWith('/contents/pr54/big.mp4')) {
      return json({ type: 'file', sha: 'def456', size: CHUNK * 3, download_url: 'https://raw.example/big.mp4?token=signed', content: '', encoding: 'none' });
    }
    return json({ message: 'Not Found' }, 404);
  });
  return { gh: new GitHub('tariq-98/aldermoor-rs', token, fetcher), calls };
}

test('serves a small capture with its type and an ETag', async () => {
  const { gh, calls } = repo('test_media_whole_000000');
  const reply = await request((req, res) => serveMedia(gh, req, res, 'captures', 'pr54/a.png'));
  assert.equal(reply.status, 200);
  assert.equal(reply.headers.get('content-type'), 'image/png');
  assert.equal(reply.headers.get('etag'), '"abc123"');
  assert.equal(reply.headers.get('cache-control'), 'private, max-age=300', 'a branch can move, so only briefly');
  assert.deepEqual(reply.body, PNG);
  assert.equal(calls[0]?.params.get('ref'), 'captures');
});

test('answers ranges and repeat visits', async () => {
  const { gh } = repo('test_media_range_000000');
  const commit = '0123456789abcdef0123456789abcdef01234567';
  const part = await request((req, res) => serveMedia(gh, req, res, commit, 'pr54/a.png'), { headers: { Range: 'bytes=0-7' } });
  assert.equal(part.status, 206);
  assert.equal(part.headers.get('content-range'), `bytes 0-7/${PNG.length}`);
  assert.deepEqual(part.body, PNG.subarray(0, 8));
  assert.equal(part.headers.get('cache-control'), 'private, max-age=31536000, immutable', 'a commit never changes');
  const again = await request((req, res) => serveMedia(gh, req, res, commit, 'pr54/a.png'), { headers: { 'If-None-Match': '"abc123"' } });
  assert.equal(again.status, 304);
  const past = await request((req, res) => serveMedia(gh, req, res, commit, 'pr54/a.png'), { headers: { Range: 'bytes=999-' } });
  assert.equal(past.status, 416);
});

test('sends big files to GitHub instead of through the function', async () => {
  const { gh } = repo('test_media_big_00000000');
  const reply = await request((req, res) => serveMedia(gh, req, res, 'captures', 'pr54/big.mp4'));
  assert.equal(reply.status, 302);
  assert.equal(reply.headers.get('location'), 'https://raw.example/big.mp4?token=signed');
});

test('refuses anything that is not a capture', async () => {
  const { gh, calls } = repo('test_media_refuse_00000');
  for (const [ref, path] of [['captures', '../secret.png'], ['captures', 'pr54/notes.txt'], ['a..b', 'pr54/a.png']] as const) {
    const reply = await request((req, res) => serveMedia(gh, req, res, ref, path));
    assert.equal(reply.status, 400, `${ref} ${path}`);
  }
  assert.equal(calls.length, 0, 'GitHub is never asked');
  const missing = await request((req, res) => serveMedia(gh, req, res, 'captures', 'pr54/gone.png'));
  assert.equal(missing.status, 404);
});
