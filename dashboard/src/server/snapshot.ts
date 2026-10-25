// Builds the dashboard's snapshot from GitHub: issues and PRs, comments (for claims and visuals),
// milestones, main's latest commits and the Actions runs on main and on each open PR.

import type { Activity, Issue, Label, Milestone, Pull, Snapshot, Status, VisualPost } from '../shared/types.ts';
import { claimAgent, claimEvent, currentClaim } from './claims.ts';
import { summarise } from './ci.ts';
import { GitHub, GitHubError, inBatches } from './github.ts';
import {
  commentNumber,
  type RawComment,
  type RawCommit,
  type RawIssue,
  type RawLabel,
  type RawMilestone,
  type RawPull,
  type RawRun,
} from './raw.ts';
import { plainText, visualPost } from './visuals.ts';

const STATUS_LABELS: [string, Status][] = [
  ['status:needs-decision', 'needs-decision'],
  ['status:blocked', 'blocked'],
  ['status:in-review', 'in-review'],
  ['status:claimed', 'claimed'],
  ['status:ready', 'ready'],
];

/** The rule the Tracking workflow uses to link a PR to the issues it closes. */
const CLOSES = /\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s+#(\d+)/gi;

const CLOSED_ISSUES_KEPT = 200;
const CLOSED_PULLS_KEPT = 60;
const ACTIVITY_KEPT = 120;

function labels(raw: (RawLabel | string)[]): Label[] {
  return raw.map((l) => (typeof l === 'string' ? { name: l, color: 'cccccc' } : { name: l.name, color: l.color }));
}

export function statusOf(issue: Pick<RawIssue, 'state' | 'labels'>): Status {
  if (issue.state === 'closed') return 'done';
  const names = new Set(labels(issue.labels).map((l) => l.name));
  return STATUS_LABELS.find(([name]) => names.has(name))?.[1] ?? 'untriaged';
}

export function closes(body: string | null | undefined): number[] {
  return [...new Set([...(body ?? '').matchAll(CLOSES)].map((m) => Number(m[1])))];
}

/** A comment's first line as plain text, for the activity feed. */
export function gist(body: string): string {
  const line = body.replace(/<!--[\s\S]*?-->/g, '').split('\n').find((l) => l.trim()) ?? '';
  const plain = plainText(line);
  return plain.length > 140 ? `${plain.slice(0, 139)}…` : plain;
}

function headline(message: string): string {
  return message.split('\n', 1)[0] ?? '';
}

async function runsFor(gh: GitHub, sha: string): Promise<RawRun[]> {
  const page = await gh.get<{ workflow_runs: RawRun[] }>(`repo:/actions/runs?head_sha=${sha}&per_page=50`);
  return page.data.workflow_runs;
}

