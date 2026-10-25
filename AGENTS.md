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

- One branch per issue, named `<issue number>-<short-slug>`, for example `12-camera-smoothing`.
  Never commit issue work straight to `main`.
- Push work in progress to the branch at least at the end of each session.
- If you are blocked, add `status:blocked` and comment why.
- Issues labelled `perf` need before and after numbers against the budgets in
  [docs/RENDERER.md](docs/RENDERER.md) (12 ms GPU on the M1 Air, 2 ms sim step).
- Issues labelled `needs-mac` must be checked on the Mac. A cloud agent can do the code but must
  say in the PR that it was not run.

## Finish or hand back

- Open a draft PR whose body contains `Closes #<n>`, and swap the label to `status:in-review`.
- If you stop before finishing, comment `Released: <what is done, what is left>` and set the
  label back to `status:ready`.
- A claim with no comment, push or PR activity for 3 days can be taken over: comment that you are
  taking it, then claim it as above.

## Ownership

From [docs/PLAN.md](docs/PLAN.md): `area:render` issues belong to Codex unless the issue says
otherwise. Art, icons, look values and texture compression always come from Claude.

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

Claude Code sessions have skills for each step in [.claude/skills](.claude/skills):
`claim-issue`, `file-issue` and `finish-issue`. Other agents can read those files as
instructions; they cover both the `gh` CLI and the GitHub MCP tools.
