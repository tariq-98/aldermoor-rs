import assert from 'node:assert/strict';
import { test } from 'node:test';
import { GitHub } from '../src/server/github.ts';
import type { RawComment, RawCommit, RawIssue, RawMilestone, RawPull, RawRun } from '../src/server/raw.ts';
import { buildSnapshot, closes, gist, statusOf } from '../src/server/snapshot.ts';
import { fakeFetch, json, type Call } from './fake.ts';

const REPO = 'tariq-98/aldermoor-rs';
const WEB = `https://github.com/${REPO}`;
const MAIN = 'd'.repeat(40);
const FOG = 'a'.repeat(40);
const TIMERS = 'b'.repeat(40);

test('reads the status label, and closed means done', () => {
  assert.equal(statusOf({ state: 'open', labels: [{ name: 'area:render', color: '' }, { name: 'status:blocked', color: '' }] }), 'blocked');
  assert.equal(statusOf({ state: 'open', labels: ['status:needs-decision'] }), 'needs-decision');
  assert.equal(statusOf({ state: 'open', labels: [] }), 'untriaged');
  assert.equal(statusOf({ state: 'closed', labels: ['status:claimed'] }), 'done');
});

test('finds the issues a PR closes, as the Tracking workflow does', () => {
  assert.deepEqual(closes('Closes #57. Also fixes #3 and resolves #3.'), [57, 3]);
  assert.deepEqual(closes('Part of #12, see #13'), []);
  assert.deepEqual(closes(null), []);
});

test('a comment gist is its first line as plain text', () => {
  assert.equal(gist('<!-- marker -->\n\n**Mac check: PASS** on the M1\nmore'), 'Mac check: PASS on the M1');
  assert.equal(gist('x'.repeat(200)).length, 140);
});

function issue(number: number, title: string, fields: Partial<RawIssue>): RawIssue {
  return {
    number,
    title,
    html_url: `${WEB}/${fields.pull_request ? 'pull' : 'issues'}/${number}`,
    state: 'open',
    created_at: '2026-10-20T09:00:00Z',
    updated_at: '2026-10-20T09:00:00Z',
    closed_at: null,
    body: '',
    labels: [],
    milestone: null,
    ...fields,
  };
}

function comment(id: number, number: number, at: string, body: string): RawComment {
  return { id, html_url: `${WEB}/issues/${number}#issuecomment-${id}`, issue_url: `https://api.github.com/repos/${REPO}/issues/${number}`, body, created_at: at, updated_at: at };
}

function commit(sha: string, message: string, date: string): RawCommit {
  return { sha, html_url: `${WEB}/commit/${sha}`, commit: { message, committer: { date }, author: { date } } };
}

function run(name: string, workflow: number, sha: string, status: string, conclusion: string | null, attempt = 1): RawRun {
  return { id: workflow * 10 + attempt, name, workflow_id: workflow, head_sha: sha, head_branch: null, event: 'push', status, conclusion, run_number: 4, run_attempt: attempt, html_url: `${WEB}/actions/runs/${workflow}`, created_at: '2026-10-24T10:00:00Z' };
}

const ISSUES: RawIssue[] = [
  issue(1, 'Fix the fog', { labels: [{ name: 'status:claimed', color: 'fbca04' }, { name: 'area:render', color: '1d76db' }], milestone: { number: 2 }, updated_at: '2026-10-24T10:00:00Z' }),
  issue(2, 'Pick the camera', { labels: ['status:needs-decision'], updated_at: '2026-10-23T10:00:00Z' }),
  issue(3, 'Old bug', { state: 'closed', state_reason: 'completed', closed_at: '2026-10-22T12:00:05Z', updated_at: '2026-10-22T12:00:10Z', milestone: { number: 1 } }),
  issue(4, 'Fog tuning', { body: 'Closes #1', pull_request: { merged_at: null }, created_at: '2026-10-21T09:00:00Z', updated_at: '2026-10-24T11:00:00Z' }),
  issue(5, 'Fix the old bug', { body: 'Fixes #3', state: 'closed', pull_request: { merged_at: '2026-10-22T12:00:00Z' }, closed_at: '2026-10-22T12:00:00Z', updated_at: '2026-10-22T12:00:05Z' }),
  issue(6, 'Camera try', { body: 'Closes #2', state: 'closed', pull_request: { merged_at: null }, closed_at: '2026-10-22T15:00:00Z', updated_at: '2026-10-22T15:00:00Z' }),
  issue(7, 'GPU timers', { body: 'Closes #9', draft: true, pull_request: { merged_at: null }, created_at: '2026-10-23T09:00:00Z', updated_at: '2026-10-23T09:00:00Z' }),
  issue(8, 'Not doing this', { state: 'closed', state_reason: 'not_planned', closed_at: '2026-10-23T08:00:00Z', updated_at: '2026-10-23T08:00:00Z' }),
];

