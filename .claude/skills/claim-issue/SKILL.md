---
name: claim-issue
description: Find and claim an Aldermoor GitHub issue before changing code. Use at the start of any coding task in this repo, or when asked to pick up the next piece of work.
---

# Claim an issue

Several agents work on this repo at once. Claim an issue before you edit anything, so nobody does
the same work twice. The rules are in [AGENTS.md](../../../AGENTS.md); this is the how.

Repo: `tariq-98/aldermoor-rs`. Cloud sessions use the GitHub MCP tools (`mcp__github__*`); local
sessions use the `gh` CLI. Commands for both are below.

## 1. Pull and look

```sh
git fetch origin main && git checkout main && git pull --ff-only
```

If you were given a task, search for an existing issue first:

- MCP: `search_issues` with `repo:tariq-98/aldermoor-rs is:open <keywords>`
- gh: `gh issue list --search "<keywords>"`

No issue? Use the `file-issue` skill to create one, then come back.

If you were asked to pick the next piece of work, list ready issues and prefer `p0`, then `p1`,
lowest phase first. Skip `needs-mac` if you are a cloud session, `type:epic` always, and
`area:render` unless you are Codex or the issue says otherwise. Codex never takes `area:art` or
`area:assets` issues, or any issue that needs Blender or new or changed asset files.

- MCP: `list_issues` with `labels: ["status:ready"]`, `state: OPEN`
- gh: `gh issue list --label status:ready`

## 2. Check for overlap

Read the issue's "Files or crates expected to change". Then list what is already claimed and
compare:

- MCP: `list_issues` with `labels: ["status:claimed"]`, then read bodies
- gh: `gh issue list --label status:claimed --json number,title,body`

Also check open PRs (`list_pull_requests` / `gh pr list`) for branches touching the same files. If
anything overlaps, comment on both issues saying what you plan to change and wait for, or agree on,
an order. Don't race.

## 3. Claim

Write your agent name as `<kind>: <where>`: `thread: <thread title>` for a project thread,
`local: <checkout folder>` for a local session, `codex` for Codex.

- MCP:
  - `issue_write` method `update` with the issue's labels minus `status:ready` plus `status:claimed`
    (send the full list; it replaces the labels).
  - `add_issue_comment`: `Claimed by <agent>. Plan: <one line>. Branch: <n>-<slug>`
- gh:
  ```sh
  gh issue edit <n> --remove-label status:ready --add-label status:claimed
  gh issue comment <n> --body "Claimed by <agent>. Plan: <one line>. Branch: <n>-<slug>"
  ```

End any comment you post through the MCP tools with the Claude Code footer the session requires.

## 4. Confirm you won

Read the issue's comments again (`issue_read` method `get_comments` / `gh issue view <n> --comments`).
If an earlier `Claimed by` exists with no later `Released`, you lost: comment `Backing off, <agent>
claimed first`, leave the label as it is, and pick another issue.

## 5. Branch

```sh
git checkout -b <n>-<short-slug>     # e.g. 12-camera-smoothing; Codex: codex/12-camera-smoothing
```

In a session that was told to use a specific branch name, use that branch, and put the issue
number in the branch description and PR instead.

Now do the work. Push to the branch at least at the end of each session. When done or stopping,
use the `finish-issue` skill.
