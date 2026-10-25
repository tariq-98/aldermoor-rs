// Request and response helpers for the functions, and where the GitHub token comes from.

import type { IncomingMessage, ServerResponse } from 'node:http';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';

export const REPO = process.env.DASHBOARD_REPO ?? 'tariq-98/aldermoor-rs';
export const COOKIE = 'aldermoor_gh';
const TOKEN = /^[A-Za-z0-9_]{20,255}$/;

export type TokenSource = 'cookie' | 'env' | 'dev';

export function cookies(req: IncomingMessage): Map<string, string> {
  const out = new Map<string, string>();
  for (const part of (req.headers.cookie ?? '').split(';')) {
    const eq = part.indexOf('=');
    if (eq > 0) out.set(part.slice(0, eq).trim(), decodeURIComponent(part.slice(eq + 1).trim()));
  }
  return out;
}

export function validToken(token: unknown): token is string {
  return typeof token === 'string' && TOKEN.test(token);
}

/**
 * The token to read GitHub with: the one this browser connected (an HttpOnly cookie), else a
 * GITHUB_TOKEN set on the Vercel project for every device. `dev` is the local dev server, which
 * runs behind a proxy that signs requests itself; it never applies on Vercel.
 */
export function tokenFor(req: IncomingMessage): { token: string | null; source: TokenSource } | null {
  const cookie = cookies(req).get(COOKIE);
  if (validToken(cookie)) return { token: cookie, source: 'cookie' };
  if (validToken(process.env.GITHUB_TOKEN)) return { token: process.env.GITHUB_TOKEN, source: 'env' };
  if (process.env.DASHBOARD_DEV_ANONYMOUS === '1' && !process.env.VERCEL) return { token: null, source: 'dev' };
  return null;
}

export function tokenCookie(token: string | null): string {
  const secure = process.env.DASHBOARD_DEV_ANONYMOUS === '1' && !process.env.VERCEL ? '' : '; Secure';
  return token
    ? `${COOKIE}=${encodeURIComponent(token)}; Path=/api; HttpOnly; SameSite=Strict; Max-Age=31536000${secure}`
    : `${COOKIE}=; Path=/api; HttpOnly; SameSite=Strict; Max-Age=0${secure}`;
}

export function query(req: IncomingMessage): URLSearchParams {
  return new URL(req.url ?? '/', 'http://local').searchParams;
}

/** Same-origin check for requests that change state: a browser always sends Origin on them. */
export function sameOrigin(req: IncomingMessage): boolean {
  const origin = req.headers.origin;
  if (!origin) return true;
  try {
    return new URL(origin).host === req.headers.host;
  } catch {
    return false;
  }
}

export async function readBody(req: IncomingMessage, limit = 16_384): Promise<string> {
  let size = 0;
  const chunks: Buffer[] = [];
  for await (const chunk of req) {
    size += (chunk as Buffer).length;
    if (size > limit) throw new Error('Request body too large');
    chunks.push(chunk as Buffer);
  }
  return Buffer.concat(chunks).toString('utf8');
}

/** Sends `body`, compressed when the client accepts it (the snapshot shrinks about sixfold). */
export function send(req: IncomingMessage, res: ServerResponse, status: number, body: string | Buffer,
  headers: Record<string, string>): void {
  let data = typeof body === 'string' ? Buffer.from(body) : body;
  const accept = String(req.headers['accept-encoding'] ?? '');
  const out: Record<string, string> = { ...headers, Vary: 'Accept-Encoding' };
  if (data.length > 1024 && /\bbr\b/.test(accept)) {
    data = brotliCompressSync(data, { params: { [constants.BROTLI_PARAM_QUALITY]: 5 } });
    out['Content-Encoding'] = 'br';
  } else if (data.length > 1024 && /\bgzip\b/.test(accept)) {
    data = gzipSync(data, { level: 6 });
    out['Content-Encoding'] = 'gzip';
  }
  out['Content-Length'] = String(data.length);
  res.writeHead(status, out);
  res.end(data);
}

export function sendJson(req: IncomingMessage, res: ServerResponse, status: number, value: unknown,
  headers: Record<string, string> = {}): void {
  send(req, res, status, JSON.stringify(value), {
    'Content-Type': 'application/json; charset=utf-8',
    'Cache-Control': 'private, no-store',
    ...headers,
  });
}