const PULLS: RawPull[] = [
  { number: 4, draft: false, head: { ref: '1-fog', sha: FOG } },
  { number: 7, draft: true, head: { ref: 'codex/26-gpu-timers', sha: TIMERS } },
];

const manifest = JSON.stringify({ v: 1, title: 'Fog check', verdict: 'pass', head: FOG.slice(0, 7), ref: 'captures', items: [{ path: 'pr4/fog.jpg', caption: 'Fog at 60 m' }] });
const COMMENTS: RawComment[] = [
  comment(101, 1, '2026-10-20T09:30:00Z', 'Claimed by thread: Fix the fog. Plan: retune the haze curve. Branch: 1-fog'),
  comment(102, 2, '2026-10-21T08:00:00Z', '**Which camera?** Orbit or follow, needs a call.'),
  comment(104, 3, '2026-10-22T13:00:00Z', `**Mac check: PASS (head 1234567)**\n\n![x](${WEB}/blob/captures/issue3/before.png?raw=true)`),
  comment(103, 4, '2026-10-24T10:30:00Z', `<!-- aldermoor-visuals ${manifest} -->\n**Fog check**`),
];

const MILESTONES: RawMilestone[] = [
  { number: 1, title: 'Phase 10: Polish', description: null, state: 'open', open_issues: 0, closed_issues: 1, html_url: `${WEB}/milestone/1` },
  { number: 2, title: 'Phase 2: World', description: 'The map', state: 'open', open_issues: 1, closed_issues: 0, html_url: `${WEB}/milestone/2` },
];

const COMMITS: RawCommit[] = [
  commit(MAIN, 'Fix the old bug (#5)\n\nSquashed.', '2026-10-22T12:00:00Z'),
  commit('e'.repeat(40), 'Tidy the README', '2026-10-21T07:00:00Z'),
];

const RUNS: RawRun[] = [
  run('CI', 1, MAIN, 'completed', 'success'),
  run('Perf', 2, MAIN, 'completed', 'failure'),
  run('CI', 1, FOG, 'completed', 'failure', 1),
  run('CI', 1, FOG, 'in_progress', null, 2),
];

/** The repo above as GitHub's REST API serves it; `actions` answers the Actions runs. */
function github(actions: (call: Call) => Response = (call) => json({ workflow_runs: RUNS.filter((r) => r.head_sha === call.params.get('head_sha')) })) {
  const base = `/repos/${REPO}`;
  const paged = (items: unknown[], call: Call) => json(call.params.get('page') === '1' ? items : []);
  return fakeFetch((call) => {
    switch (call.path) {
      case `${base}/issues`: return paged(ISSUES, call);
      case `${base}/issues/comments`: return paged(COMMENTS, call);
      case `${base}/pulls`: return json(PULLS);
      case `${base}/milestones`: return json(MILESTONES);
      case `${base}/commits`: return json(COMMITS);
      case `${base}/actions/runs`: return actions(call);
      default: return json({ message: 'Not Found' }, 404);
    }
  });
}

