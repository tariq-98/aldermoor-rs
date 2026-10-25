#!/usr/bin/env python3
"""Post screenshots and videos to an Aldermoor issue or PR in the format the dashboard reads.

Uploads the files to the `captures` branch in one commit (with phone-sized thumbnails), then posts
a comment that shows them on GitHub and carries a hidden `aldermoor-visuals` manifest for the
dashboard's Visuals tab. Needs `gh` signed in with write access, as on the Mac; a Claude cloud
session's GitHub proxy refuses the upload. Stdlib only, so the Mac's system python3 runs it. The
format is described in .claude/skills/attach-visuals.

    scripts/attach_visuals.py --pr 54 --title "Mac check: limestone repeat" --verdict pass \\
        --head 5f5e4d7 --base 7286aa4 --by "thread: Run Mac tasks from home folder" \\
        --item out/east-cliff.jpg "East cliff, west face. Left main, right this branch" \\
        --item out/fight.mp4 "Wolf fight with the new effects" --note-file findings.md
"""
from __future__ import annotations

import argparse, base64, json, os, re, shutil, subprocess, sys, tempfile
from pathlib import Path

REPO = os.environ.get('ALDERMOOR_REPO', 'tariq-98/aldermoor-rs')
IMAGES = {'.jpg', '.jpeg', '.png', '.webp', '.gif'}
VIDEOS = {'.mp4', '.mov', '.webm'}
THUMB_WIDTH = 800
MAX_BYTES = 50 * 1024 * 1024  # GitHub refuses blobs near 100 MB once base64 has grown them.
WARN_BYTES = 25 * 1024 * 1024
MARKER = 'aldermoor-visuals'


def fail(message: str) -> None:
    sys.exit(f'attach_visuals: {message}')


def gh(path: str, payload: dict | None = None, method: str | None = None) -> dict:
    """One GitHub REST call through `gh api`; JSON in, JSON out."""
    cmd = ['gh', 'api', path]
    if method:
        cmd += ['-X', method]
    if payload is not None:
        cmd += ['--input', '-']
    run = subprocess.run(cmd, input=json.dumps(payload).encode() if payload is not None else None,
                         capture_output=True)
    if run.returncode != 0:
        raise RuntimeError(f'gh api {path}: {run.stderr.decode().strip() or run.stdout.decode().strip()}')
    return json.loads(run.stdout or b'{}')


def slug(name: str) -> str:
    stem = re.sub(r'[^a-z0-9._]+', '-', name.lower()).strip('-.')
    return stem or 'capture'


def make_thumb(src: Path, dst: Path) -> bool:
    """A JPEG at most THUMB_WIDTH wide for the dashboard's grid; videos get a poster frame."""
    if src.suffix.lower() in VIDEOS:
        if not shutil.which('ffmpeg'):
            return False
        run = subprocess.run(['ffmpeg', '-v', 'error', '-y', '-ss', '0.5', '-i', str(src), '-frames:v', '1',
                              '-vf', f"scale='min({THUMB_WIDTH},iw)':-2", '-q:v', '5', str(dst)], capture_output=True)
        return run.returncode == 0 and dst.exists()
    if shutil.which('sips'):  # macOS
        width = subprocess.run(['sips', '-g', 'pixelWidth', str(src)], capture_output=True, text=True).stdout
        found = re.search(r'pixelWidth: (\d+)', width)
        args = ['--resampleWidth', str(THUMB_WIDTH)] if found and int(found.group(1)) > THUMB_WIDTH else []
        run = subprocess.run(['sips', '-s', 'format', 'jpeg', '-s', 'formatOptions', '72', *args, str(src),
                              '--out', str(dst)], capture_output=True)
        return run.returncode == 0 and dst.exists()
    try:
        from PIL import Image
    except ImportError:
        return False
    try:
        with Image.open(src) as image:
            image = image.convert('RGB')
            if image.width > THUMB_WIDTH:
                image = image.resize((THUMB_WIDTH, round(image.height * THUMB_WIDTH / image.width)), Image.LANCZOS)
            image.save(dst, 'JPEG', quality=72, optimize=True, progressive=True)
    except Exception as error:  # A thumbnail is a nicety; the capture still goes up without one.
        print(f'warning: no thumbnail for {src.name}: {error}', file=sys.stderr)
        return False
    return True


