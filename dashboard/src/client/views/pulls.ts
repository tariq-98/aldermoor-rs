// Pull requests: the open ones with their CI, then what was merged or closed, newest first.

import type { Pull } from '../../shared/types.ts';
import { h, link } from '../dom.ts';
import { empty, pullRow } from '../parts.ts';
import type { ViewContext } from '../view.ts';

const FILTERS = [
  { key: 'open', name: 'Open', href: '#prs', none: 'No PRs open right now.' },
  { key: 'merged', name: 'Merged', href: '#prs-merged', none: 'Nothing merged yet.' },
  { key: 'closed', name: 'Closed', href: '#prs-closed', none: 'No PRs closed without merging.' },
] as const;

const newestClosed = (a: Pull, b: Pull) => (b.closedAt ?? '').localeCompare(a.closedAt ?? '');

export function pullsView(view: ViewContext): HTMLElement {
  const s = view.snapshot;
  const current = FILTERS.find((f) => f.key === view.route.arg) ?? FILTERS[0];
  const lists: Record<Pull['state'], Pull[]> = { open: [], merged: [], closed: [] };
  for (const pull of s.pulls) lists[pull.state].push(pull);
  lists.merged.sort(newestClosed);
  lists.closed.sort(newestClosed);
  const shown = lists[current.key];
  return h('div', { class: 'page' },
    h('div', { class: 'page-head' }, h('h1', null, 'Pull requests')),
    h('nav', { class: 'segmented', 'aria-label': 'Pull requests by state' }, FILTERS.map((f) =>
      h('a', { href: f.href, 'aria-current': f.key === current.key ? 'page' : null }, f.name, h('span', { class: 'count' }, String(lists[f.key].length))))),
    h('section', { class: 'card' },
      shown.length ? h('ul', { class: 'items' }, shown.map((p) => pullRow(view, p))) : empty(current.none, 'pr')),
    current.key === 'open'
      ? null
      : h('p', { class: 'page-note' }, 'The latest are here; older ones are on ',
        link(`https://github.com/${s.repo}/pulls?q=is%3Apr+is%3Aclosed`, {}, 'GitHub'), '.'));
}
