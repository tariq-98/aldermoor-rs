// Times, counts and the words for each state, as the page shows them.

import type { CiState, Pull, Status, Verdict } from '../shared/types.ts';

export function ago(iso: string, now = Date.now()): string {
  const seconds = Math.max(0, Math.round((now - Date.parse(iso)) / 1000));
  if (seconds < 45) return 'just now';
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days} d ago`;
  return new Date(iso).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
}

export function clock(iso: string): string {
  return new Date(iso).toLocaleString('en-GB', { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
}

/** The heading for a day in the activity feed, in local time. */
export function dayLabel(iso: string, now = new Date()): string {
  const date = new Date(iso);
  const start = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((start(now) - start(date)) / 86_400_000);
  if (days === 0) return 'Today';
  if (days === 1) return 'Yesterday';
  return date.toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'long' });
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

export const STATUS_NAMES: Record<Status, string> = {
  'needs-decision': 'Needs you',
  blocked: 'Blocked',
  'in-review': 'In review',
  claimed: 'Claimed',
  ready: 'Ready',
  untriaged: 'No status',
  done: 'Done',
};

export const STATUS_SHORT: Record<Status, string> = { ...STATUS_NAMES, 'needs-decision': 'Decisions' };

export const CI_NAMES: Record<CiState, string> = {
  success: 'Passing',
  failure: 'Failing',
  running: 'Running',
  cancelled: 'Cancelled',
  none: 'No CI yet',
};

export const CI_ICONS: Record<CiState, string> = {
  success: 'check',
  failure: 'cross',
  running: 'running',
  cancelled: 'dash',
  none: 'dash',
};

export const VERDICT_NAMES: Record<Verdict, string> = { pass: 'Pass', fail: 'Fail', info: 'Captures' };

export function pullState(pull: Pull): { name: string; tone: string } {
  if (pull.state === 'merged') return { name: 'Merged', tone: 'done' };
  if (pull.state === 'closed') return { name: 'Closed', tone: 'bad' };
  return pull.draft ? { name: 'Draft', tone: 'muted' } : { name: 'Open', tone: 'ok' };
}

/** Labels worth a chip: status and phase are shown in other ways. */
export function chipLabels(names: string[]): string[] {
  return names.filter((n) => !n.startsWith('status:') && !n.startsWith('phase:'));
}
