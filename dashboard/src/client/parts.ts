// Pieces the tabs share: pills, times, CI details, issue and PR rows, capture frames and the
// activity feed.

import type { Activity, ActivityKind, CiState, CiSummary, Issue, Label, Pull, Snapshot, Status, Verdict, VisualItem } from '../shared/types.ts';
import { type Child, h, icon, link } from './dom.ts';
import { ago, CI_ICONS, CI_NAMES, chipLabels, clock, dayLabel, plural, pullState, STATUS_NAMES } from './format.ts';
import type { ViewContext } from './view.ts';

/** How long activity that arrived while the page was open stays marked as new. */
export const FRESH_MS = 120_000;

export const STATUS_TONES: Record<Status, string> = {
  'needs-decision': 'decide',
  blocked: 'bad',
  'in-review': 'info',
  claimed: 'warn',
  ready: 'ok',
  untriaged: 'muted',
  done: 'done',
};

export const STATUS_ICONS: Record<Status, string> = {
  'needs-decision': 'decide',
  blocked: 'blocked',
  'in-review': 'review',
  claimed: 'claim',
  ready: 'issue',
  untriaged: 'untriaged',
  done: 'done',
};

export const CI_TONES: Record<CiState, string> = { success: 'ok', failure: 'bad', running: 'warn', cancelled: 'muted', none: 'muted' };

export const VERDICT_TONES: Record<Verdict, string> = { pass: 'ok', fail: 'bad', info: 'info' };
export const VERDICT_ICONS: Record<Verdict, string> = { pass: 'check', fail: 'cross', info: 'visuals' };

export function pill(text: string, tone: string, iconName: string | null = null, extra = ''): HTMLSpanElement {
  return h('span', { class: `pill tone-${tone}${extra ? ` ${extra}` : ''}` }, iconName ? icon(iconName) : null, text);
}

/** Waiting on Chippy is the one state drawn solid, so it stands out in any list. */
export function statusPill(status: Status): HTMLSpanElement {
  return pill(STATUS_NAMES[status], STATUS_TONES[status], STATUS_ICONS[status], status === 'needs-decision' ? 'solid' : '');
}

export function time(iso: string | null): HTMLTimeElement | null {
  return iso ? h('time', { datetime: iso, title: clock(iso), 'data-ago': '' }, ago(iso)) : null;
}

/** Brings every relative time on the page up to date. */
export function tickTimes(root: ParentNode = document): void {
  const now = Date.now();
  for (const el of root.querySelectorAll<HTMLTimeElement>('time[data-ago]')) el.textContent = ago(el.dateTime, now);
}

export function ref(repo: string, n: number): HTMLAnchorElement {
  return link(`https://github.com/${repo}/issues/${n}`, { class: 'ref' }, `#${n}`);
}

/** The title of issue or PR `n`, as far as the snapshot knows it. */
export function subjectOf(snapshot: Snapshot, n: number): string {
  return snapshot.pulls.find((p) => p.number === n)?.title ?? snapshot.issues.find((i) => i.number === n)?.title ?? '';
}

/** A line of small facts, each led by its icon; nothing when none are there. */
export function meta(...parts: Child[]): HTMLElement | null {
  const present = parts.filter((p) => p !== null && p !== undefined && p !== false && p !== '');
  if (present.length === 0) return null;
  return h('div', { class: 'meta' }, present.map((p) => (p instanceof HTMLElement ? p : h('span', null, p))));
}

export function section(title: string, opts: { count?: number | null; more?: { href: string; text: string } | null; cls?: string },
  ...body: Child[]): HTMLElement {
  return h('section', { class: `card${opts.cls ? ` ${opts.cls}` : ''}` },
    h('header', { class: 'card-head' },
      h('h2', null, title, opts.count !== undefined && opts.count !== null ? h('span', { class: 'count' }, String(opts.count)) : null),
      opts.more ? h('a', { class: 'more', href: opts.more.href }, opts.more.text, icon('next')) : null),
    body);
}

export function empty(text: string, iconName = 'done'): HTMLElement {
  return h('p', { class: 'empty' }, icon(iconName), h('span', null, text));
}

/** CI as a pill that opens to each workflow's result. */
export function ciDetails(ci: CiSummary | null, key: string): HTMLElement {
  if (!ci) return pill('CI unknown', 'muted', 'dash');
  if (ci.runs.length === 0) return pill(CI_NAMES.none, 'muted', CI_ICONS.none);
  return h('details', { class: 'ci', 'data-key': key },
    h('summary', null,
      pill(CI_NAMES[ci.state], CI_TONES[ci.state], CI_ICONS[ci.state]),
      h('span', { class: 'ci-count' }, plural(ci.runs.length, 'check')),
      icon('next', 'icon chevron')),
    h('ul', { class: 'runs' }, ci.runs.map((run) => h('li', null,
      link(run.url, { class: 'run' },
        h('span', { class: `run-icon tone-${CI_TONES[run.state]}` }, icon(CI_ICONS[run.state])),
        h('span', { class: 'run-name' }, run.name),
        run.informational ? h('span', { class: 'run-note' }, 'report only') : null,
        h('span', { class: 'run-state' }, CI_NAMES[run.state]))))));
}

export function labelChips(labels: Label[]): HTMLElement | null {
  const names = chipLabels(labels.map((l) => l.name)).map((n) => n.replace(/^(?:area|type):/, ''));
  return names.length ? h('div', { class: 'labels' }, names.map((n) => h('span', { class: 'label' }, n))) : null;
}