export async function buildSnapshot(gh: GitHub): Promise<Snapshot> {
  const warnings: string[] = [];
  const [everything, openPulls, comments, rawMilestones, commits] = await Promise.all([
    gh.all<RawIssue>('repo:/issues?state=all&sort=created&direction=asc', 20),
    gh.get<RawPull[]>('repo:/pulls?state=open&per_page=100').then((p) => p.data),
    gh.all<RawComment>('repo:/issues/comments?sort=created&direction=asc', 50),
    gh.get<RawMilestone[]>('repo:/milestones?state=all&per_page=100').then((p) => p.data),
    gh.get<RawCommit[]>('repo:/commits?per_page=30').then(
      (p) => p.data,
      (error: unknown) => {
        if (error instanceof GitHubError && error.status === 409) return []; // An empty repository.
        throw error;
      },
    ),
  ]);
  const rawIssues = everything.filter((i) => !i.pull_request);
  const rawPulls = everything.filter((i) => i.pull_request);
  const heads = new Map(openPulls.map((p) => [p.number, p]));

  const byNumber = new Map<number, RawComment[]>();
  for (const comment of comments) {
    const n = commentNumber(comment);
    byNumber.set(n, [...(byNumber.get(n) ?? []), comment]);
  }

  // CI for main's latest commit and every open PR's head. A token without Actions access gets a
  // warning rather than a broken page.
  const shas = [...new Set([commits[0]?.sha, ...openPulls.map((p) => p.head.sha)].filter((s): s is string => !!s))];
  const runs = new Map<string, RawRun[]>();
  try {
    const found = await inBatches(shas.map((sha) => () => runsFor(gh, sha)), 6);
    shas.forEach((sha, i) => runs.set(sha, found[i]!));
  } catch (error) {
    if (!(error instanceof GitHubError) || (error.status !== 403 && error.status !== 404)) throw error;
    warnings.push('CI results need a token that can read Actions.');
  }
  const ci = (sha: string) => (runs.has(sha) ? summarise(runs.get(sha)!, sha) : null);

  const claims = new Map(rawIssues.map((i) => [i.number, i.state === 'open' ? currentClaim(byNumber.get(i.number) ?? []) : null]));

  // Open and merged PRs count as an issue's PRs; one closed unmerged gave up on it.
  const pullsFor = new Map<number, number[]>();
  for (const pull of rawPulls) {
    if (pull.state === 'closed' && !pull.pull_request?.merged_at) continue;
    for (const n of closes(pull.body)) pullsFor.set(n, [...(pullsFor.get(n) ?? []), pull.number]);
  }

  const issues: Issue[] = rawIssues
    .map((i) => ({
      number: i.number,
      title: i.title,
      url: i.html_url,
      state: i.state,
      createdAt: i.created_at,
      updatedAt: i.updated_at,
      closedAt: i.closed_at,
      milestone: i.milestone?.number ?? null,
      status: statusOf(i),
      labels: labels(i.labels),
      claim: claims.get(i.number) ?? null,
      pulls: pullsFor.get(i.number) ?? [],
    }))
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));

  const pulls: Pull[] = rawPulls
    .map((p) => {
      const closed = closes(p.body);
      const head = heads.get(p.number);
      const branch = head?.head.ref ?? '';
      const claim = closed.map((n) => claims.get(n)).find((c) => c && !c.agent.startsWith('PR #'));
      const merged = p.pull_request?.merged_at ?? null;
      return {
        number: p.number,
        title: p.title,
        url: p.html_url,
        state: merged ? ('merged' as const) : p.state,
        draft: !!(head?.draft ?? p.draft),
        createdAt: p.created_at,
        updatedAt: p.updated_at,
        closedAt: merged ?? p.closed_at,
        branch,
        headSha: head?.head.sha ?? '',
        closes: closed,
        labels: labels(p.labels),
        ci: head ? ci(head.head.sha) : null,
        agent: branch.startsWith('codex/') ? 'codex' : claim?.agent ?? null,
      };
    })
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));

  const pullByNumber = new Map(pulls.map((p) => [p.number, p]));
  const visuals: VisualPost[] = comments
    .flatMap((comment) => {
      const number = commentNumber(comment);
      const pull = pullByNumber.get(number);
      const post = visualPost(comment, {
        repo: gh.repo,
        number,
        kind: pull ? 'pr' : 'issue',
        headSha: pull?.state === 'open' ? pull.headSha || null : null,
      });
      return post ? [post] : [];
    })
    .sort((a, b) => b.at.localeCompare(a.at));

  const milestones: Milestone[] = rawMilestones
    .map((m) => ({
      number: m.number,
      title: m.title,
      description: m.description ?? '',
      state: m.state,
      open: m.open_issues,
      closed: m.closed_issues,
      url: m.html_url,
    }))
    .sort((a, b) => a.title.localeCompare(b.title, 'en', { numeric: true }));

  const activity = feed(everything, pulls, comments, visuals, commits);
  const head = commits[0];
  return {
    repo: gh.repo,
    generatedAt: new Date().toISOString(),
    main: head
      ? {
          sha: head.sha,
          message: headline(head.commit.message),
          url: head.html_url,
          date: head.commit.committer?.date ?? head.commit.author?.date ?? '',
          ci: ci(head.sha) ?? { state: 'none', runs: [] },
        }
      : null,
    milestones,
    issues: [...issues.filter((i) => i.state === 'open'), ...issues.filter((i) => i.state === 'closed').slice(0, CLOSED_ISSUES_KEPT)],
    pulls: [...pulls.filter((p) => p.state === 'open'), ...pulls.filter((p) => p.state !== 'open').slice(0, CLOSED_PULLS_KEPT)],
    visuals,
    activity,
    warnings,
  };
}

