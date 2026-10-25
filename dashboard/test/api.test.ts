import assert from 'node:assert/strict';
import type { IncomingMessage } from 'node:http';
import { test } from 'node:test';
import media from '../src/api/media.ts';
import session from '../src/api/session.ts';
import state, { problem } from '../src/api/state.ts';
import { GitHubError } from '../src/server/github.ts';
import { cookies, sameOrigin, tokenCookie, tokenFor, validToken } from '../src/server/http.ts';
import { request } from './fake.ts';

// Nothing here may reach GitHub: with no token, no handler gets that far.
delete process.env.GITHUB_TOKEN;
delete process.env.DASHBOARD_DEV_ANONYMOUS;
delete process.env.VERCEL;

const TOKEN = `github_pat_${'A1b2'.repeat(10)}`;
const req = (headers: Record<string, string>) => ({ headers }) as unknown as IncomingMessage;

test('reads the token from this browser first, then the project', () => {
  assert.equal(tokenFor(req({})), null);
  assert.deepEqual(tokenFor(req({ cookie: `other=1; aldermoor_gh=${TOKEN}` })), { token: TOKEN, source: 'cookie' });
  assert.equal(tokenFor(req({ cookie: 'aldermoor_gh=not a token' })), null);
  process.env.GITHUB_TOKEN = `ghp_${'x'.repeat(36)}`;
  try {
    assert.equal(tokenFor(req({}))?.source, 'env');
    assert.equal(tokenFor(req({ cookie: `aldermoor_gh=${TOKEN}` }))?.source, 'cookie');
  } finally {
    delete process.env.GITHUB_TOKEN;
  }
  process.env.DASHBOARD_DEV_ANONYMOUS = '1';
  process.env.VERCEL = '1';
  try {
    assert.equal(tokenFor(req({})), null, 'never anonymous on Vercel');
  } finally {
    delete process.env.DASHBOARD_DEV_ANONYMOUS;
    delete process.env.VERCEL;
  }
});

test('keeps the token in a cookie scripts cannot read', () => {
  const cookie = tokenCookie(TOKEN);
  for (const part of ['Path=/api', 'HttpOnly', 'SameSite=Strict', 'Secure']) assert.ok(cookie.includes(part), part);
  assert.match(tokenCookie(null), /^aldermoor_gh=; .*Max-Age=0/);
  assert.equal(cookies(req({ cookie: 'a=1; b=x%20y' })).get('b'), 'x y');
  assert.ok(validToken(TOKEN));
  assert.ok(!validToken('github_pat_short'));
  assert.ok(!validToken(`${TOKEN}; Path=/`));
});

test('only this site may change the session', () => {
  assert.ok(sameOrigin(req({ host: 'live.example', origin: 'https://live.example' })));
  assert.ok(sameOrigin(req({ host: 'live.example' })), 'no Origin: not a cross-site browser request');
  assert.ok(!sameOrigin(req({ host: 'live.example', origin: 'https://evil.example' })));
  assert.ok(!sameOrigin(req({ host: 'live.example', origin: 'null' })));
});

test('asks to connect before showing anything', async () => {
  const reply = await request(state);
  assert.equal(reply.status, 401);
  assert.equal(JSON.parse(reply.body.toString()).error, 'connect');
  assert.equal(reply.headers.get('cache-control'), 'private, no-store');
  assert.equal((await request(state, { method: 'POST' })).status, 405);
  assert.equal((await request(media, { path: '/?ref=captures&path=pr1/a.png' })).status, 401);
});

test('checks a token before keeping it', async () => {
  const status = await request(session);
  assert.deepEqual(JSON.parse(status.body.toString()), { connected: false, source: null, repo: 'tariq-98/aldermoor-rs' });
  const post = (body: string, headers: Record<string, string> = { 'Content-Type': 'application/json' }) =>
    request(session, { method: 'POST', body, headers });
  assert.equal((await post('{}', { 'Content-Type': 'text/plain' })).status, 415);
  assert.equal((await post('not json')).status, 400);
  const short = await post(JSON.stringify({ token: 'hunter2' }));
  assert.equal(short.status, 400);
  assert.equal(short.headers.get('set-cookie'), null);
  const cross = await request(session, { method: 'POST', body: '{}', headers: { 'Content-Type': 'application/json', Origin: 'https://evil.example' } });
  assert.equal(cross.status, 403);
  const out = await request(session, { method: 'DELETE' });
  assert.match(out.headers.get('set-cookie') ?? '', /Max-Age=0/);
});

test('explains what GitHub refused', () => {
  const at = (status: number, message: string, path: string) => problem(new GitHubError(message, status, path));
  assert.equal(at(401, 'Bad credentials', '/repos/x/issues').body.error, 'token');
  assert.equal(at(404, 'Not Found', '/repos/x/issues?state=all').body.error, 'repo');
  assert.deepEqual([at(403, 'API rate limit exceeded', '/x').status, at(403, 'API rate limit exceeded', '/x').body.error], [429, 'rate']);
  assert.equal(at(403, 'Resource not accessible', '/repos/x/milestones').body.error, 'permission');
  assert.equal(at(500, 'Server Error', '/x').status, 502);
  assert.equal(problem(new Error('socket hang up')).body.message, 'socket hang up');
});
