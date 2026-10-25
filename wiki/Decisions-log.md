# Decisions log

Choices that shape the game, newest first. Open questions are issues labelled
[`status:needs-decision`](https://github.com/tariq-98/aldermoor-rs/issues?q=is%3Aopen+label%3Astatus%3Aneeds-decision).
When one is answered, add it here and close the issue.

| Date | Decision | Why | Issue |
|---|---|---|---|
| 2026-10-25 | The budget is measured at 75% render scale (1920×1200 internal, about 2.3 MP) on the Air's 2560×1600 panel; every budget lives in docs/PERFORMANCE.md | Native at 100% is 4.1 MP, more work than the ultrawide at 65%; 75% with TAA still looks sharp on the Retina panel | #36, #30 |
| 2026-10-25 | Agents merge their own PRs once tested and working, without asking | Chippy doesn't want to be the bottleneck; AGENTS.md lists what "tested and working" means | |
| 2026-10-25 | Codex never creates or edits assets or works in Blender | Assets and look come from Claude; Codex sticks to renderer code. Enforced by the Codex guard check on `codex/` branches | |
| 2026-10-25 | 60 fps / 12 ms GPU is measured on the M1 Air's built-in display at native resolution; 5120×1440 is best effort | Chippy set the ultrawide aside; effects and scatter stay in scope, priced against this target. Render scale: see the 75% row above | #36 |
| 2026-10-25 | Classic (WoW-style) is the default camera and movement scheme | Matches the WoW and Guild Wars feel; action camera stays as an option | #22 (guard key still open) |
| 2026-10-25 | MetalFX stays opt-in, not the main performance lever | Measured at about 2x the cost of the custom TAA on the M1 (22.7 vs 11.1 ms) | #10 |
| 2026-10-25 | GitHub issues are the source of truth for agent work | Lets several agents work in parallel without clashing | #24 |
| 2026-10-24 | Claude makes all art; Codex owns the renderer and doesn't invent look values | Set in docs/PLAN.md | |
