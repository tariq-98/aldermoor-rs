// Who has an issue. Every agent posts as the same GitHub user, so the claim lives in comments
// (AGENTS.md, Claim it): "Claimed by <agent>. Plan: ... Branch: ...", "Released ...", and a
// takeover of a stale claim that says it is taking the issue over.

import type { Claim } from '../shared/types.ts';
import type { RawComment } from './raw.ts';

const CLAIM = /^Claimed by\s+(.+?)(?:\.(?=\s|$)|$)/m;
const PLAN = /\bPlan:\s*(.+?)(?:\.\s+Branch:|\s*Branch:|$)/m;
const RELEASE = /^Released\b/;
const TAKEOVER = /\btak(?:e|ing) (?:it |this )?over\b/i;

function firstLine(body: string): string {
  return body.trimStart().split('\n', 1)[0] ?? '';
}

export type ClaimEvent = 'claim' | 'release' | null;

/** Whether a comment claims or releases its issue. Only the first line counts, so a comment that
 * quotes the rules doesn't. */
export function claimEvent(body: string | null | undefined): ClaimEvent {
  const line = firstLine(body ?? '');
  if (/^Claimed by\s/.test(line)) return 'claim';
  if (RELEASE.test(line)) return 'release';
  return null;
}

/** The agent as written in a claim, without a note on the branch or on whose claim it takes over. */
export function claimAgent(body: string): string | null {
  const match = firstLine(body).match(CLAIM);
  if (!match) return null;
  return match[1]!.replace(/\s*\([^()]*\b(?:branch|taking over|takes over|take over)\b[^()]*\)\s*$/i, '').trim();
}

/**
 * The claim that holds now: the earliest claim since the last release, because the earliest
 * unreleased claim wins and a later one backs off. A takeover comment ends the old claim, so the
 * claim after it holds.
 */
export function currentClaim(comments: RawComment[]): Claim | null {
  let claim: Claim | null = null;
  let takeover = false;
  for (const comment of [...comments].sort((a, b) => a.created_at.localeCompare(b.created_at))) {
    const body = comment.body ?? '';
    const event = claimEvent(body);
    if (event === 'release') {
      claim = null;
    } else if (event === 'claim') {
      const agent = claimAgent(body);
      if (agent && (!claim || takeover || TAKEOVER.test(firstLine(body)))) {
        const plan = firstLine(body).match(PLAN)?.[1]?.trim().replace(/\.$/, '');
        claim = { agent, at: comment.created_at, url: comment.html_url, ...(plan ? { plan } : {}) };
      }
      takeover = false;
    } else if (TAKEOVER.test(firstLine(body))) {
      takeover = true;
    }
  }
  return claim;
}