/** What happened, newest first: issues and PRs opening and closing, claims, visuals, other
 * comments, and pushes straight to main (a PR's squash merge is already there as the merge). */
function feed(everything: RawIssue[], pulls: Pull[], comments: RawComment[], visuals: VisualPost[], commits: RawCommit[]): Activity[] {
  const out: Activity[] = [];
  const titles = new Map(everything.map((i) => [i.number, i.title]));
  for (const p of pulls) {
    out.push({ key: `pr-${p.number}-open`, at: p.createdAt, kind: 'pr-opened', number: p.number, subject: p.title, text: p.draft ? 'Draft PR opened' : 'PR opened', url: p.url });
    if (p.state === 'merged') out.push({ key: `pr-${p.number}-merged`, at: p.closedAt!, kind: 'pr-merged', number: p.number, subject: p.title, text: 'Merged', url: p.url });
    if (p.state === 'closed' && p.closedAt) out.push({ key: `pr-${p.number}-closed`, at: p.closedAt, kind: 'pr-closed', number: p.number, subject: p.title, text: 'Closed without merging', url: p.url });
  }
  for (const i of everything) {
    if (i.pull_request) continue;
    out.push({ key: `issue-${i.number}-open`, at: i.created_at, kind: 'issue-opened', number: i.number, subject: i.title, text: 'Issue opened', url: i.html_url });
    if (i.state === 'closed' && i.closed_at) {
      const text = i.state_reason === 'not_planned' ? 'Closed as not planned' : 'Done';
      out.push({ key: `issue-${i.number}-closed`, at: i.closed_at, kind: 'issue-closed', number: i.number, subject: i.title, text, url: i.html_url });
    }
  }
  const posts = new Map(visuals.map((v) => [v.id, v]));
  for (const c of comments.slice(-ACTIVITY_KEPT)) {
    const number = commentNumber(c);
    const base = { key: `comment-${c.id}`, at: c.created_at, number, subject: titles.get(number) ?? `#${number}`, url: c.html_url };
    const post = posts.get(c.id);
    const event = claimEvent(c.body);
    if (post) {
      const count = `${post.items.length} ${post.items.length === 1 ? 'capture' : 'captures'}`;
      out.push({ ...base, kind: 'visuals', text: `${post.title} · ${count}`, verdict: post.verdict });
    } else if (event === 'claim') {
      out.push({ ...base, kind: 'claim', text: `Claimed by ${claimAgent(c.body ?? '') ?? 'an agent'}` });
    } else if (event === 'release') {
      out.push({ ...base, kind: 'release', text: gist(c.body ?? '') || 'Released' });
    } else {
      out.push({ ...base, kind: 'comment', text: gist(c.body ?? '') || 'Commented' });
    }
  }
  const merged = new Set(pulls.filter((p) => p.state === 'merged').map((p) => p.number));
  for (const c of commits) {
    const title = headline(c.commit.message);
    if (merged.has(Number(title.match(/\(#(\d+)\)$/)?.[1]))) continue;
    const at = c.commit.committer?.date ?? c.commit.author?.date ?? '';
    out.push({ key: `push-${c.sha}`, at, kind: 'push', number: null, subject: title, text: `Pushed to main · ${c.sha.slice(0, 7)}`, url: c.html_url });
  }
  return out.sort((a, b) => b.at.localeCompare(a.at)).slice(0, ACTIVITY_KEPT);
}
