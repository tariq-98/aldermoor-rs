# How agents work here

Several agents work on Aldermoor at once: Claude project threads, local Claude Code sessions on the
Mac, and Codex. They all post to GitHub as the same user, so the issue tracker, not the assignee
field, says who is doing what.

The full rules are in [AGENTS.md](https://github.com/tariq-98/aldermoor-rs/blob/main/AGENTS.md).
The short version:

1. **Every piece of work has an issue.** No issue, no code. File one with the Task template.
2. **Claim before you start.** Swap `status:ready` for `status:claimed` and comment
   `Claimed by <agent>`. The earliest unreleased claim wins.
3. **Check for overlap.** Each issue lists the files it expects to change. Compare with other
   claimed issues and open PRs first.
4. **One branch per issue**, named `<number>-<slug>` when you pick the name.
5. **Finish with a draft PR** that says `Closes #<number>`. The Tracking workflow moves the issue
   to `status:in-review`, and back to `status:ready` if the PR closes unmerged.
6. **Stopping early?** Comment `Released: ...` and set `status:ready` again.
7. **Stale claims** (3 days with no activity) can be taken over after a comment.

Claude Code and Codex share step-by-step skills in
[`.claude/skills`](https://github.com/tariq-98/aldermoor-rs/tree/main/.claude/skills)
(Codex reads them through `.agents/skills`): `claim-issue`, `file-issue` and `finish-issue`.

## Codex

- **Codex CLI** on the Mac works like a local Claude session once `gh` is signed in.
- **Codex cloud** can't reach GitHub while it works: start it with "Work on #n", post
  `Claimed by codex (cloud)` on the issue, and its PR's `Closes #n` does the rest.
- **Codex reviews** follow the Code Review Rules in AGENTS.md.

## Labels at a glance

- `status:*` is where an issue is. `status:needs-decision` waits on Chippy.
- `area:*` matches the crates. `area:render` belongs to Codex by default.
- `phase:1` to `phase:6` files the issue under the matching milestone automatically.
- `perf` means before and after numbers are required. `needs-mac` means a cloud agent can't verify it.

Labels and milestones are defined in
[`.github/tracking.json`](https://github.com/tariq-98/aldermoor-rs/blob/main/.github/tracking.json).
