// Every capture posted on an issue or PR, newest first. Tap one to see it full screen.

import type { VisualPost } from '../../shared/types.ts';
import { h, icon, link } from '../dom.ts';
import { plural, VERDICT_NAMES } from '../format.ts';
import { empty, frame, meta, pill, ref, subjectOf, time, VERDICT_ICONS, VERDICT_TONES } from '../parts.ts';
import type { ViewContext } from '../view.ts';

export function visualsView(view: ViewContext): HTMLElement {
  const s = view.snapshot;
  const number = view.route.arg && /^\d+$/.test(view.route.arg) ? Number(view.route.arg) : null;
  const posts = number === null ? s.visuals : s.visuals.filter((p) => p.number === number);
  const counts = new Map<number, number>();
  for (const post of s.visuals) counts.set(post.number, (counts.get(post.number) ?? 0) + 1);
  const choose = (event: Event) => {
    const value = (event.currentTarget as HTMLSelectElement).value;
    location.hash = value ? `visuals-${value}` : 'visuals';
  };
  return h('div', { class: 'page' },
    h('div', { class: 'page-head' },
      h('h1', null, 'Visuals'),
      h('p', { class: 'lede' }, 'Screenshots and videos agents post on issues and PRs, newest first.')),
    s.visuals.length
      ? h('label', { class: 'picker' },
        h('span', { class: 'picker-label' }, 'Show'),
        h('select', { onchange: choose },
          h('option', { value: '' }, `Everything (${plural(s.visuals.length, 'post')})`),
          [...counts].map(([n, count]) => h('option', { value: String(n), selected: n === number },
            `#${n} ${subjectOf(s, n)} (${count})`))))
      : null,
    posts.length
      ? h('div', { class: 'posts' }, posts.map((post) => postCard(view, post)))
      : h('section', { class: 'card' }, empty(number === null
        ? 'No captures yet. Agents post them with the attach-visuals skill.'
        : `No captures on #${number} yet.`, 'visuals')));
}

function postCard(view: ViewContext, post: VisualPost): HTMLElement {
  const s = view.snapshot;
  return h('article', { class: 'card post' },
    h('header', { class: 'post-head' },
      h('div', { class: 'item-head' },
        pill(VERDICT_NAMES[post.verdict], VERDICT_TONES[post.verdict], VERDICT_ICONS[post.verdict]),
        post.current === false ? pill('Older commit', 'warn', 'warning') : null,
        time(post.at)),
      h('h2', { class: 'post-title' }, post.title),
      meta(
        h('span', { class: 'subject' }, icon(post.kind === 'pr' ? 'pr' : 'issue'), ref(s.repo, post.number), subjectOf(s, post.number)),
        post.by ? h('span', null, icon('agent'), post.by) : null,
        post.head ? h('span', { class: 'sha' }, icon('push'), post.base ? `${post.head.slice(0, 7)} vs ${post.base.slice(0, 7)}` : post.head.slice(0, 7)) : null)),
    h('ul', { class: 'thumbs' }, post.items.map((item, i) => h('li', null,
      h('button', { type: 'button', class: 'thumb', onclick: () => view.openVisuals(post, i) },
        frame(item),
        h('span', { class: 'caption' }, item.caption || (item.type === 'video' ? 'Video' : 'Screenshot')))))),
    h('footer', { class: 'post-foot' }, link(post.url, { class: 'more' }, 'Comment on GitHub', icon('external'))));
}
