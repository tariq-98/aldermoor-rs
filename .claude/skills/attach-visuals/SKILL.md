---
name: attach-visuals
description: Post screenshots or videos of a visible Aldermoor change to its PR or issue in the standard format the dashboard reads. Use whenever work changes something you can see (looks, terrain, effects, HUD, animation), for every Mac check, and when asked to attach captures.
---

# Attach visuals

Chippy reviews most work from a phone, in the dashboard (`dashboard/`). Its Visuals tab collects
every capture posted with this skill, so a change nobody can see there is a change Chippy can't
review. Post visuals for anything you can see, on the PR that makes the change (or on the issue, for
findings before there is a PR).

## What to capture

- The change on the PR's **latest commit**. Captures of main taken before the change don't count.
- For a before and after, render main and the branch from the same camera and put them side by
  side in one image, main on the left. Say so in the caption.
- Stills as JPEG. Motion as MP4 (H.264), under a minute and under 25 MB:
  `ffmpeg -i in.mov -vf scale=1920:-2 -c:v libx264 -crf 26 -an out.mp4`.
- Name files after what they show (`east-cliff-west-face.jpg`, not `capture3.jpg`), and write
  captions that say what to look at.

## Post them

`scripts/attach_visuals.py` does it in one go: it uploads the files and their phone-sized
thumbnails to the `captures` branch in one commit (through the API, so never as LFS pointers), then
posts the comment. It needs `gh` signed in; on the Mac and in cloud sessions with a proxied `gh`
that is already true.

```sh
scripts/attach_visuals.py --pr 54 --title "Mac check: limestone repeat" --verdict pass \
  --head 5f5e4d7 --base 7286aa4 --by "thread: Run Mac tasks from home folder" \
  --item out/east-cliff.jpg "East cliff, west face. Left main, right this branch" \
  --item out/fight.mp4 "Wolf fight with the new effects" \
  --note-file findings.md
```

- `--pr` or `--issue`: where to post. Files go to `pr<n>/` or `issue<n>/` on `captures`.
- `--title`: a short heading. `--verdict pass|fail` for a check, `info` (the default) otherwise.
- `--head`: the commit the captures show. The dashboard flags captures whose head is not the PR's
  latest commit. `--base`: what it is compared with, for a before and after.
- `--by`: who you are, written as in claims (`thread: <title>`, `local: <checkout>`, `codex`).
- `--item FILE CAPTION`, once per file, in reading order. `--note` or `--note-file` adds markdown
  (findings, what to look at, what is still wrong) below the captures.
- `--dry-run` prints the comment without uploading anything.

To correct a post, post a new one; the dashboard shows every post, newest first.

## Without gh

The GitHub MCP file tools only take text, so they can't upload images. Hand the captures to a
session that has `gh`, or, for Mac renders, ask for a Mac check through the coordinator. If the
files are already on `captures`, post the comment yourself with `add_issue_comment`, in the format
below.

## The format

A visuals comment starts with a manifest in an HTML comment. GitHub hides it; the dashboard reads
it. The rest of the comment is ordinary markdown that shows the same captures on GitHub.

```markdown
<!-- aldermoor-visuals {"v":1,"title":"Mac check: limestone repeat","verdict":"pass","head":"5f5e4d7","base":"7286aa4","by":"thread: Run Mac tasks from home folder","ref":"<captures commit SHA>","items":[{"path":"pr54/east-cliff.jpg","caption":"East cliff, west face. Left main, right this branch","thumb":"pr54/east-cliff.thumb.jpg"}]} -->
**Visuals: Mac check: limestone repeat** (PASS) · head `5f5e4d7` · compared with `7286aa4`

**East cliff, west face. Left main, right this branch**

![East cliff, west face. Left main, right this branch](https://github.com/tariq-98/aldermoor-rs/blob/<ref>/pr54/east-cliff.jpg?raw=true)
```

| Field | Meaning |
|---|---|
| `v` | Format version, `1`. |
| `title` | Short heading. |
| `verdict` | `pass`, `fail` or `info`. |
| `head`, `base` | Optional. The commit shown and the commit it is compared with. |
| `by` | Optional. Who captured it. |
| `ref` | The `captures` commit that holds the files. Links pin to it, so later uploads with the same names don't change old posts. |
| `items` | In order: `path` on `captures`, `caption`, and optional `thumb` (a smaller JPEG; a video's poster frame). |

Images are `.jpg`, `.jpeg`, `.png`, `.webp` or `.gif`; videos are `.mp4`, `.mov` or `.webm`. The
manifest is JSON on one line, and any `--` inside it is written `-\u002d` so the HTML comment
can't end early. The dashboard also picks up older comments that link images on the `captures` branch
directly, but new posts use this format.
