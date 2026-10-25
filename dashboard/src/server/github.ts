// A small GitHub REST client. Every GET is conditional (If-None-Match against an ETag cache kept
// per token), so a refresh where nothing changed is a round of 304s, which GitHub doesn't count
// against the rate limit.

import { createHash } from 'node:crypto';

export const API = 'https://api.github.com';

export class GitHubError extends Error {
  readonly status: number;
  readonly path: string;

  constructor(message: string, status: number, path: string) {
    super(message);
    this.status = status;
    this.path = path;
  }
}

interface Cached {
  etag: string;
  body: unknown;
  link: string | null;
}

export interface Page<T> {
  data: T;
  link: string | null;
}

// Module scope outlives a request on a warm function instance; that is the whole cache.
const cache = new Map<string, Cached>();
const CACHE_LIMIT = 600;

function remember(key: string, entry: Cached): void {
  cache.delete(key);
  cache.set(key, entry);
  if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value as string);
}

/** The page number in a Link header's rel="last", if there is one. */
export function lastPage(link: string | null): number | null {
  const match = link?.match(/[?&]page=(\d+)[^>]*>;\s*rel="last"/);
  return match ? Number(match[1]) : null;
}

/** Runs `tasks` with at most `limit` in flight, keeping their order. */
export async function inBatches<T>(tasks: (() => Promise<T>)[], limit = 8): Promise<T[]> {
  const out: T[] = new Array(tasks.length);
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < tasks.length) {
      const i = next++;
      out[i] = await tasks[i]!();
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, tasks.length) }, worker));
  return out;
}

export class GitHub {
  readonly repo: string;
  readonly #token: string | null;
  readonly #key: string;
  readonly #fetch: typeof fetch;

  /** `token` null sends no Authorization header; only the local dev server, behind an
   * authenticating proxy, does that. */
  constructor(repo: string, token: string | null, fetcher: typeof fetch = fetch) {
    this.repo = repo;
    this.#token = token;
    this.#key = createHash('sha256').update(token ?? 'anonymous').digest('hex').slice(0, 16);
    this.#fetch = fetcher;
  }

  headers(extra: Record<string, string> = {}): Record<string, string> {
    const headers: Record<string, string> = {
      Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
      'User-Agent': 'aldermoor-dashboard',
      ...extra,
    };
    if (this.#token) headers.Authorization = `Bearer ${this.#token}`;
    return headers;
  }

  get token(): string | null {
    return this.#token;
  }

  /** GET `path` (relative to the API root, or `repo:` for the repo's own routes). `remember`
   * false skips the ETag cache, for large one-off bodies such as file contents. */
  async get<T>(path: string, { remember: keep = true } = {}): Promise<Page<T>> {
    const url = path.startsWith('repo:') ? `/repos/${this.repo}${path.slice(5)}` : path;
    const key = `${this.#key} ${url}`;
    const hit = keep ? cache.get(key) : undefined;
    const res = await this.#fetch(API + url, {
      headers: this.headers(hit ? { 'If-None-Match': hit.etag } : {}),
      signal: AbortSignal.timeout(10_000),
    });
    if (res.status === 304 && hit) {
      remember(key, hit);
      return { data: hit.body as T, link: hit.link };
    }
    if (!res.ok) {
      let message = res.statusText;
      try {
        message = ((await res.json()) as { message?: string }).message ?? message;
      } catch {
        // Not JSON; keep the status text.
      }
      throw new GitHubError(message, res.status, url);
    }
    const body = (await res.json()) as T;
    const link = res.headers.get('link');
    const etag = res.headers.get('etag');
    if (etag && keep) remember(key, { etag, body, link });
    return { data: body, link };
  }

  /**
   * Every item of a list sorted oldest first, so earlier pages stay the same and come back 304.
   * Page 1's Link header can be one page out of date when page 1 itself is a 304, so the page
   * after the last known one is always asked for too. Keeps the newest `maxPages` pages.
   */
  async all<T>(path: string, maxPages = 30): Promise<T[]> {
    const sep = path.includes('?') ? '&' : '?';
    const page = (n: number) => this.get<T[]>(`${path}${sep}per_page=100&page=${n}`);
    const first = await page(1);
    const known = lastPage(first.link) ?? 1;
    const start = Math.max(2, known + 2 - maxPages);
    const numbers = Array.from({ length: Math.max(0, known + 2 - start) }, (_, i) => start + i);
    const pages = [first.data, ...(await inBatches(numbers.map((n) => async () => (await page(n)).data)))];
    let n = known + 1;
    while (pages[pages.length - 1]!.length === 100 && n < known + maxPages) {
      n += 1;
      pages.push((await page(n)).data);
    }
    const items = pages.flat();
    return start > 2 ? items.slice(first.data.length) : items;
  }
}
