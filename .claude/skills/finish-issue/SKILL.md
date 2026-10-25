---
name: finish-issue
description: Hand off an Aldermoor issue you claimed, by opening a PR that closes it, releasing it, or marking it blocked. Use when your work on an issue is done or when you stop before finishing.
---

# Finish or hand back an issue

Never leave a claimed issue silent. Pick the case that fits.

## Done: open a PR

1. Run what you can: `cargo fmt`, `cargo clippy`, `cargo test` for the crates you touched. On a
   cloud session, Mac-only crates (`render`, `client`, `apple`) may not build; say so.
2. For `perf` issues, record before and after numbers (see docs/RENDERER.md for how).
3. Push the branch and open a **draft** PR against `main`:
   - Body follows `.github/pull_request_template.md` and starts with `Closes #<n>`.
   - MCP: `create_pull_request` with `draft: true`. gh: `gh pr create --draft --fill`, then edit.
4. The Tracking workflow swaps the issue to `status:in-review` when the PR opens. If it hasn't
   after a minute (for example the PR body was missing `Closes #<n>` at first), set it yourself
   (`issue_write` update with the full label list / `gh issue edit <n> --remove-label
   status:claimed --add-label status:in-review`).
5. Comment on the issue with the PR link.

Merging the PR closes the issue. Don't merge your own PR unless Chippy asked you to.

## Stopping before it's done: release

Comment, then put it back on the shelf:

```
Released by <agent>. Done: <what works, branch name>. Left: <what remains>. Gotchas: <anything the next agent should know>.
```

Swap `status:claimed` for `status:ready`. Push the branch first so the next agent can build on it.

## Stuck: blocked

Add `status:blocked` (keep `status:claimed`) and comment what you need and from whom. If it's a
choice for Chippy, file a `Decision:` issue with the `file-issue` skill and link it.

## Found more work

Anything outside this issue's scope goes in a new issue (`file-issue` skill), linked from your PR.
Don't widen the PR.
