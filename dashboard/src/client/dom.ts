// DOM building without a framework. Text always goes in as text nodes, never as HTML, because
// titles and comments are written by agents and could hold anything.

export type Child = Node | string | number | null | undefined | false | Child[];
export type Props = Record<string, string | number | boolean | null | undefined | EventListener>;

export function h<K extends keyof HTMLElementTagNameMap>(tag: K, props: Props | null = null, ...children: Child[]): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props ?? {})) {
    if (value === null || value === undefined || value === false) continue;
    if (typeof value === 'function') el.addEventListener(key.replace(/^on/, ''), value);
    else if (key === 'class') el.className = String(value);
    else if (value === true) el.setAttribute(key, '');
    else el.setAttribute(key, String(value));
  }
  append(el, children);
  return el;
}

export function append(parent: Node, children: Child[]): void {
  for (const child of children.flat(Infinity as 1) as Child[]) {
    if (child === null || child === undefined || child === false) continue;
    parent.appendChild(child instanceof Node ? child : document.createTextNode(String(child)));
  }
}

/** Only links into GitHub or this site are made clickable. */
export function safeHref(url: string): string | null {
  if (url.startsWith('/api/')) return url;
  return /^https:\/\/github\.com\//.test(url) ? url : null;
}

/** A link that opens GitHub in a new tab. Anything else stays in this tab unless `props` says. */
export function link(url: string, props: Props, ...children: Child[]): HTMLAnchorElement {
  const href = safeHref(url);
  const target = props.target ?? (href?.startsWith('https:') ? '_blank' : null);
  return h('a', { ...props, href, target, rel: 'noopener noreferrer' }, ...children);
}

const SVG = 'http://www.w3.org/2000/svg';

// 24-unit line icons, drawn for this page.
const PATHS: Record<string, string> = {
  overview: 'M4 4h7v7H4z M13 4h7v4h-7z M13 10h7v10h-7z M4 13h7v7H4z',
  pr: 'M6 8v8 M18 16v-5a4 4 0 0 0-4-4h-3 M13 4.5 10.5 7 13 9.5',
  merged: 'M6 8v8 M6 8c0 4 4 5 9.5 5',
  issue: 'M12 3a9 9 0 1 0 0.01 0 M12 11.2v1.6',
  visuals: 'M3.5 5.5h17v13h-17z M3.5 15l5-4.5 4 3.5 3-2.5 5 4 M15.5 9.5h.01',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  cross: 'M6.5 6.5l11 11 M17.5 6.5l-11 11',
  running: 'M12 3a9 9 0 1 0 9 9',
  dash: 'M7 12h10',
  play: 'M8 5.5v13l10.5-6.5z',
  comment: 'M4 5h16v11H10l-6 4z',
  claim: 'M6 21V4h11l-2.5 4 2.5 4H6',
  release: 'M7 11V8a5 5 0 0 1 9.6-2 M5 11h14v10H5z',
  push: 'M12 9a3 3 0 1 0 0.01 0 M3 12h6 M15 12h6',
  external: 'M14 4h6v6 M20 4l-8.5 8.5 M18 14v6H4V6h6',
  refresh: 'M20 12a8 8 0 1 1-2.4-5.7 M20 4v4.5h-4.5',
  close: 'M6 6l12 12 M18 6L6 18',
  prev: 'M15 5l-7 7 7 7',
  next: 'M9 5l7 7-7 7',
  done: 'M12 3a9 9 0 1 0 0.01 0 M8 12.3l2.8 2.7L16.3 9.5',
  decide: 'M12 2.8 21.2 12 12 21.2 2.8 12z M12 8v5 M12 16.2v.3',
  blocked: 'M12 3a9 9 0 1 0 0.01 0 M5.7 5.7l12.6 12.6',
  review: 'M2.5 12s3.5-6.5 9.5-6.5 9.5 6.5 9.5 6.5-3.5 6.5-9.5 6.5S2.5 12 2.5 12z M12 9.5a2.5 2.5 0 1 0 0.01 0',
  untriaged: 'M12 3a9 9 0 1 0 0.01 0',
  warning: 'M12 3.5 21.5 20h-19z M12 10v4.5 M12 17.2v.3',
  branch: 'M7 4v16 M17 8v1a4 4 0 0 1-4 4H7',
  agent: 'M5.5 9.5h13v9.5h-13z M12 9.5V6.5 M9.5 14v.5 M14.5 14v.5',
};

const CIRCLES: Record<string, [number, number, number][]> = {
  pr: [[6, 5.5, 2.5], [6, 18.5, 2.5], [18, 18.5, 2.5]],
  merged: [[6, 5.5, 2.5], [6, 18.5, 2.5], [18, 13, 2.5]],
  branch: [[17, 5.5, 2.5]],
  agent: [[12, 4.8, 1.6]],
};

export function icon(name: keyof typeof PATHS | string, cls = 'icon'): SVGSVGElement {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('class', `${cls} icon-${name}`);
  svg.setAttribute('aria-hidden', 'true');
  const path = document.createElementNS(SVG, 'path');
  path.setAttribute('d', PATHS[name] ?? PATHS.dash!);
  svg.appendChild(path);
  for (const [cx, cy, r] of CIRCLES[name] ?? []) {
    const circle = document.createElementNS(SVG, 'circle');
    circle.setAttribute('cx', String(cx));
    circle.setAttribute('cy', String(cy));
    circle.setAttribute('r', String(r));
    svg.appendChild(circle);
  }
  return svg;
}
