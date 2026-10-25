---
name: file-issue
description: Create a well-formed Aldermoor GitHub issue (task, bug, follow-up or decision) with the right labels. Use when work has no issue yet, when you find a bug or follow-up outside your current issue, or when a choice needs Chippy.
---

# File an issue

Every piece of work in `tariq-98/aldermoor-rs` has an issue, so other agents can see it and avoid
it. Search first (`search_issues` / `gh issue list --search`) so you don't file a duplicate.

## Body

Use these headings. "Files or crates expected to change" matters most: it is how other agents
spot overlap.

```markdown
## Goal
What this changes for the player or the project, in a sentence or two.

## Files or crates expected to change
crates/client/src/hud.rs, crates/protocol

## Done when
Concrete, checkable result.

## Performance impact
Expected cost against the 12 ms GPU and 2 ms sim budgets, or "None expected".

## Notes
Links, dependencies ("Blocked on #5"), related issues.
```

For a bug, use: What happens, What should happen, How to reproduce, Files likely involved.

## Labels

Pick one from each group that applies (the full list with meanings is in `.github/tracking.json`):

- Status: `status:ready` for new work. `status:needs-decision` if Chippy must choose first.
  If you are about to start it yourself, file it as `status:ready` and then claim it with the
  `claim-issue` skill, so the claim comment exists.
- Area: `area:sim`, `area:client`, `area:render`, `area:art`, `area:world`, `area:content`,
  `area:server`, `area:ai`, `area:assets`, `area:infra`, `area:docs`.
- Type: `type:feature`, `type:bug`, `type:chore`, `type:spike`, `type:epic`.
- Priority: `p0` now, `p1` this phase, `p2` later.
- Phase: `phase:1` to `phase:6` from the roadmap. A workflow files the issue under the matching
  milestone automatically.
- `perf` if it can move frame or step time; `needs-mac` if it can only be verified on the Mac.

## Create

- MCP: `issue_write` method `create`, `owner: tariq-98`, `repo: aldermoor-rs`, with `title`, `body`,
  `labels`.
- gh: `gh issue create --title "..." --body-file body.md --label status:ready,area:client,type:feature,p1,phase:1`

Titles say what the result is, in plain words: "Cast bar with interrupt feedback", not
"HUD changes".

## Decisions

When something needs Chippy's call, file it as `Decision: <question>` with `status:needs-decision`,
list the options, give a recommendation, and link the issues it blocks. Don't build on a guess
that would be expensive to undo.
