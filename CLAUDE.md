# Claude Code on Aldermoor

Other agents may be working on this repo right now. Follow [AGENTS.md](AGENTS.md):

- Before changing code, claim a GitHub issue with the `claim-issue` skill.
- Work on one branch per issue.
- When done or stopping, use the `finish-issue` skill: a draft PR that closes the issue, or a
  release comment. Merge it yourself once it is tested and working (the skill lists what that
  means); don't ask Chippy first.
- Work you discover outside your issue goes in a new issue via the `file-issue` skill.
- Anything you can see that changes (looks, effects, HUD, animation) gets captures of the latest
  commit on its PR via the `attach-visuals` skill. Chippy reviews them in the dashboard.
