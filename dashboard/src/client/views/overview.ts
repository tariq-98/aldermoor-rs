// The first tab: what needs Chippy, the board in numbers, main's CI, the PRs in flight, the
// phases, the latest captures and everything that happened, newest first.

import type { Issue, Pull, Snapshot, Status } from '../../shared/types.ts';
import { h, icon, link } from '../dom.ts';
import { plural, STATUS_SHORT, VERDICT_NAMES } from '../format.ts';
import {
  activityList,
  ciDetails,
  empty,
  frame,
  issueRow,
  pill,
  pullRow,
  section,
  STATUS_ICONS,
  STATUS_TONES,
  time,
  VERDICT_ICONS,
  VERDICT_TONES,
} from '../parts.ts';
import type { ViewContext } from '../view.ts';

const TILES: Status[] = ['needs-decision', 'blocked', 'in-review', 'claimed', 'ready'];
const PAGE = 30;

// How much of the feed is open; "Show more" adds a page. Kept across redraws.
let shown = PAGE;

const MAIN_STATE = { success: 'main passing', failure: 'main failing', running: "main's CI running", cancelled: "main's CI cancelled", none: null };
const TOKEN_FROM: Record<string, string> = { cookie: " with this browser's token", env: " with the project's token", dev: ' through the dev proxy' };

export function overviewView(view: ViewContext): HTMLElement {
  const s = view.snapshot;
  const open = s.issues.filter((i) => i.state === 'open');
  const pulls = s.pulls.filter((p) => p.state === 'open');
  return h('div', { class: 'page overview' },
    h('div', { class: 'page-head' }, h('h1', null, 'Overview'), h('p', { class: 'lede' }, summary(s, open, pulls))),
    s.warnings.length
      ? h('div', { class: 'banner tone-warn', role: 'note' }, icon('warning'), h('div', null, s.warnings.map((w) => h('p', null, w))))
      : null,
    h('div', { class: 'columns' },
      h('div', { class: 'column' }, needsYou(view, open), tiles(open, pulls), mainBranch(s), openPulls(view, pulls), phases(s)),
      h('div', { class: 'column' }, latestVisuals(view), activity(view))),
    footer(view));
}

function summary(s: Snapshot, open: Issue[], pulls: Pull[]): string {
  const busy = open.filter((i) => i.status === 'claimed' || i.status === 'in-review').length;
  const parts = [plural(pulls.length, 'open PR'), `${busy} ${busy === 1 ? 'issue' : 'issues'} in progress`];
  const main = s.main ? MAIN_STATE[s.main.ci.state] : null;
  if (main) parts.push(main);
  return parts.join(' · ');
}

/** Decisions waiting on Chippy, then anything stuck. */
function needsYou(view: ViewContext, open: Issue[]): HTMLElement {
  const decisions = open.filter((i) => i.status === 'needs-decision');
  const stuck = open.filter((i) => i.status === 'blocked');
  if (decisions.length === 0 && stuck.length === 0) {
    return h('section', { class: 'card calm' },
      icon('done', 'icon calm-icon'),
      h('div', null, h('h2', null, 'Nothing needs you'), h('p', { class: 'calm-text' }, 'No decisions waiting and nothing stuck.')));
  }
  return section('Needs you', { count: decisions.length + stuck.length, cls: 'needs' },
    decisions.length ? h('ul', { class: 'items' }, decisions.map((i) => issueRow(view, i, { status: false }))) : null,
    stuck.length ? [h('h3', { class: 'subhead' }, 'Stuck'), h('ul', { class: 'items' }, stuck.map((i) => issueRow(view, i, { status: false })))] : null);
}

function tiles(open: Issue[], pulls: Pull[]): HTMLElement {
  const failing = pulls.filter((p) => p.ci?.state === 'failure').length;
  return h('nav', { class: 'tiles', 'aria-label': 'Open issues by status' },
    TILES.map((status) => tile(`#issues-${status}`, STATUS_SHORT[status], open.filter((i) => i.status === status).length,
      STATUS_TONES[status], STATUS_ICONS[status])),
    tile('#prs', 'Open PRs', pulls.length, failing ? 'bad' : 'ok', 'pr', failing ? `${failing} failing` : null));
}

