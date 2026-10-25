// Full-screen viewer for one post's captures. Swipe, the arrow keys or the side buttons move
// between them; the back button closes it. Videos play inline, which iOS needs.

import type { VisualItem, VisualPost } from '../shared/types.ts';
import { h, icon, link } from './dom.ts';

interface Parts {
  dialog: HTMLDialogElement;
  stage: HTMLElement;
  counter: HTMLElement;
  title: HTMLElement;
  caption: HTMLElement;
  links: HTMLElement;
  prev: HTMLButtonElement;
  next: HTMLButtonElement;
}

let parts: Parts | null = null;
let post: VisualPost | null = null;
let index = 0;

export function openLightbox(open: VisualPost, at: number): void {
  parts ??= build();
  post = open;
  index = Math.max(0, Math.min(at, open.items.length - 1));
  show();
  if (!parts.dialog.open) {
    // An entry of its own in the history, so the phone's back gesture closes the viewer rather
    // than leaving the tab.
    history.pushState({ lightbox: true }, '');
    parts.dialog.showModal();
    document.documentElement.classList.add('lightbox-open');
  }
}

function build(): Parts {
  const close = h('button', { type: 'button', class: 'lb-button', 'aria-label': 'Close' }, icon('close'));
  const prev = h('button', { type: 'button', class: 'lb-side lb-prev', 'aria-label': 'Previous capture' }, icon('prev'));
  const next = h('button', { type: 'button', class: 'lb-side lb-next', 'aria-label': 'Next capture' }, icon('next'));
  const counter = h('span', { class: 'lb-counter' });
  const title = h('span', { class: 'lb-title' });
  const stage = h('div', { class: 'lb-stage' });
  const caption = h('p', { class: 'lb-caption' });
  const links = h('div', { class: 'lb-links' });
  const dialog = h('dialog', { class: 'lightbox', 'aria-label': 'Captures' },
    h('div', { class: 'lb-top' }, h('div', { class: 'lb-heading' }, counter, title), close),
    h('div', { class: 'lb-body' }, stage, prev, next),
    h('div', { class: 'lb-bottom' }, caption, links));

  close.addEventListener('click', () => dialog.close());
  prev.addEventListener('click', () => move(-1));
  next.addEventListener('click', () => move(1));
  dialog.addEventListener('keydown', (event) => {
    if (event.key === 'ArrowLeft') move(-1);
    if (event.key === 'ArrowRight') move(1);
  });
  dialog.addEventListener('close', () => {
    stage.replaceChildren(); // Stops a playing video.
    document.documentElement.classList.remove('lightbox-open');
    if ((history.state as { lightbox?: boolean } | null)?.lightbox) history.back();
  });
  window.addEventListener('popstate', () => {
    if (dialog.open) dialog.close();
  });

  // A sideways swipe on a picture moves to the next or previous one. Videos keep their own
  // controls, so swipes on them are left alone.
  let start: { x: number; y: number } | null = null;
  stage.addEventListener('pointerdown', (event) => {
    start = event.isPrimary && event.target instanceof HTMLImageElement ? { x: event.clientX, y: event.clientY } : null;
  });
  stage.addEventListener('pointerup', (event) => {
    if (!start) return;
    const dx = event.clientX - start.x;
    const dy = event.clientY - start.y;
    start = null;
    if (Math.abs(dx) > 48 && Math.abs(dx) > Math.abs(dy) * 1.5) move(dx < 0 ? 1 : -1);
  });
  stage.addEventListener('pointercancel', () => {
    start = null;
  });

  document.body.append(dialog);
  return { dialog, stage, counter, title, caption, links, prev, next };
}

function move(step: number): void {
  if (!post) return;
  const to = index + step;
  if (to < 0 || to >= post.items.length) return;
  index = to;
  show();
}

function media(item: VisualItem): HTMLElement {
  if (item.type === 'video') {
    return h('video', { src: item.src, poster: item.thumb, controls: true, playsinline: true, preload: 'metadata' });
  }
  const img = h('img', { src: item.thumb ?? item.src, alt: item.caption, decoding: 'async' });
  if (item.thumb) {
    // The thumbnail shows at once; the full picture replaces it when it has loaded.
    const full = new Image();
    full.decoding = 'async';
    full.onload = () => {
      if (img.isConnected) img.src = item.src;
    };
    full.src = item.src;
  }
  return img;
}

function show(): void {
  if (!parts || !post) return;
  const item = post.items[index];
  if (!item) return;
  const many = post.items.length > 1;
  parts.counter.textContent = many ? `${index + 1} of ${post.items.length}` : '';
  parts.title.textContent = `#${post.number} · ${post.title}`;
  parts.caption.textContent = item.caption;
  parts.caption.hidden = !item.caption;
  parts.links.replaceChildren(
    link(item.src, { class: 'lb-link', target: '_blank' }, item.type === 'video' ? 'Open video' : 'Full size', icon('external')),
    link(item.github, { class: 'lb-link' }, 'File on GitHub', icon('external')),
    link(post.url, { class: 'lb-link' }, 'Comment', icon('external')));
  parts.prev.hidden = !many;
  parts.next.hidden = !many;
  parts.prev.disabled = index === 0;
  parts.next.disabled = index === post.items.length - 1;
  parts.stage.replaceChildren(media(item));
}