def md(text: str) -> str:
    """`text` as literal markdown, so a title or caption can't open a link, emphasis or an HTML
    comment that hides the rest of the post."""
    return re.sub(r'([\\`*_\[\]<>])', r'\\\1', text)


def manifest_comment(manifest: dict) -> str:
    """The manifest as an HTML comment. JSON can only hold `--` inside strings, so escaping it
    there keeps the comment from ending early."""
    text = json.dumps(manifest, ensure_ascii=False, separators=(',', ':')).replace('--', '-\\u002d')
    return f'<!-- {MARKER} {text} -->'


def comment_body(manifest: dict, sizes: dict, note: str, footer: str = '') -> str:
    ref = manifest['ref']
    blob = f'https://github.com/{REPO}/blob/{ref}/'
    head = [f"**Visuals: {md(manifest['title'])}**"]
    if manifest.get('verdict') in ('pass', 'fail'):
        head[0] += f" ({manifest['verdict'].upper()})"
    if manifest.get('head'):
        head.append(f"head `{manifest['head'][:7]}`")
    if manifest.get('base'):
        head.append(f"compared with `{manifest['base'][:7]}`")
    if manifest.get('by'):
        head.append(f"by {md(manifest['by'])}")
    parts = [manifest_comment(manifest), ' · '.join(head)]
    for item in manifest['items']:
        caption = md(item['caption'] or Path(item['path']).name)
        if Path(item['path']).suffix.lower() in VIDEOS:
            megabytes = sizes[item['path']] / 1e6
            line = f"**{caption}** · [video, {megabytes:.1f} MB]({blob}{item['path']})"
            if item.get('thumb'):
                line += f"\n\n[![{caption}]({blob}{item['thumb']}?raw=true)]({blob}{item['path']})"
            parts.append(line)
        else:
            parts.append(f"**{caption}**\n\n![{caption}]({blob}{item['path']}?raw=true)")
    if note.strip():
        parts.append(note.strip())
    parts.append('<sub>Posted with `scripts/attach_visuals.py`; the Aldermoor dashboard shows these under Visuals.</sub>')
    if footer.strip():
        parts.append(footer.strip())
    return '\n\n'.join(parts) + '\n'


