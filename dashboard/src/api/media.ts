// GET /api/media?ref=<branch or commit>&path=<file>: a capture from the repo.

import type { IncomingMessage, ServerResponse } from 'node:http';
import { GitHub } from '../server/github.ts';
import { query, REPO, tokenFor } from '../server/http.ts';
import { serveMedia } from '../server/media.ts';
import { problem } from './state.ts';

export default async function handler(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const params = query(req);
  const auth = tokenFor(req);
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    res.writeHead(405, { Allow: 'GET, HEAD' });
    return void res.end();
  }
  if (!auth) {
    res.writeHead(401, { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'private, no-store' });
    return void res.end('Connect a GitHub token first');
  }
  try {
    await serveMedia(new GitHub(REPO, auth.token), req, res, params.get('ref') ?? '', params.get('path') ?? '');
  } catch (error) {
    if (res.headersSent) return void res.end();
    const { status, body } = problem(error);
    res.writeHead(status, { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'private, no-store' });
    res.end(body.message);
  }
}
