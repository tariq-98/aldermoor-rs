// Stand-ins for GitHub and for a browser talking to a function, so tests run offline.

import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';

export interface Call {
  path: string;
  params: URLSearchParams;
  headers: Headers;
}

/** A fetch that answers from `answer` and records every call. */
export function fakeFetch(answer: (call: Call) => Response): { fetcher: typeof fetch; calls: Call[] } {
  const calls: Call[] = [];
  const fetcher = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url);
    const call = { path: url.pathname, params: url.searchParams, headers: new Headers(init?.headers) };
    calls.push(call);
    return answer(call);
  }) as typeof fetch;
  return { fetcher, calls };
}

export function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', ...headers } });
}

export interface Reply {
  status: number;
  headers: Headers;
  body: Buffer;
}

/** Runs `handler` behind a real HTTP server for one request. */
export async function request(
  handler: (req: IncomingMessage, res: ServerResponse) => unknown,
  init: RequestInit & { path?: string } = {},
): Promise<Reply> {
  const server = createServer((req, res) => {
    Promise.resolve(handler(req, res)).catch((error: unknown) => {
      res.writeHead(500);
      res.end(String(error));
    });
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  try {
    const res = await fetch(`http://127.0.0.1:${port}${init.path ?? '/'}`, { redirect: 'manual', ...init });
    return { status: res.status, headers: res.headers, body: Buffer.from(await res.arrayBuffer()) };
  } finally {
    server.close();
  }
}
