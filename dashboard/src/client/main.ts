// Aldermoor Live. Polls /api/state, which answers an empty 304 when nothing changed, and redraws
// the open tab when GitHub has. Tabs and filters are links (#prs-merged, #issues-claimed,
// #visuals-59), so the back button and shared links work.

import type { Snapshot, VisualPost } from '../shared/types.ts';
import { connectScreen } from './connect.ts';
import { h, icon } from './dom.ts';
import { ago } from './format.ts';
import { openLightbox } from './lightbox.ts';
import { FRESH_MS, tickTimes } from './parts.ts';
import { parseRoute, type Route, type Tab, type ViewContext } from './view.ts';
import { issuesView } from './views/issues.ts';
import { overviewView } from './views/overview.ts';
import { pullsView } from './views/pulls.ts';
import { visualsView } from './views/visuals.ts';

const POLL_MS = 15_000;
const SLOWEST_MS = 120_000;
const SEEN_KEY = 'aldermoor-live:visuals-seen';
/** What each optional token permission is for (api/session.ts checks them). */
const NEEDED_FOR: Record<string, string> = { Actions: 'CI results', Contents: "main's commits and the captures" };

const VIEWS: Record<Tab, (view: ViewContext) => HTMLElement> = {
  overview: overviewView,
  prs: pullsView,
  issues: issuesView,
  visuals: visualsView,
};
const TAB_ICONS: Record<Tab, string> = { overview: 'overview', prs: 'pr', issues: 'issue', visuals: 'visuals' };

function byId<T extends HTMLElement = HTMLElement>(id: string): T {
  const el = document.getElementById(id);
  if (!el) throw new Error(`#${id} is missing from the page`);
  return el as T;
}

const root = byId('view');
const live = byId<HTMLButtonElement>('live');
const liveText = byId('live-text');
const notice = byId('notice');
const tabs = [...document.querySelectorAll<HTMLAnchorElement>('a[data-tab]')];

let snapshot: Snapshot | null = null;
let etag: string | null = null;
let source: string | null = null;
let route: Route = parseRoute(location.hash);
let drawn = '';
let timer = 0;
let polling = false;
let connecting = false;
let failures = 0;
let lastOk = 0;
let lastTry = 0;
let problem: string | null = null;
let note: string | null = null;
let seen = readSeen();
const fresh = new Map<string, number>();

function readSeen(): number | null {
  try {
    const value = localStorage.getItem(SEEN_KEY);
    return value === null ? null : Number(value);
  } catch {
    return null;
  }
}

function writeSeen(id: number): void {
  seen = id;
  try {
    localStorage.setItem(SEEN_KEY, String(id));
  } catch {
    // Private browsing: the page remembers until it closes.
  }
}

// --- Polling -------------------------------------------------------------------------------

async function poll(): Promise<void> {
  window.clearTimeout(timer);
  if (polling || connecting) return;
  polling = true;
  lastTry = Date.now();
  try {
    const res = await fetch('/api/state', { cache: 'no-store', headers: etag ? { 'If-None-Match': etag } : {} });
    if (res.status === 304) {
      succeeded();
    } else if (res.ok) {
      const next = (await res.json()) as Snapshot;
      etag = res.headers.get('ETag');
      source = res.headers.get('X-Token-Source');
      succeeded();
      apply(next);
    } else {
      const body = (await res.json().catch(() => null)) as { error?: string; message?: string } | null;
      if (res.status === 401 && body?.error) void showConnect(body.error === 'connect' ? null : body.message ?? null);
      else failed(body?.message ?? `The dashboard answered ${res.status}. Reload the page if this keeps up.`);
    }
  } catch {
    failed(navigator.onLine ? "Can't reach the dashboard. Reload the page if this keeps up." : "You're offline.");
  } finally {
    polling = false;
    schedule();
  }
}

