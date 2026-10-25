# Decisions log

Choices that shape the game, newest first. Open questions are issues labelled
[`status:needs-decision`](https://github.com/tariq-98/aldermoor-rs/issues?q=is%3Aopen+label%3Astatus%3Aneeds-decision).
When one is answered, add it here and close the issue.

| Date | Decision | Why | Issue |
|---|---|---|---|
| 2026-10-25 | Codex never creates or edits assets or works in Blender | Assets and look come from Claude; Codex sticks to renderer code. Enforced by the Codex guard check on `codex/` branches | |
| 2026-10-25 | 60 fps / 12 ms GPU is measured on the M1 Air's built-in display at native resolution; 5120×1440 is best effort | Chippy set the ultrawide aside; effects and scatter stay in scope, priced against this target. Render scale for the budget is still open | #36 |
| 2026-10-25 | Classic (WoW-style) is the default camera and movement scheme | Matches the WoW and Guild Wars feel; action camera stays as an option | #22 (guard key still open) |
| 2026-10-25 | MetalFX stays opt-in, not the main performance lever | Measured at about 2x the cost of the custom TAA on the M1 (22.7 vs 11.1 ms) | #10 |
| 2026-10-25 | GitHub issues are the source of truth for agent work | Lets several agents work in parallel without clashing | #24 |
| 2026-10-24 | Claude makes all art; Codex owns the renderer and doesn't invent look values | Set in docs/PLAN.md | |
