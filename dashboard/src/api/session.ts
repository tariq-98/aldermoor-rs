// /api/session: connect this browser to GitHub with a read-only token, or disconnect it.
//   GET     whether a token is set and where it comes from
//   POST    {"token": "..."} checks the token against the repo and keeps it in an HttpOnly cookie
//   DELETE  forgets this browser's token

import type { IncomingMessage, ServerResponse } from 'node:http';
import { GitHub, GitHubError } from '../server/github.ts';
import { readBody, REPO, sameOrigin, sendJson, tokenCookie, tokenFor, validToken } from '../server/http.ts';

/** What the token must read, and the permissions only some sections need: Actions for CI results,
 * Contents for main's commits and the captures. */
const REQUIRED = [
  ['repo:', 'the repository'],
  ['repo:/issues?per_page=1', 'issues'],
  ['repo:/pulls?per_page=1', 'pull requests'],
] as const;
const OPTIONAL = [
  ['repo:/actions/runs?per_page=1', 'Actions'],
  ['repo:/commits?per_page=1', 'Contents'],
] as const;

async function check(token: string): Promise<{ ok: boolean; message?: string; missing: string[] }> {
  const gh = new GitHub(REPO, token);
  for (const [path, what] of REQUIRED) {
    try {
      await gh.get(path, { remember: false });
    } catch (error) {
      if (error instanceof GitHubError && error.status === 401) {
        return { ok: false, message: 'GitHub turned that token down. Check you copied all of it.', missing: [] };
      }
      if (error instanceof GitHubError && (error.status === 403 || error.status === 404)) {
        return {
          ok: false,
          message: `That token can't read ${what} in ${REPO}. Under Repository access choose "Only select repositories" and pick ${REPO.split('/')[1]}, then give it read access to the permissions listed.`,
          missing: [],
        };
      }
      throw error;
    }
  }
  const missing: string[] = [];
  for (const [path, what] of OPTIONAL) {
    try {
      await gh.get(path, { remember: false });
    } catch (error) {
      if (!(error instanceof GitHubError) || (error.status !== 403 && error.status !== 404)) throw error;
      missing.push(what);
    }
  }
  return { ok: true, missing };
}

export default async function handler(req: IncomingMessage, res: ServerResponse): Promise<void> {
  if (req.method === 'GET') {
    const auth = tokenFor(req);
    return sendJson(req, res, 200, { connected: !!auth, source: auth?.source ?? null, repo: REPO });
  }
  if (!sameOrigin(req)) return sendJson(req, res, 403, { error: 'origin', message: 'Cross-site request refused' });
  if (req.method === 'DELETE') {
    return sendJson(req, res, 200, { connected: false }, { 'Set-Cookie': tokenCookie(null) });
  }
  if (req.method !== 'POST') return sendJson(req, res, 405, { error: 'method', message: 'Use GET, POST or DELETE' });
  if (!String(req.headers['content-type'] ?? '').startsWith('application/json')) {
    return sendJson(req, res, 415, { error: 'type', message: 'Send JSON' });
  }
  let token: unknown;
  try {
    token = (JSON.parse(await readBody(req)) as { token?: unknown }).token;
  } catch {
    return sendJson(req, res, 400, { error: 'body', message: 'Send {"token": "..."}' });
  }
  if (typeof token === 'string') token = token.trim();
  if (!validToken(token)) {
    return sendJson(req, res, 400, { error: 'token', message: "That doesn't look like a GitHub token. It starts with github_pat_." });
  }
  try {
    const result = await check(token);
    if (!result.ok) return sendJson(req, res, 400, { error: 'token', message: result.message });
    return sendJson(req, res, 200, { connected: true, missing: result.missing }, { 'Set-Cookie': tokenCookie(token) });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return sendJson(req, res, 502, { error: 'github', message: `Couldn't reach GitHub to check the token: ${message}` });
  }
}