/** The next poll: every 15 s while the page is in view, backing off to 2 min while failing. */
function schedule(): void {
  window.clearTimeout(timer);
  if (connecting || document.hidden) return;
  const wait = failures ? Math.min(SLOWEST_MS, POLL_MS * 2 ** failures) : POLL_MS;
  timer = window.setTimeout(() => void poll(), wait);
}

function succeeded(): void {
  failures = 0;
  lastOk = Date.now();
  problem = null;
  updateLive();
  updateNotice();
}

function failed(message: string): void {
  failures += 1;
  problem = message;
  updateLive();
  updateNotice();
  if (!snapshot) root.replaceChildren(errorScreen(message));
}

function apply(next: Snapshot): void {
  if (snapshot) {
    const known = new Set(snapshot.activity.map((a) => a.key));
    const now = Date.now();
    for (const event of next.activity) if (!known.has(event.key)) fresh.set(event.key, now);
  }
  snapshot = next;
  render();
}

// --- Drawing -------------------------------------------------------------------------------

function render(): void {
  if (!snapshot || connecting) return;
  const key = `${route.tab}:${route.arg ?? ''}`;
  const sameRoute = key === drawn;
  const y = window.scrollY;
  const open = new Set([...root.querySelectorAll<HTMLDetailsElement>('details[open][data-key]')].map((d) => d.dataset.key));
  const now = Date.now();
  for (const [k, at] of fresh) if (now - at > FRESH_MS) fresh.delete(k);
  const page = VIEWS[route.tab]({ snapshot, route, fresh, source, openVisuals, disconnect: () => void disconnect(), redraw: render });
  for (const details of page.querySelectorAll<HTMLDetailsElement>('details[data-key]')) details.open = open.has(details.dataset.key);
  root.replaceChildren(page);
  root.removeAttribute('aria-busy');
  drawn = key;
  updateTabs();
  window.scrollTo(0, sameRoute ? y : 0);
}

function openVisuals(post: VisualPost, index: number): void {
  openLightbox(post, index);
}

function badge(tab: Tab, text: string | null, tone: string): void {
  const el = tabs.find((t) => t.dataset.tab === tab)?.querySelector<HTMLElement>('.badge');
  if (!el) return;
  el.hidden = !text;
  el.textContent = text ?? '';
  el.className = `badge tone-${tone}`;
}

function updateTabs(): void {
  for (const tab of tabs) {
    if (tab.dataset.tab === route.tab) tab.setAttribute('aria-current', 'page');
    else tab.removeAttribute('aria-current');
  }
  if (!snapshot) return;
  const decisions = snapshot.issues.filter((i) => i.state === 'open' && i.status === 'needs-decision').length;
  const open = snapshot.pulls.filter((p) => p.state === 'open');
  const failing = open.some((p) => p.ci?.state === 'failure');
  const newest = Math.max(0, ...snapshot.visuals.map((v) => v.id));
  // The first visit starts from what's there now; opening Visuals marks everything seen.
  if (seen === null || route.tab === 'visuals') writeSeen(newest);
  const unseen = snapshot.visuals.filter((v) => v.id > (seen ?? newest)).length;
  badge('overview', decisions ? String(decisions) : null, 'decide');
  badge('prs', open.length ? String(open.length) : null, failing ? 'bad' : 'neutral');
  badge('visuals', unseen ? String(unseen) : null, 'info');
  document.title = decisions ? `(${decisions}) Aldermoor Live` : 'Aldermoor Live';
}

function updateLive(): void {
  let state = 'live';
  let text = 'Live';
  if (connecting) [state, text] = ['off', 'Not connected'];
  else if (!navigator.onLine) [state, text] = ['off', 'Offline'];
  else if (failures) [state, text] = ['stale', 'Retrying'];
  else if (!lastOk) [state, text] = ['wait', 'Connecting'];
  else if (Date.now() - lastOk > POLL_MS * 3) [state, text] = ['stale', `Updated ${ago(new Date(lastOk).toISOString())}`];
  live.dataset.state = state;
  liveText.textContent = text;
  live.title = lastOk ? `Checked GitHub ${ago(new Date(lastOk).toISOString())}. Tap to check now.` : 'Tap to check now.';
}

