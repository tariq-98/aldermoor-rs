// Serves a capture from the repo to the page. The repo is private, so the browser can't load
// GitHub's file URLs itself; this reads them with the token and passes them on with the right
// type and caching. Videos come in ranges, which iOS needs to play them, capped below the
// 4.5 MB a Vercel function may return.

import type { IncomingMessage, ServerResponse } from 'node:http';
import { GitHub, GitHubError } from './github.ts';
import { mediaType, safePath, safeRef } from './visuals.ts';

export const CHUNK = 4 * 1024 * 1024;

const TYPES: Record<string, string> = {
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  png: 'image/png',
  webp: 'image/webp',
  gif: 'image/gif',
  mp4: 'video/mp4',
  mov: 'video/quicktime',
  webm: 'video/webm',
};

const LFS_POINTER = /^version https:\/\/git-lfs\.github\.com\/spec\/v1\s+oid sha256:([0-9a-f]{64})\s+size (\d+)/;

export interface FileInfo {
  sha: string;
  size: number;
  /** A short-lived signed URL that needs no token. */
  downloadUrl: string | null;
  content: Buffer | null;
  lfs: { oid: string; size: number } | null;
}

export function contentType(path: string): string | null {
  return TYPES[path.slice(path.lastIndexOf('.') + 1).toLowerCase()] ?? null;
}

/** Parses a single `bytes=` range against `total`. Null means serve the whole file. */
export function parseRange(header: string | undefined, total: number): { start: number; end: number } | 'invalid' | null {
  const match = header?.match(/^bytes=(\d*)-(\d*)$/);
  if (!match || (match[1] === '' && match[2] === '')) return null;
  let start: number;
  let end: number;
  if (match[1] === '') {
    start = Math.max(0, total - Number(match[2]));
    end = total - 1;
  } else {
    start = Number(match[1]);
    end = match[2] === '' ? total - 1 : Math.min(Number(match[2]), total - 1);
  }
  if (start >= total || start > end) return 'invalid';
  return { start, end };
}

export function lfsPointer(content: Buffer | null): { oid: string; size: number } | null {
  if (!content || content.length > 1024) return null;
  const match = content.toString('utf8').match(LFS_POINTER);
  return match ? { oid: match[1]!, size: Number(match[2]) } : null;
}

const infoCache = new Map<string, { info: FileInfo; at: number }>();

export async function fileInfo(gh: GitHub, ref: string, path: string): Promise<FileInfo> {
  const key = `${gh.token ?? ''} ${ref} ${path}`;
  const hit = infoCache.get(key);
  if (hit && Date.now() - hit.at < 60_000) return hit.info;
  const encoded = path.split('/').map(encodeURIComponent).join('/');
  const { data } = await gh.get<{ type: string; sha: string; size: number; download_url: string | null; content?: string; encoding?: string }>(
    `repo:/contents/${encoded}?ref=${encodeURIComponent(ref)}`,
    { remember: false },
  );
  if (data.type !== 'file') throw new GitHubError('Not a file', 404, path);
  const content = data.encoding === 'base64' && data.content ? Buffer.from(data.content, 'base64') : null;
  const info: FileInfo = { sha: data.sha, size: data.size, downloadUrl: data.download_url, content, lfs: lfsPointer(content) };
  infoCache.set(key, { info: { ...info, content: null }, at: Date.now() });
  if (infoCache.size > 300) infoCache.delete(infoCache.keys().next().value as string);
  return info;
}

