// Serves the built site the way Vercel does: static files from .vercel/output/static, /api/<name>
// through the built functions and the headers from config.json. Run it with `npm run dev`.
//
// In a Claude Code cloud session GitHub is reached through a proxy that signs requests itself, so
// no token is needed there: NODE_USE_ENV_PROXY=1 DASHBOARD_DEV_ANONYMOUS=1 npm run dev

import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const OUT = '.vercel/output';
const STATIC = resolve(OUT, 'static');
const PORT = Number(process.env.PORT ?? 3000);
const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.webmanifest': 'application/manifest+json',
};

const config = JSON.parse(await readFile(join(OUT, 'config.json'), 'utf8'));
const handlers = new Map();

function headersFor(path) {
  const out = {};
  for (const route of config.routes) {
    if (route.handle) break;
    if (new RegExp(`^${route.src}$`).test(path)) Object.assign(out, route.headers);
  }
  return out;
}

async function handler(name) {
  if (!handlers.has(name)) {
    const file = pathToFileURL(resolve(OUT, 'functions', 'api', `${name}.func`, 'index.js')).href;
    handlers.set(name, (await import(file)).default);
  }
  return handlers.get(name);
}

createServer(async (req, res) => {
  const url = new URL(req.url ?? '/', 'http://localhost');
  for (const [key, value] of Object.entries(headersFor(url.pathname))) res.setHeader(key, value);
  const fn = url.pathname.match(/^\/api\/(\w+)$/)?.[1];
  if (fn) {
    try {
      return await (await handler(fn))(req, res);
    } catch (error) {
      console.error(error);
      if (!res.headersSent) res.writeHead(error?.code === 'ERR_MODULE_NOT_FOUND' ? 404 : 500);
      return res.end();
    }
  }
  const file = normalize(join(STATIC, url.pathname === '/' ? 'index.html' : decodeURIComponent(url.pathname)));
  if (!file.startsWith(STATIC)) {
    res.writeHead(403);
    return res.end();
  }
  try {
    const body = await readFile(file);
    res.writeHead(200, { 'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('Not found');
  }
}).listen(PORT, () => console.log(`Aldermoor Live on http://localhost:${PORT}`));
