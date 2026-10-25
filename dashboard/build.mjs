// Builds the site into .vercel/output, Vercel's Build Output API (v3): the page and its assets as
// static files, and each file in src/api as a Node function at /api/<name>.

import { build } from 'esbuild';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

const OUT = '.vercel/output';
const STATIC = join(OUT, 'static');
const FILES = ['icon.svg', 'apple-touch-icon.png', 'icon-192.png', 'icon-512.png', 'icon-maskable-512.png', 'manifest.webmanifest'];

// The page may only load its own scripts and styles. Pictures and videos come through
// /api/media, which sends the browser to GitHub's signed URLs for the big ones.
const GITHUB_MEDIA = 'https://*.githubusercontent.com https://github-cloud.s3.amazonaws.com';
const CSP = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self'",
  `img-src 'self' data: ${GITHUB_MEDIA}`,
  `media-src 'self' ${GITHUB_MEDIA}`,
  "connect-src 'self'",
  "manifest-src 'self'",
  "frame-ancestors 'none'",
  "base-uri 'none'",
  "form-action 'self'",
  "object-src 'none'",
].join('; ');

const hash = (data) => createHash('sha256').update(data).digest('hex').slice(0, 12);

await rm(OUT, { recursive: true, force: true });
await mkdir(join(STATIC, 'assets'), { recursive: true });

// Script and stylesheet, named by their content so they can be cached for good.
const [script, style] = await Promise.all([
  build({ entryPoints: ['src/client/main.ts'], bundle: true, format: 'esm', target: ['es2022', 'safari16'], minify: true, write: false, legalComments: 'none' }),
  build({ entryPoints: ['static/styles.css'], bundle: true, target: ['safari16'], minify: true, write: false }),
]);
const js = script.outputFiles[0].contents;
const css = style.outputFiles[0].contents;
const jsPath = `/assets/app.${hash(js)}.js`;
const cssPath = `/assets/app.${hash(css)}.css`;
await writeFile(join(STATIC, jsPath), js);
await writeFile(join(STATIC, cssPath), css);
const html = (await readFile('static/index.html', 'utf8')).replace('/app.css', cssPath).replace('/app.js', jsPath);
await writeFile(join(STATIC, 'index.html'), html);
await Promise.all(FILES.map((file) => copyFile(join('static', file), join(STATIC, file))));

// Functions: one self-contained ES module each.
const functions = (await readdir('src/api')).filter((f) => f.endsWith('.ts')).map((f) => f.slice(0, -3));
for (const name of functions) {
  const dir = join(OUT, 'functions', 'api', `${name}.func`);
  await mkdir(dir, { recursive: true });
  await build({
    entryPoints: [`src/api/${name}.ts`],
    bundle: true,
    platform: 'node',
    format: 'esm',
    target: 'node22',
    outfile: join(dir, 'index.js'),
    legalComments: 'none',
  });
  await writeFile(join(dir, 'package.json'), JSON.stringify({ type: 'module' }));
  await writeFile(join(dir, '.vc-config.json'), JSON.stringify({
    runtime: 'nodejs22.x',
    handler: 'index.js',
    launcherType: 'Nodejs',
    shouldAddHelpers: false,
    supportsResponseStreaming: true,
    maxDuration: 30,
  }, null, 2));
}

await writeFile(join(OUT, 'config.json'), JSON.stringify({
  version: 3,
  routes: [
    { src: '/assets/(.*)', headers: { 'Cache-Control': 'public, max-age=31536000, immutable' }, continue: true },
    {
      src: '/(.*)',
      headers: {
        'Content-Security-Policy': CSP,
        'Referrer-Policy': 'no-referrer',
        'X-Content-Type-Options': 'nosniff',
        'Permissions-Policy': 'camera=(), microphone=(), geolocation=()',
      },
      continue: true,
    },
    { handle: 'filesystem' },
  ],
}, null, 2));

console.log(`Built ${jsPath} (${js.length} B), ${cssPath} (${css.length} B) and /api/{${functions.join(',')}}`);
