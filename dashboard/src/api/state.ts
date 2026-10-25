// GET /api/state: the snapshot, with an ETag so a poll where nothing changed is an empty 304.

import { createHash } from 'node:crypto';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { GitHub, GitHubError } from '../server/github.ts';
import { REPO, sendJson, send, tokenFor } from '../server/http.ts';
import { buildSnapshot } from '../server/snapshot.ts';
import type { Snapshot } from '../shared/types.ts';

// Several tabs or devices polling at once share one build for a few seconds.
const recent = new Map<string, { at: number; snapshot: Promise<Snapshot> }>();
const SHARE_MS = 4_000;

function snapshotFor(gh: GitHub, key: string): Promise<Snapshot> {
  const hit = recent.get(key);
  if (hit && Date.now() - hit.at < SHARE_MS) return hit.snapshot;
  const snapshot = buildSnapshot(gh);
  recent.set(key, { at: Date.now(), snapshot });
  snapshot.catch(() => recent.delete(key));
  if (recent.size > 20) recent.delete(recent.keys().next().value as string);
  return snapshot;
}

export function problem(error: unknown): { status: number; body: { error: string; message: string } } {
  if (error instanceof GitHubError) {
    if (error.status === 401) {
      return { status: 401, body: { error: 'token', message: 'GitHub turned the token down. It may have expired or been revoked; connect a new one.' } };
    }
    if (error.status === 404 && /\/(issues|pulls)\b/.test(error.path)) {
      return { status: 401, body: { error: 'repo', message: `The token can't see ${REPO}. Give it access to that repository.` } };
    }
    if (error.status === 403 && /rate limit/i.test(error.message)) {
      return { status: 429, body: { error: 'rate', message: 'GitHub rate limit reached. The dashboard will catch up in a few minutes.' } };
    }
    if (error.status === 403) {
      return { status: 401, body: { error: 'permission', message: `The token can't read ${error.path}. Give it read access to issues, pull requests, contents and Actions.` } };
    }
    return { status: 502, body: { error: 'github', message: `GitHub answered ${error.status}: ${error.message}` } };
  }
  return { status: 502, body: { error: 'github', message: error instanceof Error ? error.message : String(error) } };
}

export default async function handler(req: IncomingMessage, res: ServerResponse): Promise<void> {
  if (req.method !== 'GET') return sendJson(req, res, 405, { error: 'method', message: 'Use GET' });
  const auth = tokenFor(req);
  if (!auth) return sendJson(req, res, 401, { error: 'connect', message: 'Connect a GitHub token to see the dashboard.' });
  const key = createHash('sha256').update(auth.token ?? auth.source).digest('hex');
  let snapshot: Snapshot;
  try {
    snapshot = await snapshotFor(new GitHub(REPO, auth.token), key);
  } catch (error) {
    const { status, body } = problem(error);
    return sendJson(req, res, status, body);
  }
  const body = JSON.stringify(snapshot);
  const etag = `"${createHash('sha1').update(JSON.stringify({ ...snapshot, generatedAt: '' })).digest('hex')}"`;
  const headers = { ETag: etag, 'Cache-Control': 'private, no-cache', 'X-Token-Source': auth.source };
  if (req.headers['if-none-match'] === etag) {
    res.writeHead(304, headers);
    return void res.end();
  }
  send(req, res, 200, body, { 'Content-Type': 'application/json; charset=utf-8', ...headers });
}