def upload(files: dict, branch: str, message: str) -> str:
    """Adds `files` ({path in branch: local Path}) to `branch` in one commit; returns its SHA.
    Retries if someone else pushed to the branch in between."""
    blobs = {}
    for path, local in files.items():
        data = base64.b64encode(local.read_bytes()).decode()
        blobs[path] = gh(f'repos/{REPO}/git/blobs', {'content': data, 'encoding': 'base64'})['sha']
        print(f'  uploaded {path}', file=sys.stderr)
    for attempt in range(4):
        try:
            parent = gh(f'repos/{REPO}/git/ref/heads/{branch}')['object']['sha']
        except RuntimeError as error:
            if 'Not Found' not in str(error):
                raise
            parent = None
        tree = {'tree': [{'path': p, 'mode': '100644', 'type': 'blob', 'sha': s} for p, s in blobs.items()]}
        if parent:
            tree['base_tree'] = gh(f'repos/{REPO}/git/commits/{parent}')['tree']['sha']
        tree_sha = gh(f'repos/{REPO}/git/trees', tree)['sha']
        commit = gh(f'repos/{REPO}/git/commits',
                    {'message': message, 'tree': tree_sha, 'parents': [parent] if parent else []})['sha']
        try:
            if parent:
                gh(f'repos/{REPO}/git/refs/heads/{branch}', {'sha': commit, 'force': False}, method='PATCH')
            else:
                gh(f'repos/{REPO}/git/refs', {'ref': f'refs/heads/{branch}', 'sha': commit})
            return commit
        except RuntimeError as error:
            if attempt == 3:
                raise
            print(f'  {branch} moved while uploading, retrying ({error})', file=sys.stderr)
    raise AssertionError('unreachable')


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    target = p.add_mutually_exclusive_group(required=True)
    target.add_argument('--pr', type=int, help='PR to post on')
    target.add_argument('--issue', type=int, help='issue to post on')
    p.add_argument('--title', required=True, help='short heading, e.g. "Mac check: limestone repeat"')
    p.add_argument('--item', nargs=2, action='append', metavar=('FILE', 'CAPTION'), required=True,
                   help='a capture and what it shows; repeat for each file, in reading order')
    p.add_argument('--verdict', choices=['pass', 'fail', 'info'], default='info',
                   help='pass or fail for a check, info for plain captures (default)')
    p.add_argument('--head', help='commit the captures show, usually the PR head')
    p.add_argument('--base', help='commit it is compared with, e.g. main for a before and after')
    p.add_argument('--by', help='who captured them, as in claims: "thread: <title>", "local: <checkout>"')
    p.add_argument('--note', default='', help='markdown to add below the captures')
    p.add_argument('--note-file', type=Path, help='file of markdown to add below the captures')
    p.add_argument('--footer', default='', help='markdown to end the comment with, such as an attribution line')
    p.add_argument('--branch', default='captures', help='branch that stores captures (default captures)')
    p.add_argument('--dry-run', action='store_true', help='print the comment and upload nothing')
    a = p.parse_args()

    number = a.pr or a.issue
    folder = f"{'pr' if a.pr else 'issue'}{number}"
    note = a.note + ('\n\n' + a.note_file.read_text() if a.note_file else '')
    if not a.dry_run and not shutil.which('gh'):
        fail('needs the GitHub CLI (gh), signed in. Without it, see .claude/skills/attach-visuals.')

    work = Path(tempfile.mkdtemp(prefix='visuals-'))
    files, items, sizes, used = {}, [], {}, set()
    for name, caption in a.item:
        local = Path(name)
        if not local.is_file():
            fail(f'no such file: {local}')
        ext = local.suffix.lower()
        if ext not in IMAGES | VIDEOS:
            fail(f'{local.name}: use an image ({", ".join(sorted(IMAGES))}) or a video ({", ".join(sorted(VIDEOS))})')
        size = local.stat().st_size
        if size > MAX_BYTES:
            fail(f'{local.name} is {size / 1e6:.0f} MB; keep captures under 50 MB '
                 '(ffmpeg -i in.mov -vf scale=1920:-2 -c:v libx264 -crf 26 -an out.mp4)')
        if size > WARN_BYTES:
            print(f'warning: {local.name} is {size / 1e6:.0f} MB; a phone has to download all of it', file=sys.stderr)
        stem, n = slug(local.stem), 2
        while f'{stem}{ext}' in used:
            stem, n = f'{slug(local.stem)}-{n}', n + 1
        used.add(f'{stem}{ext}')
        path = f'{folder}/{stem}{ext}'
        files[path], sizes[path] = local, size
        item = {'path': path, 'caption': caption.strip()}
        thumb = work / f'{stem}.thumb.jpg'
        if ext != '.gif' and make_thumb(local, thumb):  # a still would lose a GIF's motion
            item['thumb'] = f'{folder}/{stem}.thumb.jpg'
            files[item['thumb']] = thumb
        items.append(item)

    manifest = {'v': 1, 'title': a.title.strip(), 'verdict': a.verdict}
    for key in ('head', 'base', 'by'):
        if getattr(a, key):
            manifest[key] = getattr(a, key).strip()
    manifest['ref'] = 'DRY-RUN' if a.dry_run else ''
    manifest['items'] = items

    if a.dry_run:
        print(comment_body(manifest, sizes, note, a.footer))
        print(f'would upload to {a.branch}: ' + ', '.join(files), file=sys.stderr)
        return
    try:
        manifest['ref'] = upload(files, a.branch, f'Captures for {"PR" if a.pr else "issue"} #{number}: {a.title}')
    except RuntimeError as error:
        fail(f'upload refused: {error}\nThis gh can\'t write to the repo (a cloud session\'s proxy never can). '
             'Post from the Mac instead; see "Without gh" in .claude/skills/attach-visuals.')
    posted = gh(f'repos/{REPO}/issues/{number}/comments', {'body': comment_body(manifest, sizes, note, a.footer)})
    print(posted['html_url'])


if __name__ == '__main__':
    main()
