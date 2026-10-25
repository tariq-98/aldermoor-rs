// The snapshot /api/state sends to the page: everything the dashboard shows, already joined up,
// so a phone downloads one small JSON document instead of talking to GitHub itself.

/** An issue's place on the board, from its `status:*` label (AGENTS.md, Labels). */
export type Status = 'needs-decision' | 'blocked' | 'in-review' | 'claimed' | 'ready' | 'untriaged' | 'done';

/** One workflow's result, or a commit's overall result across its workflows. */
export type CiState = 'success' | 'failure' | 'running' | 'cancelled' | 'none';

export type Verdict = 'pass' | 'fail' | 'info';

export interface Label {
  name: string;
  color: string;
}

export interface Snapshot {
  repo: string;
  generatedAt: string;
  main: MainBranch | null;
  milestones: Milestone[];
  issues: Issue[];
  pulls: Pull[];
  visuals: VisualPost[];
  activity: Activity[];
  /** Sections the token couldn't read, said plainly, e.g. that CI needs Actions access. */
  warnings: string[];
}

export interface MainBranch {
  sha: string;
  message: string;
  url: string;
  date: string;
  ci: CiSummary;
}

export interface Milestone {
  number: number;
  title: string;
  description: string;
  state: 'open' | 'closed';
  open: number;
  closed: number;
  url: string;
}

export interface Claim {
  /** As written after "Claimed by": `thread: <title>`, `local: <checkout>`, `codex`, `PR #49`. */
  agent: string;
  at: string;
  url: string;
  plan?: string;
}

export interface Issue {
  number: number;
  title: string;
  url: string;
  state: 'open' | 'closed';
  createdAt: string;
  updatedAt: string;
  closedAt: string | null;
  milestone: number | null;
  status: Status;
  labels: Label[];
  claim: Claim | null;
  /** Open or merged PRs that say they close this issue. */
  pulls: number[];
}

export interface CiRun {
  name: string;
  state: CiState;
  url: string;
  /** Workflows that never block a merge (Perf, Tracking, Wiki) don't count toward the overall state. */
  informational: boolean;
}

export interface CiSummary {
  state: CiState;
  runs: CiRun[];
}

export interface Pull {
  number: number;
  title: string;
  url: string;
  state: 'open' | 'merged' | 'closed';
  draft: boolean;
  createdAt: string;
  updatedAt: string;
  closedAt: string | null;
  branch: string;
  headSha: string;
  closes: number[];
  labels: Label[];
  ci: CiSummary | null;
  /** The claim on the first issue it closes, which says which agent is behind the PR. */
  agent: string | null;
}

export interface VisualItem {
  type: 'image' | 'video';
  caption: string;
  /** Same-origin URL served by /api/media. */
  src: string;
  thumb: string | null;
  /** The file on GitHub. */
  github: string;
}

export interface VisualPost {
  id: number;
  url: string;
  at: string;
  number: number;
  kind: 'pr' | 'issue';
  title: string;
  verdict: Verdict;
  head: string | null;
  base: string | null;
  by: string | null;
  /** For a PR still open: whether `head` is the PR's latest commit. Null when it can't be told. */
  current: boolean | null;
  /** Posted before the attach-visuals format, read from the comment's image links instead. */
  legacy: boolean;
  items: VisualItem[];
}

export type ActivityKind =
  | 'pr-opened'
  | 'pr-merged'
  | 'pr-closed'
  | 'issue-opened'
  | 'issue-closed'
  | 'claim'
  | 'release'
  | 'visuals'
  | 'comment'
  | 'push';

export interface Activity {
  key: string;
  at: string;
  kind: ActivityKind;
  /** Issue or PR number, when there is one. */
  number: number | null;
  /** The issue's, PR's or commit's title. */
  subject: string;
  /** What happened, in a few words: "Claimed by codex", "Mac check: PASS". */
  text: string;
  url: string;
  verdict?: Verdict;
}
