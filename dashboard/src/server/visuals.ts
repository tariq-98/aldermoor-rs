// Screenshots and videos posted on issues and PRs. New posts carry an `aldermoor-visuals`
// manifest (.claude/skills/attach-visuals); older ones only link images on the captures branch,
// so those are read from the links and the bold caption above each one.

import type { Verdict, VisualItem, VisualPost } from '../shared/types.ts';
import type { RawComment } from './raw.ts';

const MANIFEST = /<!--\s*aldermoor-visuals\s+(\{[\s\S]*?\})\s*-->/;
const IMAGE_EXT = new Set(['jpg', 'jpeg', 'png', 'webp', 'gif']);
const VIDEO_EXT = new Set(['mp4', 'mov', 'webm']);
const SAFE_PATH = /^(?!\/)(?!.*\.\.)[A-Za-z0-9._\-/]{1,300}$/;
const SAFE_REF = /^[A-Za-z0-9._\-/]{1,100}$/;

export function mediaType(path: string): 'image' | 'video' | null {
  const ext = path.slice(path.lastIndexOf('.') + 1).toLowerCase();
  if (IMAGE_EXT.has(ext)) return 'image';
  if (VIDEO_EXT.has(ext)) return 'video';
  return null;
}

export function safePath(path: string): boolean {
  return SAFE_PATH.test(path) && !path.split('/').some((part) => part === '' || part === '.');
}

export function safeRef(ref: string): boolean {
  return SAFE_REF.test(ref) && !ref.includes('..');
}

export function mediaUrl(ref: string, path: string): string {
  return `/api/media?ref=${encodeURIComponent(ref)}&path=${encodeURIComponent(path)}`;
}

interface Context {
  repo: string;
  number: number;
  kind: 'pr' | 'issue';
  /** The PR's latest commit, when the post is on an open PR. */
  headSha: string | null;
}

function item(repo: string, ref: string, path: string, caption: string, thumb: string | null): VisualItem | null {
  const type = mediaType(path);
  if (!type || !safeRef(ref) || !safePath(path)) return null;
  const thumbOk = thumb && safePath(thumb) && mediaType(thumb) === 'image';
  return {
    type,
    caption,
    src: mediaUrl(ref, path),
    thumb: thumbOk ? mediaUrl(ref, thumb) : null,
    github: `https://github.com/${repo}/blob/${ref}/${path}`,
  };
}

function text(value: unknown, limit = 300): string | null {
  return typeof value === 'string' && value.trim() ? value.trim().slice(0, limit) : null;
}

function isCurrent(head: string | null, ctx: Context): boolean | null {
  if (!head || !ctx.headSha || head.length < 7) return null;
  return ctx.headSha.toLowerCase().startsWith(head.toLowerCase());
}

/** A post from the manifest, or null when the comment has none or it can't be read. */
export function fromManifest(comment: RawComment, ctx: Context): VisualPost | null {
  const raw = (comment.body ?? '').match(MANIFEST)?.[1];
  if (!raw) return null;
  let data: Record<string, unknown>;
  try {
    data = JSON.parse(raw) as Record<string, unknown>;
  } catch {
    return null;
  }
  if (data.v !== 1 || !Array.isArray(data.items)) return null;
  const ref = text(data.ref, 100);
  if (!ref) return null;
  const items = data.items.flatMap((entry: unknown) => {
    const e = (entry ?? {}) as Record<string, unknown>;
    const path = text(e.path);
    const made = path ? item(ctx.repo, ref, path, text(e.caption) ?? '', text(e.thumb)) : null;
    return made ? [made] : [];
  });
  if (items.length === 0) return null;
  const verdict = data.verdict === 'pass' || data.verdict === 'fail' ? data.verdict : 'info';
  const head = text(data.head, 40);
  return {
    id: comment.id,
    url: comment.html_url,
    at: comment.created_at,
    number: ctx.number,
    kind: ctx.kind,
    title: text(data.title, 140) ?? 'Visuals',
    verdict,
    head,
    base: text(data.base, 40),
    by: text(data.by, 120),
    current: isCurrent(head, ctx),
    legacy: false,
    items,
  };
}

