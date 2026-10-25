// The parts of GitHub's REST responses the dashboard reads.

export interface RawLabel {
  name: string;
  color: string;
}

/** The issues list, which holds PRs too (with `pull_request` set). It is much smaller than the
 * pulls list, so only open PRs are fetched from there, for their head commit. */
export interface RawIssue {
  number: number;
  title: string;
  html_url: string;
  state: 'open' | 'closed';
  state_reason?: string | null;
  created_at: string;
  updated_at: string;
  closed_at: string | null;
  body?: string | null;
  draft?: boolean;
  labels: (RawLabel | string)[];
  milestone: { number: number } | null;
  pull_request?: { merged_at?: string | null };
}

export interface RawPull {
  number: number;
  draft?: boolean;
  head: { ref: string; sha: string };
}

export interface RawComment {
  id: number;
  html_url: string;
  issue_url: string;
  body?: string | null;
  created_at: string;
  updated_at: string;
}

export interface RawMilestone {
  number: number;
  title: string;
  description: string | null;
  state: 'open' | 'closed';
  open_issues: number;
  closed_issues: number;
  html_url: string;
}

export interface RawRun {
  id: number;
  name: string | null;
  workflow_id: number;
  head_sha: string;
  head_branch: string | null;
  event: string;
  status: string | null;
  conclusion: string | null;
  run_number: number;
  run_attempt?: number;
  html_url: string;
  created_at: string;
}

export interface RawCommit {
  sha: string;
  html_url: string;
  commit: { message: string; committer: { date: string } | null; author: { date: string } | null };
}

/** The issue or PR number a comment belongs to, from its issue_url. */
export function commentNumber(comment: RawComment): number {
  return Number(comment.issue_url.slice(comment.issue_url.lastIndexOf('/') + 1));
}