function pullRef(view: ViewContext, n: number): HTMLElement | null {
  const pull = view.snapshot.pulls.find((p) => p.number === n);
  if (!pull) return null;
  const state = pullState(pull);
  return link(pull.url, { class: `pull-ref tone-${state.tone}`, title: `${state.name}: ${pull.title}` },
    icon(pull.state === 'merged' ? 'merged' : 'pr'), `#${n} ${state.name.toLowerCase()}`);
}

export function issueRow(view: ViewContext, issue: Issue, { status = true } = {}): HTMLLIElement {
  return h('li', { class: 'item' },
    h('div', { class: 'item-head' },
      status ? statusPill(issue.status) : null,
      h('span', { class: 'num' }, `#${issue.number}`),
      time(issue.state === 'closed' ? issue.closedAt : issue.updatedAt)),
    link(issue.url, { class: 'item-title' }, issue.title),
    meta(
      issue.claim ? h('span', { class: 'claim', title: issue.claim.plan ?? null }, icon('claim'), issue.claim.agent) : null,
      ...issue.pulls.map((n) => pullRef(view, n))),
    labelChips(issue.labels));
}

/** A PR with its state, CI and captures. `compact` leaves out the branch and labels. */
export function pullRow(view: ViewContext, pull: Pull, { compact = false } = {}): HTMLLIElement {
  const repo = view.snapshot.repo;
  const state = pullState(pull);
  const captures = view.snapshot.visuals.filter((v) => v.number === pull.number).reduce((n, v) => n + v.items.length, 0);
  return h('li', { class: 'item' },
    h('div', { class: 'item-head' },
      pill(state.name, state.tone, pull.state === 'merged' ? 'merged' : 'pr'),
      h('span', { class: 'num' }, `#${pull.number}`),
      time(pull.state === 'open' ? pull.updatedAt : pull.closedAt)),
    link(pull.url, { class: 'item-title' }, pull.title),
    meta(
      pull.agent ? h('span', { class: 'agent' }, icon('agent'), pull.agent) : null,
      pull.branch && !compact ? h('span', { class: 'branch' }, icon('branch'), pull.branch) : null,
      pull.closes.length ? h('span', null, icon('issue'), 'Closes ', pull.closes.map((n, i) => [i ? ', ' : '', ref(repo, n)])) : null),
    pull.state === 'open' || captures
      ? h('div', { class: 'item-foot' },
        pull.state === 'open' ? ciDetails(pull.ci, `ci-${pull.number}`) : null,
        captures ? h('a', { class: 'pill tone-info link-pill', href: `#visuals-${pull.number}` }, icon('visuals'), plural(captures, 'capture')) : null)
      : null,
    compact ? null : labelChips(pull.labels));
}

/** A capture's picture: its thumbnail, or the image itself when it has none. A video without a
 * thumbnail gets a play mark rather than loading the video. */
export function frame(item: VisualItem): HTMLElement {
  const src = item.thumb ?? (item.type === 'image' ? item.src : null);
  return h('span', { class: `frame${item.type === 'video' ? ' video' : ''}` },
    src ? h('img', { src, alt: '', loading: 'lazy', decoding: 'async', onerror: broken }) : null,
    item.type === 'video' ? h('span', { class: 'play' }, icon('play')) : null);
}

function broken(event: Event): void {
  (event.currentTarget as HTMLElement).parentElement?.classList.add('broken');
}

const KINDS: Record<ActivityKind, { icon: string; tone: string }> = {
  'pr-opened': { icon: 'pr', tone: 'ok' },
  'pr-merged': { icon: 'merged', tone: 'done' },
  'pr-closed': { icon: 'pr', tone: 'bad' },
  'issue-opened': { icon: 'issue', tone: 'ok' },
  'issue-closed': { icon: 'done', tone: 'done' },
  claim: { icon: 'claim', tone: 'warn' },
  release: { icon: 'release', tone: 'muted' },
  visuals: { icon: 'visuals', tone: 'info' },
  comment: { icon: 'comment', tone: 'muted' },
  push: { icon: 'push', tone: 'muted' },
};

function activityRow(view: ViewContext, event: Activity): HTMLLIElement {
  const kind = KINDS[event.kind];
  let tone = kind.tone;
  if (event.kind === 'visuals') tone = VERDICT_TONES[event.verdict ?? 'info'];
  if (event.kind === 'issue-closed' && event.text !== 'Done') tone = 'muted';
  const arrived = view.fresh.get(event.key);
  const subject = () => [event.number ? h('span', { class: 'num' }, `#${event.number}`) : null, event.subject];
  // A comment leads with what it is on, then its first words; everything else says what happened.
  const body = event.kind === 'comment'
    ? [h('span', { class: 'event-text' }, subject()), h('span', { class: 'event-subject' }, event.text)]
    : [h('span', { class: 'event-text' }, event.text), h('span', { class: 'event-subject' }, subject())];
  // Captures open here; everything else opens on GitHub.
  const target = event.kind === 'visuals' && event.number
    ? h('a', { class: 'event-body', href: `#visuals-${event.number}` }, body)
    : link(event.url, { class: 'event-body' }, body);
  return h('li', { class: `event${arrived && Date.now() - arrived < FRESH_MS ? ' fresh' : ''}` },
    h('span', { class: `event-icon tone-${tone}` }, icon(kind.icon)),
    target,
    time(event.at));
}

/** The feed, under a heading for each day. */
export function activityList(view: ViewContext, events: Activity[]): HTMLElement {
  const rows: HTMLLIElement[] = [];
  let day = '';
  const now = new Date();
  for (const event of events) {
    const label = dayLabel(event.at, now);
    if (label !== day) {
      day = label;
      rows.push(h('li', { class: 'day' }, label));
    }
    rows.push(activityRow(view, event));
  }
  return h('ol', { class: 'feed' }, rows);
}
