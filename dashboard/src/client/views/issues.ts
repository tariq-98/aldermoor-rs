// Issues by status, grouped by phase, with who has each one and the PRs that close it.

import type { Issue, Milestone, Status } from '../../shared/types.ts';
import { h } from '../dom.ts';
import { STATUS_SHORT } from '../format.ts';
import { empty, issueRow, STATUS_TONES } from '../parts.ts';
import type { ViewContext } from '../view.ts';

/** Open statuses in the order they need attention. */
const ORDER: Status[] = ['needs-decision', 'blocked', 'in-review', 'claimed', 'ready', 'untriaged'];

interface Filter {
  key: string;
  name: string;
  tone: string;
  test: (issue: Issue) => boolean;
}

const FILTERS: Filter[] = [
  { key: 'open', name: 'All open', tone: 'neutral', test: (i) => i.state === 'open' },
  ...ORDER.map((status) => ({
    key: status,
    name: STATUS_SHORT[status],
    tone: STATUS_TONES[status],
    test: (i: Issue) => i.state === 'open' && i.status === status,
  })),
  { key: 'done', name: 'Done', tone: 'done', test: (i) => i.state === 'closed' },
];

const byStatus = (a: Issue, b: Issue) => ORDER.indexOf(a.status) - ORDER.indexOf(b.status) || a.number - b.number;
const byClosed = (a: Issue, b: Issue) => (b.closedAt ?? '').localeCompare(a.closedAt ?? '');

/** Groups in phase order, then issues in a milestone the snapshot doesn't list, then no phase. */
function byPhase(issues: Issue[], milestones: Milestone[], done: boolean): { title: string; issues: Issue[]; milestone: Milestone | null }[] {
  const groups = new Map<number | null, Issue[]>();
  for (const issue of issues) groups.set(issue.milestone, [...(groups.get(issue.milestone) ?? []), issue]);
  const known = milestones.map((m) => m.number);
  const order = [...known, ...[...groups.keys()].filter((n): n is number => n !== null && !known.includes(n)), null];
  return order.filter((n) => groups.has(n)).map((n) => {
    const milestone = milestones.find((m) => m.number === n) ?? null;
    return {
      title: milestone?.title ?? (n === null ? 'No phase' : `Milestone ${n}`),
      issues: groups.get(n)!.sort(done ? byClosed : byStatus),
      milestone,
    };
  });
}

export function issuesView(view: ViewContext): HTMLElement {
  const s = view.snapshot;
  const filter = FILTERS.find((f) => f.key === view.route.arg) ?? FILTERS[0]!;
  const groups = byPhase(s.issues.filter(filter.test), s.milestones, filter.key === 'done');
  return h('div', { class: 'page' },
    h('div', { class: 'page-head' }, h('h1', null, 'Issues')),
    h('nav', { class: 'chips', 'aria-label': 'Issues by status' }, FILTERS.map((f) =>
      h('a', {
        class: `chip tone-${f.tone}`,
        href: f.key === 'open' ? '#issues' : `#issues-${f.key}`,
        'aria-current': f === filter ? 'page' : null,
      }, f.name, h('span', { class: 'count' }, String(s.issues.filter(f.test).length))))),
    groups.length
      ? groups.map((group) => h('section', { class: 'card' },
        h('header', { class: 'card-head' },
          h('h2', null, group.title, h('span', { class: 'count' }, String(group.issues.length))),
          group.milestone ? h('span', { class: 'card-note' }, `${group.milestone.closed} of ${group.milestone.open + group.milestone.closed} done`) : null),
        h('ul', { class: 'items' }, group.issues.map((i) => issueRow(view, i, { status: filter.key === 'open' })))))
      : h('section', { class: 'card' }, empty(filter.key === 'open' ? 'No open issues.' : `No issues here: ${filter.name.toLowerCase()} is empty.`)));
}
