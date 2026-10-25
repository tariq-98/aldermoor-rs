// CI state for a commit, from its GitHub Actions runs. Every check in this repo is a workflow, so
// the Actions API (which a fine-grained token can read) covers what the checks API would.

import type { CiRun, CiState, CiSummary } from '../shared/types.ts';
import type { RawRun } from './raw.ts';

/** Workflows that report but never block a merge: perf.yml warns instead of failing, and the
 * Tracking and Wiki workflows do bookkeeping. */
export const INFORMATIONAL = new Set(['Perf', 'Tracking', 'Wiki']);

export function runState(run: Pick<RawRun, 'status' | 'conclusion'>): CiState {
  if (run.status !== 'completed') return 'running';
  switch (run.conclusion) {
    case 'success':
    case 'skipped':
    case 'neutral':
      return 'success';
    case 'cancelled':
    case 'stale':
      return 'cancelled';
    case 'action_required':
      return 'running';
    default:
      return 'failure';
  }
}

/** Overall state: any failing blocking workflow fails it, then anything still running, then
 * success. Only cancelled runs (superseded by a newer push) or none at all report as such. A
 * commit that ran only informational workflows reports theirs. */
export function overall(runs: CiRun[]): CiState {
  const counted = runs.some((run) => !run.informational) ? runs.filter((run) => !run.informational) : runs;
  for (const state of ['failure', 'running', 'success', 'cancelled'] as const) {
    if (counted.some((run) => run.state === state)) return state;
  }
  return 'none';
}

/** The latest attempt of each workflow that ran on `sha`. */
export function summarise(runs: RawRun[], sha: string): CiSummary {
  const latest = new Map<number, RawRun>();
  for (const run of runs) {
    if (run.head_sha !== sha) continue;
    const seen = latest.get(run.workflow_id);
    const newer = !seen || run.run_number > seen.run_number ||
      (run.run_number === seen.run_number && (run.run_attempt ?? 1) > (seen.run_attempt ?? 1));
    if (newer) latest.set(run.workflow_id, run);
  }
  const out: CiRun[] = [...latest.values()]
    .map((run) => ({
      name: run.name ?? 'Workflow',
      state: runState(run),
      url: run.html_url,
      informational: INFORMATIONAL.has(run.name ?? ''),
    }))
    .sort((a, b) => Number(a.informational) - Number(b.informational) || a.name.localeCompare(b.name));
  return { state: overall(out), runs: out };
}