test('builds the snapshot from the repo', async () => {
  const s = await buildSnapshot(new GitHub(REPO, 'test_snapshot_token_0000', github().fetcher));
  assert.deepEqual(s.warnings, []);

  assert.deepEqual(s.issues.map((i) => [i.number, i.status, i.pulls]), [
    [1, 'claimed', [4]],
    [2, 'needs-decision', []],
    [8, 'done', []],
    [3, 'done', [5]],
  ], 'open first, then closed, each newest first; a PR closed without merging does not count');
  const fog = s.issues[0]!;
  assert.deepEqual(fog.claim, { agent: 'thread: Fix the fog', at: '2026-10-20T09:30:00Z', url: `${WEB}/issues/1#issuecomment-101`, plan: 'retune the haze curve' });
  assert.equal(fog.milestone, 2);
  assert.deepEqual(s.issues[1]!.labels, [{ name: 'status:needs-decision', color: 'cccccc' }]);

  assert.deepEqual(s.pulls.map((p) => [p.number, p.state, p.draft, p.agent, p.ci?.state ?? null]), [
    [4, 'open', false, 'thread: Fix the fog', 'running'],
    [7, 'open', true, 'codex', 'none'],
    [6, 'closed', false, null, null],
    [5, 'merged', false, null, null],
  ]);
  assert.equal(s.pulls[0]!.headSha, FOG);
  assert.equal(s.pulls[0]!.branch, '1-fog');
  assert.equal(s.pulls[3]!.closedAt, '2026-10-22T12:00:00Z', 'merged PRs close when they merge');

  assert.equal(s.main?.sha, MAIN);
  assert.equal(s.main?.message, 'Fix the old bug (#5)');
  assert.equal(s.main?.ci.state, 'success', 'Perf only reports');
  assert.deepEqual(s.main?.ci.runs.map((r) => r.name), ['CI', 'Perf']);

  assert.deepEqual(s.milestones.map((m) => m.title), ['Phase 2: World', 'Phase 10: Polish']);

  assert.deepEqual(s.visuals.map((v) => [v.id, v.kind, v.number, v.legacy, v.current]), [
    [103, 'pr', 4, false, true],
    [104, 'issue', 3, true, null],
  ]);

  const activity = new Map(s.activity.map((a) => [a.key, a]));
  assert.equal(activity.get('comment-101')?.text, 'Claimed by thread: Fix the fog');
  assert.equal(activity.get('comment-101')?.kind, 'claim');
  assert.equal(activity.get('comment-102')?.text, 'Which camera? Orbit or follow, needs a call.');
  assert.equal(activity.get('comment-102')?.subject, 'Pick the camera');
  assert.equal(activity.get('comment-103')?.text, 'Fog check · 1 capture');
  assert.equal(activity.get('comment-103')?.verdict, 'pass');
  assert.equal(activity.get('pr-7-open')?.text, 'Draft PR opened');
  assert.equal(activity.get('pr-6-closed')?.text, 'Closed without merging');
  assert.equal(activity.get('issue-8-closed')?.text, 'Closed as not planned');
  assert.equal(activity.get('issue-3-closed')?.text, 'Done');
  assert.ok(activity.has(`push-${'e'.repeat(40)}`), 'a push straight to main shows');
  assert.ok(!activity.has(`push-${MAIN}`), 'a squash merge shows as the merge, not as a push');
  assert.deepEqual(s.activity.map((a) => a.at), [...s.activity.map((a) => a.at)].sort().reverse(), 'newest first');
});

test('a token without Actions access still gets the rest, with a warning', async () => {
  const s = await buildSnapshot(new GitHub(REPO, 'test_snapshot_no_actions', github(() => json({ message: 'Resource not accessible by personal access token' }, 403)).fetcher));
  assert.deepEqual(s.warnings, ['CI results need a token that can read Actions.']);
  assert.equal(s.main?.ci.state, 'none');
  assert.equal(s.pulls[0]!.ci, null);
  assert.equal(s.issues.length, 4);
});

test('other GitHub failures are not hidden', async () => {
  await assert.rejects(buildSnapshot(new GitHub(REPO, 'test_snapshot_down_00000', github(() => json({ message: 'Server Error' }, 500)).fetcher)), /Server Error/);
});
