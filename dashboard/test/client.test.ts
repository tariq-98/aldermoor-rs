import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ago, chipLabels, dayLabel, plural, pullState } from '../src/client/format.ts';
import { parseRoute } from '../src/client/view.ts';
import type { Pull } from '../src/shared/types.ts';

test('says how long ago, briefly', () => {
  const now = Date.parse('2026-10-25T12:00:00Z');
  const before = (seconds: number) => new Date(now - seconds * 1000).toISOString();
  assert.equal(ago(before(10), now), 'just now');
  assert.equal(ago(before(-30), now), 'just now', 'a clock a little ahead is not the future');
  assert.equal(ago(before(5 * 60), now), '5 min ago');
  assert.equal(ago(before(3 * 3600), now), '3 h ago');
  assert.equal(ago(before(2 * 86400), now), '2 d ago');
  assert.equal(ago('2026-03-01T12:00:00Z', now), '1 Mar');
});

test('heads each day of the feed', () => {
  const now = new Date(2026, 9, 25, 15, 0);
  assert.equal(dayLabel(new Date(2026, 9, 25, 0, 5).toISOString(), now), 'Today');
  assert.equal(dayLabel(new Date(2026, 9, 24, 23, 55).toISOString(), now), 'Yesterday');
  const older = dayLabel(new Date(2026, 9, 20, 12).toISOString(), now);
  assert.match(older, /^Tuesday,? 20 October$/);
});

test('counts with the right noun', () => {
  assert.equal(plural(1, 'post'), '1 post');
  assert.equal(plural(0, 'post'), '0 posts');
  assert.equal(plural(3, 'match', 'matches'), '3 matches');
});

test('leaves status and phase labels off the chips', () => {
  assert.deepEqual(chipLabels(['status:claimed', 'phase:2', 'area:render', 'type:bug']), ['area:render', 'type:bug']);
});

test('names a PR state', () => {
  const pull = (fields: Partial<Pull>) => ({ state: 'open', draft: false, ...fields }) as Pull;
  assert.deepEqual(pullState(pull({ state: 'merged' })), { name: 'Merged', tone: 'done' });
  assert.deepEqual(pullState(pull({ state: 'closed' })), { name: 'Closed', tone: 'bad' });
  assert.deepEqual(pullState(pull({ draft: true })), { name: 'Draft', tone: 'muted' });
  assert.deepEqual(pullState(pull({})), { name: 'Open', tone: 'ok' });
});

test('reads the tab and its filter from the hash', () => {
  assert.deepEqual(parseRoute(''), { tab: 'overview', arg: null });
  assert.deepEqual(parseRoute('#prs'), { tab: 'prs', arg: null });
  assert.deepEqual(parseRoute('#prs-merged'), { tab: 'prs', arg: 'merged' });
  assert.deepEqual(parseRoute('#issues-needs-decision'), { tab: 'issues', arg: 'needs-decision' });
  assert.deepEqual(parseRoute('#visuals-59'), { tab: 'visuals', arg: '59' });
  assert.deepEqual(parseRoute('#settings'), { tab: 'overview', arg: null });
  assert.deepEqual(parseRoute('#prs-<b>'), { tab: 'overview', arg: null });
});
