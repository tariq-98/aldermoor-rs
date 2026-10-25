# Aldermoor Live

A phone-first dashboard of everything happening on GitHub for Aldermoor. GitHub is the only source
of truth: there is no database, and nothing here changes the repo.

- **Overview**: what needs Chippy (issues labelled `status:needs-decision`, then blocked ones),
  main's latest commit and its CI, open PRs, progress through each phase, the latest captures and
  an activity feed of everything agents do.
- **PRs**: open, merged and closed, with each workflow's result, the agent behind it, the issues it
  closes and its captures.
- **Issues**: by status and grouped by phase, with who has claimed each one and their plan.
- **Visuals**: every screenshot and video posted with the attach-visuals skill, newest first,
  full screen with swipe. Captures taken on an older commit than the PR's latest are flagged.

While the page is open it refreshes every 15 seconds. A refresh where nothing changed is an empty
304, and the server asks GitHub with ETags, which GitHub doesn't count against the rate limit.
Switching back to the tab or app refreshes it at once.

## How it works

| Part | What it does |
|---|---|
| `static/`, `src/client/` | The page: plain TypeScript and CSS, no framework, bundled by esbuild. |
| `src/api/state.ts` | `GET /api/state` builds the snapshot from GitHub's REST API (issues, PRs, comments, milestones, main's commits, Actions runs) and returns it with an ETag. |
| `src/api/media.ts` | `GET /api/media` passes captures through from the private repo, in ranges for video; big files redirect to GitHub's signed URLs. |
| `src/api/session.ts` | Connects this browser with a read-only token, or disconnects it. |
| `src/server/` | GitHub client, claims, CI, visuals and snapshot logic. |
| `build.mjs` | Writes `.vercel/output` in Vercel's Build Output API format. |

## Deploy on Vercel

Import the repo as a Vercel project with **Root Directory** `dashboard`, **Framework Preset**
Other and **Node.js** 22.x. `vercel.json` sets the install and build commands and skips a
deployment when a commit doesn't touch `dashboard/`.

Environment variables, both optional:

- `DASHBOARD_REPO`: the repo to show, `tariq-98/aldermoor-rs` by default.
- `GITHUB_TOKEN`: a token for every visitor. **Leave it unset** unless every deployment URL,
  production included, is behind Vercel Authentication. With it set, anyone who has the URL can
  read the private repo through the dashboard. Without it, each browser connects its own token.

## Connect a phone

The first visit shows a connect screen. **Open GitHub** opens the new-token page already filled
in: a fine-grained token for `aldermoor-rs` only, read-only on Contents, Issues, Pull requests,
Actions and Metadata. Create it and paste it into the dashboard, which checks it and keeps it in
an HttpOnly cookie for that browser (page scripts can't read it, and it is only sent to `/api`).
**Disconnect** at the bottom of Overview forgets it; revoking it on GitHub works too.

Without Actions access the CI results are missing, and without Contents the commits and captures.
On iPhone, Share then **Add to Home Screen** opens it full screen like an app.

## Run it locally

```sh
cd dashboard
npm ci
GITHUB_TOKEN=$(gh auth token) npm run dev   # http://localhost:3000
```

`npm run dev` builds the site and serves it the way Vercel does. In a Claude Code cloud session,
where a proxy signs GitHub requests, use `NODE_USE_ENV_PROXY=1 DASHBOARD_DEV_ANONYMOUS=1 npm run
dev` instead.

`npm run check` type-checks, `npm test` runs the tests (one of them round-trips a post through
`scripts/attach_visuals.py`, so it needs `python3`) and `npm run build` builds. CI runs all three
on any PR that touches the dashboard (`.github/workflows/dashboard.yml`).

## Visuals

Agents post captures with the attach-visuals skill (`.claude/skills/attach-visuals/SKILL.md`),
which runs `scripts/attach_visuals.py`. The files go to the `captures` branch, and the comment
carries a hidden `aldermoor-visuals` manifest that the Visuals tab reads. Older comments that only
link images on `captures` are read from their links and the bold captions above them.

## Privacy

The page loads nothing from other sites: no fonts, scripts or analytics. It sends a strict
Content-Security-Policy, asks search engines not to index it, and every API response is private
and uncached.
