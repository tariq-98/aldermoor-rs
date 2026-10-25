# Working on Aldermoor as an agent

Several agents work on this repo at once: Claude project threads, local Claude Code sessions and
Codex. GitHub issues are the source of truth for who is doing what. Follow these rules so nobody
works on the same thing twice.

Start with [docs/PLAN.md](docs/PLAN.md) for the design, crate ownership and milestones.

## Before you start

1. Pull the latest `main`.
2. Find an open issue labelled `status:ready`, ideally `p0` or `p1` in the current milestone.
   If the work you were asked to do has no issue, create one (use the Task template) before
   starting.
3. Check open `status:claimed` issues for overlap with the files your issue lists. If they
   overlap, comment on both issues and agree who goes first rather than racing.

## Claim it

Every agent posts as the same GitHub user, so the assignee field cannot tell agents apart.

1. Add `status:claimed` and remove `status:ready`.
2. Comment `Claimed by <agent>`, where `<agent>` says who you are and where, for example
   `thread: Give the valley real materials`, `local: aldermoor-look checkout` or `codex`.
3. Read the comments again. If someone else claimed it earlier and has not released it, the
   earlier claim wins: remove your label change, comment that you backed off, and pick another.

## Do the work

- One branch per issue. When you choose the name, use `<issue number>-<short-slug>`, for example
  `12-camera-smoothing`. Tools that name branches themselves (`codex/...`, `claude/...`) are fine:
  the link to the issue is `Closes #<n>` in the PR body. Never commit issue work straight to `main`.
- Push work in progress to the branch at least at the end of each session.
- If you are blocked, add `status:blocked` and comment why.
- CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests on Linux for the portable crates and
  on macOS for `render` and `client`. Keep it green; run the same commands locally before pushing.
  `perf.yml` reports sim step times and never blocks a merge.
- Issues labelled `perf` need before and after numbers against the budgets in
  [docs/RENDERER.md](docs/RENDERER.md) (12 ms GPU on the M1 Air, 2 ms sim step).
- Issues labelled `needs-mac` must be checked on the Mac. A cloud agent can do the code but must
  say in the PR that it was not run.

## Finish or hand back

- Open a draft PR whose body contains `Closes #<n>`. The Tracking workflow then sets
  `status:in-review` on the issue (and sets it back to `status:ready` if the PR closes unmerged), so
  agents without GitHub access still keep the board right.
- **Merge it yourself once it's tested and working** (Chippy's standing rule, 25 October). That
  means all of: CI green on the latest commit; you ran the checks for what you changed and they
  passed; anything `needs-mac` or visual was actually run on the Mac and looked right; no open
  review comments or requested changes; no `status:needs-decision` issue blocking it; and the PR is
  mergeable without conflicts. Then mark it ready for review and squash-merge. If any of those
  isn't true, leave the PR open and say in it exactly what's missing.
- If you stop before finishing, comment `Released: <what is done, what is left>` and set the
  label back to `status:ready`.
- A claim with no comment, push or PR activity for 3 days can be taken over: comment that you are
  taking it, then claim it as above.

## Ownership

From [docs/PLAN.md](docs/PLAN.md): `area:render` issues belong to Codex unless the issue says
otherwise. Art, icons, look values and texture compression always come from Claude.

**Codex never creates or edits assets, and never works in Blender.** That means no changes to
`art/` (the Blender pipeline), `assets/`, `crates/assets/` or `tools/`, no `.blend`, `.glb`,
`.gltf`, `.ktx2` or image files anywhere, and no `area:art` or `area:assets` issues. If renderer
work needs an asset or a look value that doesn't exist, Codex files an issue for Claude
(`area:art`, with what it needs and why) and works around it or waits. The Codex guard check fails
any `codex/` branch PR that touches those paths.

## Labels

- Status: `status:ready`, `status:claimed`, `status:blocked`, `status:in-review`,
  `status:needs-decision` (waiting on Chippy).
- Area, matching crates: `area:sim`, `area:client`, `area:render`, `area:art`, `area:world`,
  `area:content`, `area:server`, `area:ai`, `area:assets`, `area:infra`, `area:docs`.
- Type: `type:feature`, `type:bug`, `type:chore`, `type:spike`, `type:epic`.
- Priority: `p0` now, `p1` this phase, `p2` later.
- Phase: `phase:1` to `phase:6`, the roadmap phases. The Tracking workflow files each issue under
  the matching milestone.
- `perf`, `needs-mac`.

Label colours, descriptions and milestones live in [.github/tracking.json](.github/tracking.json).
Edit that file, not the GitHub UI, and the Tracking workflow applies it on merge.

## Step by step

Skills for each step live in [.claude/skills](.claude/skills): `claim-issue`, `file-issue` and
`finish-issue`. Claude Code loads them from there and Codex from `.agents/skills`, which links to
the same folder. They cover both the `gh` CLI and the GitHub MCP tools.

## Codex

Codex owns `area:render` issues (see Ownership) and can pick up others when the issue says so.

- **Codex CLI** (on the Mac): needs `gh auth login` once, and network for `gh` (set
  `network_access = true` under `[sandbox_workspace_write]` in `~/.codex/config.toml`, or approve
  the command). Then claim and finish exactly as above, signing claims `codex`. Name branches
  `codex/<issue number>-<short-slug>` so the Codex guard check applies.
- **Codex cloud** has no GitHub access while it works. Start the task with the issue number
  ("Work on #12"), and whoever starts it posts `Claimed by codex (cloud)` on the issue. Codex
  writes `Closes #12` in the PR description; the Tracking workflow moves the labels from there. Mac-only
  crates (`render`, `client`, `apple`) can't build there, so say what wasn't run.

## Code Review Rules

For Codex reviews and any other reviewer. Formatting and lint are CI's job; flag these:

- **Untracked work.** The PR body must say `Closes #<n>` (or link the issue it is part of), so the
  tracker stays the source of truth.
- **Missing performance numbers.** A PR for a `perf` issue, or one that adds GPU passes, draw calls
  or per-frame allocations, must report before and after timings against the 12 ms GPU and 2 ms
  sim budgets in docs/RENDERER.md.
- **Codex touching assets.** A PR from Codex that changes `art/`, `assets/`, `crates/assets/`,
  `tools/`, or any `.blend`, `.glb`, `.gltf`, `.ktx2` or image file is out of bounds, whatever the
  change. Assets come from Claude.
- **Look values in renderer code.** Colours, fog, exposure, material constants and similar belong
  in look settings or assets authored by Claude, not hard-coded in `crates/render`.
- **Broken determinism.** `crates/sim` must not read wall-clock time, use unseeded randomness or
  iterate hash maps in an order that affects results; the sim is a fixed 60 Hz deterministic step.

## Wiki

The GitHub wiki is published from [wiki/](wiki/) on every merge to `main`. Edit pages there, not
on the wiki. When Chippy answers a `status:needs-decision` issue, add a row to
[wiki/Decisions-log.md](wiki/Decisions-log.md) in the same PR that acts on it.
