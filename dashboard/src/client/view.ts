// What every tab is drawn from. A tab is a function of the snapshot and the route, so a new
// snapshot or a new route redraws it from scratch.

import type { Snapshot, VisualPost } from '../shared/types.ts';

export type Tab = 'overview' | 'prs' | 'issues' | 'visuals';

export interface Route {
  tab: Tab;
  /** What the tab is narrowed to, from the hash: #prs-merged, #issues-claimed, #visuals-59. */
  arg: string | null;
}

export interface ViewContext {
  snapshot: Snapshot;
  route: Route;
  /** Activity that arrived while the page was open, by key, with when it arrived. */
  fresh: Map<string, number>;
  /** Where the server's GitHub token comes from: this browser (`cookie`), the project (`env`) or
   * the local dev server (`dev`). */
  source: string | null;
  openVisuals(post: VisualPost, index: number): void;
  disconnect(): void;
  /** Redraws the tab, after something only the page knows changed (a "show more"). */
  redraw(): void;
}

export function parseRoute(hash: string): Route {
  const match = hash.match(/^#(overview|prs|issues|visuals)(?:-([a-z0-9-]+))?$/);
  return match ? { tab: match[1] as Tab, arg: match[2] ?? null } : { tab: 'overview', arg: null };
}