/** A signed download URL for an LFS object, from the LFS batch API. */
export async function lfsUrl(gh: GitHub, oid: string, size: number): Promise<string> {
  const headers: Record<string, string> = {
    Accept: 'application/vnd.git-lfs+json',
    'Content-Type': 'application/vnd.git-lfs+json',
    'User-Agent': 'aldermoor-dashboard',
  };
  if (gh.token) headers.Authorization = `Basic ${Buffer.from(`x-access-token:${gh.token}`).toString('base64')}`;
  const res = await fetch(`https://github.com/${gh.repo}.git/info/lfs/objects/batch`, {
    method: 'POST',
    headers,
    body: JSON.stringify({ operation: 'download', transfers: ['basic'], objects: [{ oid, size }] }),
    signal: AbortSignal.timeout(10_000),
  });
  if (!res.ok) throw new GitHubError(`LFS batch failed: ${res.status}`, res.status, oid);
  const body = (await res.json()) as { objects?: { actions?: { download?: { href?: string } } }[] };
  const href = body.objects?.[0]?.actions?.download?.href;
  if (!href) throw new GitHubError('LFS object missing', 404, oid);
  return href;
}

async function fetchBytes(url: string, range?: string): Promise<{ bytes: Buffer; partial: boolean }> {
  const res = await fetch(url, { headers: range ? { Range: range } : {}, signal: AbortSignal.timeout(20_000) });
  if (!res.ok) throw new GitHubError(`Download failed: ${res.status}`, res.status, url);
  return { bytes: Buffer.from(await res.arrayBuffer()), partial: res.status === 206 };
}

function plain(res: ServerResponse, status: number, message: string, headers: Record<string, string> = {}): void {
  res.writeHead(status, { 'Content-Type': 'text/plain; charset=utf-8', 'Cache-Control': 'private, no-store', ...headers });
  res.end(message);
}

export async function serveMedia(gh: GitHub, req: IncomingMessage, res: ServerResponse, ref: string, path: string): Promise<void> {
  const type = contentType(path);
  if (!type || !mediaType(path) || !safeRef(ref) || !safePath(path)) return plain(res, 400, 'Not a capture path');
  let info: FileInfo;
  try {
    info = await fileInfo(gh, ref, path);
  } catch (error) {
    if (error instanceof GitHubError && (error.status === 404 || error.status === 422)) return plain(res, 404, 'No such capture');
    throw error;
  }
  const total = info.lfs?.size ?? info.size;
  const etag = `"${info.lfs?.oid ?? info.sha}"`;
  const cache = /^[0-9a-f]{40}$/.test(ref) ? 'private, max-age=31536000, immutable' : 'private, max-age=300';
  const common = { 'Content-Type': type, 'Cache-Control': cache, ETag: etag, 'Accept-Ranges': 'bytes' };
  if (req.headers['if-none-match'] === etag) {
    res.writeHead(304, { 'Cache-Control': cache, ETag: etag });
    return void res.end();
  }
  const inline = info.lfs ? null : info.content;
  const url = info.lfs ? await lfsUrl(gh, info.lfs.oid, info.lfs.size) : info.downloadUrl;
  const range = parseRange(req.headers.range, total);
  if (range === 'invalid') return plain(res, 416, 'Range not satisfiable', { 'Content-Range': `bytes */${total}` });

  if (!range) {
    if (total > CHUNK) {
      // Too big to pass through in one response; send the browser to the signed URL instead.
      if (!url) return plain(res, 502, 'No download URL');
      res.writeHead(302, { Location: url, 'Cache-Control': 'private, max-age=60' });
      return void res.end();
    }
    const bytes = inline ?? (await fetchBytes(url!)).bytes;
    res.writeHead(200, { ...common, 'Content-Length': String(bytes.length) });
    return void res.end(bytes);
  }

  const end = Math.min(range.end, range.start + CHUNK - 1);
  let bytes: Buffer;
  if (inline) {
    bytes = inline.subarray(range.start, end + 1);
  } else {
    const got = await fetchBytes(url!, `bytes=${range.start}-${end}`);
    bytes = got.partial ? got.bytes : got.bytes.subarray(range.start, end + 1);
  }
  res.writeHead(206, {
    ...common,
    'Content-Range': `bytes ${range.start}-${range.start + bytes.length - 1}/${total}`,
    'Content-Length': String(bytes.length),
  });
  res.end(bytes);
}