/** The strip under the header: why updates have stopped, or what a new token can't read. */
function updateNotice(): void {
  const text = snapshot && problem ? problem : note;
  notice.hidden = !text;
  if (!text) return notice.replaceChildren();
  const dismiss = text === note
    ? h('button', { type: 'button', class: 'notice-close', 'aria-label': 'Dismiss', onclick: () => { note = null; updateNotice(); } }, icon('close'))
    : null;
  notice.replaceChildren(h('div', { class: 'notice-inner' }, icon('warning'), h('span', null, text), dismiss));
}

function loadingScreen(): HTMLElement {
  return h('div', { class: 'page loading', 'aria-hidden': 'true' },
    h('div', { class: 'page-head' }, h('div', { class: 'skeleton skeleton-title' })),
    [0, 1, 2].map(() => h('div', { class: 'card' }, h('div', { class: 'skeleton' }), h('div', { class: 'skeleton short' }))));
}

function errorScreen(message: string): HTMLElement {
  return h('div', { class: 'page' },
    h('section', { class: 'card problem' },
      icon('warning', 'icon problem-icon'),
      h('h1', null, "Can't load the dashboard"),
      h('p', null, message),
      h('button', { type: 'button', class: 'button primary', onclick: () => void poll() }, 'Try again')));
}

// --- Connecting ----------------------------------------------------------------------------

async function showConnect(message: string | null): Promise<void> {
  connecting = true;
  window.clearTimeout(timer);
  snapshot = null;
  etag = null;
  drawn = '';
  document.body.dataset.mode = 'connect';
  updateLive();
  updateNotice();
  let repo = 'tariq-98/aldermoor-rs';
  try {
    const res = await fetch('/api/session', { cache: 'no-store' });
    repo = ((await res.json()) as { repo?: string }).repo ?? repo;
  } catch {
    // Keep the default.
  }
  root.replaceChildren(connectScreen({ repo, message, onConnected }));
  root.removeAttribute('aria-busy');
  window.scrollTo(0, 0);
}

function onConnected(missing: string[]): void {
  connecting = false;
  delete document.body.dataset.mode;
  const lost = missing.map((permission) => NEEDED_FOR[permission] ?? permission);
  note = missing.length
    ? `Connected. The token can't read ${missing.join(' or ')}, so ${lost.join(' and ')} won't show until you add that access on GitHub.`
    : null;
  root.replaceChildren(loadingScreen());
  updateLive();
  void poll();
}

async function disconnect(): Promise<void> {
  try {
    await fetch('/api/session', { method: 'DELETE' });
  } catch {
    // The cookie stays until the next try; the screen still asks for a token.
  }
  await showConnect(null);
}

// --- Start ---------------------------------------------------------------------------------

for (const tab of tabs) tab.querySelector('.tab-icon')?.prepend(icon(TAB_ICONS[tab.dataset.tab as Tab] ?? 'dash'));

window.addEventListener('hashchange', () => {
  route = parseRoute(location.hash);
  if (snapshot) render();
  else updateTabs();
});

document.addEventListener('visibilitychange', () => {
  if (document.hidden) window.clearTimeout(timer);
  else if (Date.now() - lastTry > 5_000) void poll();
  else schedule();
});
window.addEventListener('online', () => void poll());
window.addEventListener('offline', updateLive);
window.addEventListener('pageshow', (event) => {
  if (event.persisted) void poll();
});
live.addEventListener('click', () => void poll());

let ticks = 0;
window.setInterval(() => {
  updateLive();
  if (++ticks % 6 === 0) tickTimes(root);
}, 5_000);

updateTabs();
void poll();