function decode(path: string): string {
  try {
    return decodeURIComponent(path);
  } catch {
    return path;
  }
}

/** Markdown reduced to its words: links to their text, emphasis and code marks dropped, and a
 * heading, quote or list marker at the start. */
export function plainText(markdown: string): string {
  return markdown
    .replace(/!\[[^\]]*\]\([^)]*\)/g, '')
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/<[^>]+>/g, '')
    .replace(/\*\*|__|\*|`/g, '')
    .replace(/^\s*(?:#+(?=\s)|>|[-+](?=\s)|\d+[.)](?=\s))\s*/, '')
    .trim();
}

function tableCells(line: string): string[] | null {
  const row = line.trim();
  if (!row.startsWith('|')) return null;
  return row.replace(/^\|/, '').replace(/\|$/, '').split('|').map((cell) => cell.trim());
}

const SEPARATOR = /^\|?\s*:?-{3,}:?\s*(?:\|\s*:?-{3,}:?\s*)*\|?$/;

/**
 * Image links to files in `repo`. A capture's caption is the text just above it: in a table, the
 * same column of the row above (a header, or a bold row between image rows); otherwise the line
 * above, unless that is the comment's first line, which is the post's title.
 */
export function fromLinks(comment: RawComment, ctx: Context): VisualPost | null {
  const body = (comment.body ?? '').replace(/<!--[\s\S]*?-->/g, '').trim();
  const repo = ctx.repo.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const link = new RegExp(
    String.raw`(?:!\[([^\]]*)\]\(|<img[^>]*?src=")` +
      String.raw`https://(?:github\.com/${repo}/(?:blob|raw)/|raw\.githubusercontent\.com/${repo}/)` +
      String.raw`([A-Za-z0-9._\-]+)/([^\s)"?#]+)(?:\?raw=true)?[)"]`,
    'g',
  );
  const lines = body.split('\n');
  const hasImage = (line: string) => [...line.matchAll(link)].length > 0;
  const items: VisualItem[] = [];
  lines.forEach((line, index) => {
    const row = tableCells(line);
    for (const match of line.matchAll(link)) {
      const [whole, alt, ref, path] = match;
      let caption = '';
      if (row) {
        const column = row.findIndex((cell) => cell.includes(whole));
        for (let j = index - 1; j >= 0; j--) {
          const above = lines[j]!.trim();
          if (!above.startsWith('|')) break;
          if (SEPARATOR.test(above) || hasImage(above)) continue;
          caption = plainText(tableCells(above)?.[column] ?? '');
          break;
        }
      } else {
        for (let j = index - 1; j >= Math.max(1, index - 3); j--) {
          const above = lines[j]!.trim();
          if (!above) continue;
          if (!hasImage(above)) caption = plainText(above);
          break;
        }
      }
      const file = decode(path!);
      const made = item(ctx.repo, ref!, file, caption || alt?.trim() || file.split('/').pop()!, null);
      if (made) items.push(made);
    }
  });
  if (items.length === 0) return null;
  const first = plainText(lines[0] ?? '');
  const verdict: Verdict = /\bFAIL(?:ED)?\b/.test(first) ? 'fail' : /\bPASS(?:ED)?\b/.test(first) ? 'pass' : 'info';
  const head = first.match(/\bhead\s+([0-9a-f]{7,40})\b/i)?.[1] ?? null;
  return {
    id: comment.id,
    url: comment.html_url,
    at: comment.created_at,
    number: ctx.number,
    kind: ctx.kind,
    title: first.slice(0, 140) || 'Captures',
    verdict,
    head,
    base: null,
    by: null,
    current: isCurrent(head, ctx),
    legacy: true,
    items,
  };
}

/** The comment's visuals: its manifest, or failing that (no manifest, or one that can't be read)
 * the images it links. */
export function visualPost(comment: RawComment, ctx: Context): VisualPost | null {
  return fromManifest(comment, ctx) ?? fromLinks(comment, ctx);
}
