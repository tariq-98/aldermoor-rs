// The first-run screen: make a read-only token on GitHub and hand it to the dashboard, which
// keeps it for this browser in a cookie the page's own scripts can't read.

import { h, icon, link } from './dom.ts';

/** GitHub's new fine-grained token page with the name, expiry and read permissions filled in.
 * The repository can't be filled in, so the steps say which one to pick. */
export function tokenUrl(owner: string): string {
  const params = new URLSearchParams({
    name: 'Aldermoor Live',
    description: 'Read-only access for the Aldermoor Live dashboard',
    target_name: owner,
    expires_in: '366',
    contents: 'read',
    issues: 'read',
    pull_requests: 'read',
    actions: 'read',
    metadata: 'read',
  });
  return `https://github.com/settings/personal-access-tokens/new?${params}`;
}

interface Options {
  repo: string;
  /** Why the last token stopped working, when one did. */
  message: string | null;
  /** Called with the permissions the token lacks for some sections (Actions, Contents). */
  onConnected(missing: string[]): void;
}

export function connectScreen({ repo, message, onConnected }: Options): HTMLElement {
  const [owner = '', name = ''] = repo.split('/');
  const input = h('input', {
    id: 'token',
    name: 'token',
    type: 'password',
    required: true,
    autocomplete: 'off',
    autocapitalize: 'none',
    spellcheck: 'false',
    placeholder: 'github_pat_…',
    'aria-describedby': 'token-help',
  });
  const error = h('p', { class: 'form-error', role: 'alert' }, message ?? '');
  error.hidden = !message;
  const button = h('button', { type: 'submit', class: 'button primary' }, 'Connect');

  const submit = async (event: Event) => {
    event.preventDefault();
    error.hidden = true;
    button.disabled = true;
    button.textContent = 'Checking…';
    try {
      const res = await fetch('/api/session', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ token: input.value.trim() }),
      });
      const body = (await res.json().catch(() => ({}))) as { connected?: boolean; missing?: string[]; message?: string };
      if (res.ok && body.connected) {
        input.value = '';
        onConnected(body.missing ?? []);
        return;
      }
      error.textContent = body.message ?? `The dashboard answered ${res.status}. Try again in a moment.`;
    } catch {
      error.textContent = "Couldn't reach the dashboard. Check your connection and try again.";
    }
    error.hidden = false;
    button.disabled = false;
    button.textContent = 'Connect';
  };

  return h('div', { class: 'page connect' },
    h('section', { class: 'card connect-card' },
      h('h1', null, 'Connect GitHub'),
      h('p', { class: 'lede' },
        `Aldermoor Live reads ${repo} with a read-only token. It stays in this browser, in a cookie the page itself can't read, and only ever goes to GitHub.`),
      h('ol', { class: 'steps' },
        h('li', null,
          h('h2', null, 'Make a read-only token'),
          h('p', null, 'GitHub opens with the name, expiry and read permissions filled in. Under ',
            h('strong', null, 'Repository access'), ' choose ', h('strong', null, 'Only select repositories'),
            ` and pick ${name}. Then generate the token and copy it.`),
          link(tokenUrl(owner), { class: 'button' }, 'Open GitHub', icon('external'))),
        h('li', null,
          h('h2', null, h('label', { for: 'token' }, 'Paste it here')),
          h('form', { class: 'token-form', onsubmit: submit }, input, button),
          error,
          h('p', { class: 'hint', id: 'token-help' },
            'It starts with github_pat_. You can revoke it on GitHub at any time, and Disconnect on the overview forgets it here.')))));
}
