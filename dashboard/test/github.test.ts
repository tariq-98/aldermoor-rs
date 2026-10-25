import assert from 'node:assert/strict';
import { test } from 'node:test';
import { GitHub, GitHubError, inBatches, lastPage } from '../src/server/github.ts';
import { fakeFetch, json } from './fake.ts';

const REPO = 'tariq-98/aldermoor-rs';

test('reads the last page from a Link header', () => {
  const link = '<https://api.github.com/repositories/1/issues?per_page=100&page=2>; rel="next", ' +
    '<https://api.github.com/repositories/1/issues?per_page=100&page=7>; rel="last"';
  assert.equal(lastPage(link), 7);
  assert.equal(lastPage('<https://api.github.com/x?page=1>; rel="prev"'), null);
  assert.equal(lastPage(null), null);
});

test('runs tasks a few at a time, in order', async () => {
  let running = 0;
  let most = 0;
  const tasks = Array.from({ length: 10 }, (_, i) => async () => {
    running += 1;
    most = Math.max(most, running);
    await new Promise((resolve) => setTimeout(resolve, 5 - (i % 3)));
    running -= 1;
    return i;
  });
  assert.deepEqual(await inBatches(tasks, 3), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  assert.equal(most, 3);
  assert.deepEqual(await inBatches([], 3), []);
});

test('asks again with the ETag and reuses the body on a 304', async () => {
  const { fetcher, calls } = fakeFetch((call) =>
    call.headers.get('if-none-match') === '"v1"' ? new Response(null, { status: 304 }) : json([{ number: 1 }], 200, { ETag: '"v1"' }));
  const gh = new GitHub(REPO, 'test_etag_token_0000000', fetcher);
  assert.deepEqual((await gh.get('repo:/milestones')).data, [{ number: 1 }]);
  assert.deepEqual((await gh.get('repo:/milestones')).data, [{ number: 1 }]);
  assert.equal(calls[0]?.path, `/repos/${REPO}/milestones`);
  assert.equal(calls[0]?.headers.get('authorization'), 'Bearer test_etag_token_0000000');
  assert.equal(calls[1]?.headers.get('if-none-match'), '"v1"');
  const other = new GitHub(REPO, 'test_etag_other_000000', fetcher);
  await other.get('repo:/milestones');
  assert.equal(calls[2]?.headers.get('if-none-match'), null, 'each token has its own cache');
});

test('turns GitHub errors into GitHubError with its message', async () => {
  const { fetcher } = fakeFetch(() => json({ message: 'Resource not accessible by personal access token' }, 403));
  const gh = new GitHub(REPO, 'test_error_token_000000', fetcher);
  await assert.rejects(gh.get('repo:/actions/runs'), (error: unknown) => {
    assert.ok(error instanceof GitHubError);
    assert.equal(error.status, 403);
    assert.equal(error.path, `/repos/${REPO}/actions/runs`);
    assert.equal(error.message, 'Resource not accessible by personal access token');
    return true;
  });
});

/** A list of `count` items served 100 a page, whose page 1 Link header says `claimed` pages. */
function list(count: number, claimed: number) {
  return fakeFetch((call) => {
    const page = Number(call.params.get('page'));
    const items = Array.from({ length: Math.max(0, Math.min(100, count - (page - 1) * 100)) }, (_, i) => (page - 1) * 100 + i);
    const link = page === 1 && claimed > 1 ? `<https://api.github.com/x?per_page=100&page=${claimed}>; rel="last"` : '';
    return json(items, 200, link ? { Link: link } : {});
  });
}

test('pages through a list, even when the Link header is a page behind', async () => {
  const exact = list(250, 3);
  assert.equal((await new GitHub(REPO, 'test_pages_exact_00000', exact.fetcher).all('repo:/issues?state=all')).length, 250);
  assert.deepEqual(exact.calls.map((c) => Number(c.params.get('page'))), [1, 2, 3, 4], 'one page past the last is checked');

  const behind = list(420, 3);
  const items = await new GitHub(REPO, 'test_pages_behind_0000', behind.fetcher).all<number>('repo:/issues?state=all');
  assert.equal(items.length, 420);
  assert.deepEqual(items.slice(-2), [418, 419]);

  const one = list(40, 1);
  assert.equal((await new GitHub(REPO, 'test_pages_one_0000000', one.fetcher).all('repo:/issues')).length, 40);
  assert.equal(one.calls[0]?.params.get('per_page'), '100');
});

test('keeps only the newest pages of a long list', async () => {
  const long = list(1000, 10);
  const items = await new GitHub(REPO, 'test_pages_long_000000', long.fetcher).all<number>('repo:/issues/comments', 4);
  assert.deepEqual([items[0], items.at(-1), items.length], [700, 999, 300], 'pages 8 to 10, and 11 to check for more');
});
