import assert from 'node:assert/strict';
import { test } from 'node:test';
import { overall, runState, summarise } from '../src/server/ci.ts';
import type { RawRun } from '../src/server/raw.ts';
import type { CiRun } from '../src/shared/types.ts';

const SHA = 'a'.repeat(40);

function run(name: string, workflow: number, fields: Partial<RawRun> = {}): RawRun {
  return {
    id: Math.floor(Math.random() * 1e9),
    name,
    workflow_id: workflow,
    head_sha: SHA,
    head_branch: '8-fog',
    event: 'pull_request',
    status: 'completed',
    conclusion: 'success',
    run_number: 1,
    run_attempt: 1,
    html_url: `https://github.com/tariq-98/aldermoor-rs/actions/runs/${workflow}`,
    created_at: '2026-10-24T10:00:00Z',
    ...fields,
  };
}

test('a run is passing, failing, running or cancelled', () => {
  const state = (status: string, conclusion: string | null) => runState({ status, conclusion });
  assert.equal(state('completed', 'success'), 'success');
  assert.equal(state('completed', 'skipped'), 'success');
  assert.equal(state('completed', 'neutral'), 'success');
  assert.equal(state('completed', 'failure'), 'failure');
  assert.equal(state('completed', 'timed_out'), 'failure');
  assert.equal(state('completed', 'startup_failure'), 'failure');
  assert.equal(state('completed', 'cancelled'), 'cancelled');
  assert.equal(state('completed', 'action_required'), 'running');
  assert.equal(state('in_progress', null), 'running');
  assert.equal(state('queued', null), 'running');
});

test('informational workflows never decide the overall state', () => {
  const ci = (name: string, state: CiRun['state'], informational = false): CiRun => ({ name, state, url: '', informational });
  assert.equal(overall([]), 'none');
  assert.equal(overall([ci('CI', 'success'), ci('Perf', 'failure', true)]), 'success');
  assert.equal(overall([ci('CI', 'running'), ci('Lint', 'failure')]), 'failure');
  assert.equal(overall([ci('CI', 'success'), ci('Lint', 'running')]), 'running');
  assert.equal(overall([ci('CI', 'cancelled')]), 'cancelled');
  assert.equal(overall([ci('Wiki', 'failure', true)]), 'failure', 'with nothing else, the informational ones report');
});

test('keeps the latest attempt of each workflow on the commit', () => {
  const summary = summarise([
    run('CI', 1, { run_number: 7, conclusion: 'failure' }),
    run('CI', 1, { run_number: 7, run_attempt: 2, status: 'in_progress', conclusion: null }),
    run('CI', 1, { run_number: 6, conclusion: 'success' }),
    run('Perf', 2, { conclusion: 'failure' }),
    run('Build', 3, { head_sha: 'b'.repeat(40), conclusion: 'failure' }),
    run('Assets', 4),
  ], SHA);
  assert.deepEqual(summary.runs.map((r) => [r.name, r.state, r.informational]), [
    ['Assets', 'success', false],
    ['CI', 'running', false],
    ['Perf', 'failure', true],
  ], 'blocking workflows first, then by name; other commits left out');
  assert.equal(summary.state, 'running');
});
