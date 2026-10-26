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
6. If the PR changes anything you can see, post captures of its latest commit on the PR with the
   `attach-visuals` skill. Without the Mac, ask for a Mac check through the coordinator, or say in
   the PR what needs capturing.

7. **Merge it when it's tested and working.** Chippy doesn't want to be asked. Check every item:
   - CI is green on the PR's latest commit (wait for it; don't merge while checks run).
   - The checks you ran for what you changed passed, and the PR says what you ran.
   - `needs-mac`, renderer or visual changes were actually run on the Mac and looked right, and
     their captures are on the PR. A cloud session that couldn't run them does **not** merge; it
     says so in the PR and stops.
   - No unresolved review comments or requested changes, and no open `status:needs-decision`
     issue that this PR depends on.
   - No merge conflicts (if there are, merge `main` in, re-test, and start this list again).

   Then mark it ready for review and squash-merge (MCP: `update_pull_request` with `draft: false`,
   then `merge_pull_request` with `merge_method: squash`; gh: `gh pr ready <pr>` then
   `gh pr merge <pr> --squash --delete-branch`). Merging closes the issue. If any item fails, leave
   the PR open and comment exactly what's missing.
8. **Clean up after the merge.** Delete everything the issue made as soon as it's merged:
   - the checkout's `target/` (or `cargo clean`);
   - any worktree or clone you made for this issue (`git worktree remove <path>`; a checkout you
     reuse for your next issue can stay, minus its `target/`);
   - its captures and other scratch files. Keep those in a temp folder (your scratchpad or
     `$TMPDIR`) from the start, never in the repo or `~/code`.

   Never a `CARGO_TARGET_DIR` shared between agents (cargo's lock would make their builds wait on
   each other). On the Mac a launchd job also deletes `target/` folders idle for 2 hours when the
   disk runs low, but don't rely on it.

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