function tile(href: string, label: string, value: number, tone: string, iconName: string, note: string | null = null): HTMLElement {
  return h('a', { class: `tile tone-${tone}${value === 0 ? ' zero' : ''}`, href },
    h('span', { class: 'tile-label' }, icon(iconName), label),
    h('span', { class: 'tile-value' }, String(value)),
    note ? h('span', { class: 'tile-note' }, note) : null);
}

function mainBranch(s: Snapshot): HTMLElement | null {
  const main = s.main;
  if (!main) return null;
  return section('Main branch', {},
    h('div', { class: 'item' },
      h('div', { class: 'item-head' }, h('span', { class: 'num sha' }, main.sha.slice(0, 7)), time(main.date)),
      link(main.url, { class: 'item-title' }, main.message),
      h('div', { class: 'item-foot' }, ciDetails(main.ci, 'ci-main'))));
}

function openPulls(view: ViewContext, pulls: Pull[]): HTMLElement {
  return section('Open PRs', { count: pulls.length, more: { href: '#prs', text: 'All PRs' } },
    pulls.length
      ? h('ul', { class: 'items' }, pulls.map((p) => pullRow(view, p, { compact: true })))
      : empty('No PRs open right now.', 'pr'));
}

function phases(s: Snapshot): HTMLElement | null {
  if (s.milestones.length === 0) return null;
  return section('Phases', {},
    h('ul', { class: 'phases' }, s.milestones.map((m) => {
      const total = m.open + m.closed;
      const fill = h('span', { class: 'meter-fill' });
      // A custom property rather than a style attribute, which the page's CSP refuses.
      fill.style.setProperty('--share', String(total ? m.closed / total : 0));
      return h('li', { class: `phase${m.state === 'closed' ? ' complete' : ''}` },
        link(m.url, { class: 'phase-link' },
          h('span', { class: 'phase-top' },
            h('span', { class: 'phase-title' }, m.title),
            h('span', { class: 'phase-count' }, total ? `${m.closed} of ${total} done` : 'No issues yet')),
          h('span', { class: 'meter', role: 'img', 'aria-label': `${m.closed} of ${total} issues done` }, fill),
          m.description ? h('span', { class: 'phase-desc' }, m.description) : null));
    })));
}

function latestVisuals(view: ViewContext): HTMLElement {
  const posts = view.snapshot.visuals.slice(0, 8);
  return section('Latest visuals', { more: posts.length ? { href: '#visuals', text: 'All visuals' } : null },
    posts.length
      ? h('ul', { class: 'strip' }, posts.map((post) => h('li', null,
        h('button', { type: 'button', class: 'strip-card', onclick: () => view.openVisuals(post, 0) },
          frame(post.items[0]!),
          h('span', { class: 'strip-text' },
            pill(VERDICT_NAMES[post.verdict], VERDICT_TONES[post.verdict], VERDICT_ICONS[post.verdict]),
            h('span', { class: 'strip-title' }, post.title),
            h('span', { class: 'strip-meta' }, `#${post.number} · ${plural(post.items.length, 'capture')} · `, time(post.at)))))))
      : empty('No captures yet. Agents post them with the attach-visuals skill.', 'visuals'));
}

function activity(view: ViewContext): HTMLElement {
  const events = view.snapshot.activity;
  return section('Activity', {},
    events.length ? activityList(view, events.slice(0, shown)) : empty('Nothing has happened yet.', 'comment'),
    events.length > shown
      ? h('button', { type: 'button', class: 'button quiet wide', onclick: () => { shown += PAGE; view.redraw(); } }, 'Show more')
      : null);
}

function footer(view: ViewContext): HTMLElement {
  const repo = view.snapshot.repo;
  const how = TOKEN_FROM[view.source ?? ''] ?? '';
  return h('footer', { class: 'page-foot' },
    h('p', null, 'Reading ', link(`https://github.com/${repo}`, {}, repo), `${how}.`),
    view.source === 'cookie'
      ? h('button', { type: 'button', class: 'button quiet', onclick: () => view.disconnect() }, 'Disconnect this browser')
      : null);
}
