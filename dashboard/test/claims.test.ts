import assert from 'node:assert/strict';
import { test } from 'node:test';
import { claimAgent, claimEvent, currentClaim } from '../src/server/claims.ts';
import type { RawComment } from '../src/server/raw.ts';

let id = 0;
function comment(body: string, at: string): RawComment {
  id += 1;
  return {
    id,
    html_url: `https://github.com/tariq-98/aldermoor-rs/issues/8#issuecomment-${id}`,
    issue_url: 'https://api.github.com/repos/tariq-98/aldermoor-rs/issues/8',
    body,
    created_at: at,
    updated_at: at,
  };
}

test('only a first line that claims or releases counts', () => {
  assert.equal(claimEvent('Claimed by codex. Plan: timers.'), 'claim');
  assert.equal(claimEvent('\n  Claimed by thread: Fog'), 'claim');
  assert.equal(claimEvent('Released: out of time, the branch has the start.'), 'release');
  assert.equal(claimEvent('The rule is:\nClaimed by <agent>. Plan: ...'), null, 'quoting the rule is not a claim');
  assert.equal(claimEvent('Claimed-by codex'), null);
  assert.equal(claimEvent(null), null);
});

test('reads the agent without notes on the branch or a takeover', () => {
  assert.equal(claimAgent('Claimed by thread: Fix the fog. Plan: retune the haze. Branch: 8-fog'), 'thread: Fix the fog');
  assert.equal(claimAgent('Claimed by codex (branch codex/26-gpu-timers)'), 'codex');
  assert.equal(claimAgent('Claimed by local: ~/code/aldermoor (taking over from thread: Fog)'), 'local: ~/code/aldermoor');
  assert.equal(claimAgent('Claimed by PR #49.'), 'PR #49');
  assert.equal(claimAgent('Not a claim'), null);
});

test('the earliest claim holds, with its plan', () => {
  const claim = currentClaim([
    comment('Claimed by codex. Plan: also wants it.', '2026-10-24T11:00:00Z'),
    comment('Claimed by thread: Fix the fog. Plan: retune the haze curve. Branch: 8-fog', '2026-10-24T10:00:00Z'),
  ]);
  assert.equal(claim?.agent, 'thread: Fix the fog');
  assert.equal(claim?.plan, 'retune the haze curve');
  assert.equal(claim?.at, '2026-10-24T10:00:00Z');
});

test('a release ends the claim and the next one holds', () => {
  const claimed = comment('Claimed by codex. Plan: timers.', '2026-10-24T10:00:00Z');
  const released = comment('Released: blocked on #22.', '2026-10-24T12:00:00Z');
  assert.equal(currentClaim([claimed, released]), null);
  const next = comment('Claimed by thread: Timers', '2026-10-24T13:00:00Z');
  assert.equal(currentClaim([claimed, released, next])?.agent, 'thread: Timers');
  assert.equal(currentClaim([claimed, released, next])?.plan, undefined);
  assert.equal(currentClaim([claimed])?.plan, 'timers', 'without the full stop');
});

test('a takeover replaces a stale claim', () => {
  const stale = comment('Claimed by thread: Old. Plan: something.', '2026-10-20T10:00:00Z');
  const inline = comment('Claimed by codex (taking over: no activity for three days). Plan: finish it.', '2026-10-24T10:00:00Z');
  assert.equal(currentClaim([stale, inline])?.agent, 'codex');
  const notice = comment('Taking this over, the claim has gone stale.', '2026-10-24T10:00:00Z');
  const after = comment('Claimed by thread: New. Plan: finish it.', '2026-10-24T10:01:00Z');
  assert.equal(currentClaim([stale, notice, after])?.agent, 'thread: New');
});
