// Loads looks in headless Chrome, saves screenshots and fails on console errors.
//
//   node tools/workshop/check.mjs [--out DIR] [--only name,name] [--springs]
//
// Screenshots go to art/out/review/workshop/ by default. --springs also runs the spring-bone
// test: a procedural chain hung from the upper spine, checked for attachment, finite motion and
// collider clearance through a run cycle (nothing is written to the catalogue or assets).

import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(HERE, '../..');
const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i >= 0 ? args[i + 1] : fallback;
};
const OUT = path.resolve(flag('--out', path.join(ROOT, 'art/out/review/workshop')));
const ONLY = flag('--only', '');
const SPRINGS = args.includes('--springs');
const UI = args.includes('--ui');
const NOGEAR = args.includes('--nogear');
const CHROME = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

const LOOKS = {
  farmhand: 'body=male&tone=tan&hair=hair.simple_parted&haircol=dark%20brown&brows=brows.strong&beard=beard.full&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&clip=Idle_Loop&paused=1&t=0.6&cam=three_quarter',
  ranger_hood: 'body=female&tone=porcelain&hair=hair.long&haircol=copper&brows=brows.fine&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&p.head=ranger.hood&p.shoulders=ranger.pauldron&clip=Sword_Idle&paused=1&t=0.4&cam=three_quarter',
  ranger_back: 'body=female&tone=porcelain&hair=hair.long&haircol=copper&brows=brows.fine&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&p.head=ranger.hood&p.shoulders=ranger.pauldron&clip=Sword_Idle&paused=1&t=0.4&cam=back',
  mixed: 'body=male&tone=deep&hair=hair.buzzed&haircol=black&brows=brows.strong&beard=none&p.torso_under=ranger.top&p.legs_under=peasant.trousers&p.feet=ranger.boots&v.ranger.top=T_Ranger_3_BaseColor&clip=Jog_Fwd_Loop&paused=1&t=0.3&cam=front',
  attack: 'body=female&tone=olive&hair=hair.simple_parted&haircol=honey&brows=brows.fine&beard=none&p.torso_under=peasant.top&p.legs_under=ranger.trousers&p.feet=peasant.shoes&p.head=ranger.hood&clip=Sword_Regular_A&paused=1&t=0.22&cam=three_quarter',
  regions: 'body=male&tone=warm&hair=none&brows=none&beard=none&p.torso_under=none&clip=Idle_Loop&paused=1&t=0&cam=front&dbg=regions',
  head: 'body=female&tone=brown&hair=hair.buns&haircol=black&brows=brows.fine&beard=none&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&clip=Idle_Loop&paused=1&t=0.5&cam=head',
  sword_shield: 'body=male&tone=warm&hair=hair.simple_parted&haircol=chestnut&brows=brows.strong&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&g.main_hand=sword.arming&g.off_hand=shield.round&clip=Sword_Idle&paused=1&t=0.4&cam=three_quarter',
  sword_swing: 'body=male&tone=warm&hair=hair.simple_parted&haircol=chestnut&brows=brows.strong&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&g.main_hand=sword.arming&g.off_hand=shield.round&clip=Sword_Regular_A&paused=1&t=0.2&cam=front',
  stowed: 'body=female&tone=brown&hair=hair.buns&haircol=black&brows=brows.fine&beard=none&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&g.main_hand=sword.arming&g.off_hand=dagger.hunting&drawn=0&clip=Idle_Loop&paused=1&t=0.5&cam=left',
  stowed_back: 'body=female&tone=brown&hair=hair.buns&haircol=black&brows=brows.fine&beard=none&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&g.main_hand=sword.arming&g.off_hand=shield.round&drawn=0&clip=Idle_Loop&paused=1&t=0.5&cam=back',
  archer: 'body=female&tone=porcelain&hair=hair.long&haircol=copper&brows=brows.fine&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&g.main_hand=bow.hunting&g.ammo=quiver.leather&clip=Idle_Loop&paused=1&t=0.5&cam=three_quarter',
  archer_back: 'body=female&tone=porcelain&hair=hair.long&haircol=copper&brows=brows.fine&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&g.main_hand=bow.hunting&g.ammo=quiver.leather&drawn=0&clip=Idle_Loop&paused=1&t=0.5&cam=back',
  mage: 'body=male&tone=olive&hair=hair.long&haircol=ash&brows=brows.strong&beard=beard.full&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&g.main_hand=staff.oak&clip=Spell_Simple_Idle_Loop&paused=1&t=0.5&cam=three_quarter',
  caped: 'body=male&tone=warm&hair=hair.simple_parted&haircol=chestnut&brows=brows.strong&beard=beard.full&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&g.main_hand=sword.arming&g.off_hand=shield.round&g.back=cape.traveller&drawn=0&clip=Idle_Loop&paused=1&t=0.5&cam=three_quarter',
  caped_back: 'body=male&tone=warm&hair=hair.simple_parted&haircol=chestnut&brows=brows.strong&beard=beard.full&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&g.main_hand=sword.arming&g.off_hand=shield.round&g.back=cape.traveller&drawn=0&clip=Jog_Fwd_Loop&paused=1&t=0.3&cam=back',
  cloak_swing: 'body=female&tone=porcelain&hair=hair.long&haircol=copper&brows=brows.fine&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&g.main_hand=sword.arming&g.back=cape.ranger&clip=Sword_Regular_A&paused=1&t=0.25&cam=three_quarter',
  pack: 'body=female&tone=brown&hair=hair.buns&haircol=black&brows=brows.fine&beard=none&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&g.main_hand=sword.arming&g.off_hand=shield.round&g.back=backpack.travel&drawn=0&clip=Walk_Loop&paused=1&t=0.4&cam=three_quarter',
  pack_back: 'body=male&tone=tan&hair=hair.buzzed&haircol=dark%20brown&brows=brows.strong&beard=beard.full&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&g.main_hand=bow.hunting&g.ammo=quiver.leather&g.back=backpack.travel&drawn=0&clip=Idle_Loop&paused=1&t=0.5&cam=back',
  knight: 'body=male&tone=fair&hair=hair.simple_parted&haircol=dark%20brown&brows=brows.strong&beard=beard.full&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&p.torso=plate.cuirass&p.shoulders=plate.pauldrons&p.hands=plate.gauntlets&p.legs=plate.legs&p.head=plate.helm_norman&g.main_hand=sword.arming&g.off_hand=shield.round&g.back=cape.traveller&clip=Sword_Idle&paused=1&t=0.4&cam=three_quarter',
  knight_back: 'body=female&tone=brown&hair=hair.buns&haircol=black&brows=brows.fine&beard=none&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&p.torso=plate.cuirass&p.hands=plate.gauntlets&p.legs=plate.legs&p.head=plate.helm_kettle&g.main_hand=sword.arming&g.back=cape.ranger&drawn=0&clip=Walk_Loop&paused=1&t=0.5&cam=back',
  elf_ranger: 'body=elf_female&tone=porcelain&hair=hair.long&haircol=blonde&brows=brows.fine&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&g.main_hand=bow.long&g.ammo=quiver.leather&g.back=cape.ranger&drawn=0&clip=Idle_Loop&paused=1&t=0.5&cam=three_quarter',
  dwarf_smith: 'body=dwarf_male&tone=tan&hair=hair.buzzed&haircol=auburn&brows=brows.strong&beard=beard.dwarven&p.torso_under=peasant.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&g.main_hand=tool.hammer&clip=Idle_Loop&paused=1&t=0.5&cam=three_quarter',
  dwarf_knight: 'body=dwarf_female&tone=olive&hair=hair.buns&haircol=copper&brows=brows.fine&beard=none&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&p.torso=plate.cuirass&p.shoulders=plate.pauldrons&p.hands=plate.gauntlets&p.legs=plate.legs&p.head=plate.helm_kettle&g.main_hand=axe.bearded&g.off_hand=shield.round&clip=Sword_Idle&paused=1&t=0.4&cam=three_quarter',
  goblin_raider: 'body=goblin_male&tone=goblin%20green&hair=none&brows=none&beard=none&p.torso_under=ranger.top&p.legs_under=peasant.trousers&p.feet=peasant.shoes&p.head=plate.helm_norman&g.main_hand=dagger.hunting&g.off_hand=shield.round&clip=Sword_Idle&paused=1&t=0.4&cam=three_quarter',
  bearded_head: 'body=male&tone=fair&hair=hair.long&haircol=auburn&brows=brows.strong&beard=beard.full&p.torso_under=ranger.top&p.legs_under=ranger.trousers&p.feet=ranger.boots&clip=Idle_Loop&paused=1&t=0.5&cam=head',
};

function freePort(start) {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.once('error', () => resolve(freePort(start + 1)));
    s.once('listening', () => s.close(() => resolve(start)));
    s.listen(start, '127.0.0.1');
  });
}

async function waitFor(url, ms = 10000) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    try {
      const r = await fetch(url);
      if (r.ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 150));
  }
  throw new Error(`server did not start: ${url}`);
}

async function settle(page) {
  await page.waitForFunction(() => document.body.dataset.ready === '1' || document.body.dataset.ready === 'error', null, { timeout: 60000 });
  await page.evaluate(() => window.workshop.queue);
  // Let textures finish decoding and a few frames render.
  await page.evaluate(() => new Promise((r) => {
    let n = 0;
    const f = () => (++n > 20 ? r() : requestAnimationFrame(f));
    requestAnimationFrame(f);
  }));
}

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const port = await freePort(8790);
  const server = spawn('python3', ['-m', 'http.server', String(port), '--bind', '127.0.0.1', '--directory', ROOT], { stdio: 'ignore' });
  const base = `http://127.0.0.1:${port}/tools/workshop/`;
  let failed = false;
  try {
    await waitFor(base);
    const browser = await chromium.launch({
      executablePath: CHROME,
      headless: true,
      args: ['--use-angle=metal', '--enable-gpu', '--ignore-gpu-blocklist', '--enable-unsafe-swiftshader'],
    });
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
    const problems = [];
    page.on('console', (m) => {
      if (m.type() === 'error' || m.type() === 'warning') problems.push(`[${m.type()}] ${m.text()}`);
    });
    page.on('pageerror', (e) => problems.push(`[pageerror] ${e.message}`));
    page.on('requestfailed', (r) => {
      if (!r.url().endsWith('gear.json')) problems.push(`[requestfailed] ${r.url()}`);
    });
    page.on('response', (r) => {
      if (r.status() >= 400 && !r.url().endsWith('gear.json')) problems.push(`[http ${r.status()}] ${r.url()}`);
    });

    const names = ONLY ? ONLY.split(',') : Object.keys(LOOKS);
    for (const name of names) {
      await page.goto(`${base}#${LOOKS[name]}`, { waitUntil: 'load' });
      await settle(page);
      const info = await page.evaluate(() => {
        const w = window.workshop;
        const c = w.character;
        return {
          ready: document.body.dataset.ready,
          clip: c.player.name,
          time: Number(c.player.time.toFixed(3)),
          pieces: [...c.pieces.keys()],
          hair: [...c.hair.keys()].map((k) => `${k}:${c.hair.get(k).entries.every((e) => e.mesh.visible) ? 'shown' : 'hidden'}`),
          bodyMask: c.masks.body.toString(2),
          triangles: c.triangles(),
          webgl: w.view.renderer.getContext().getParameter(w.view.renderer.getContext().RENDERER),
        };
      });
      const file = path.join(OUT, `${name}.png`);
      await page.screenshot({ path: file });
      console.log(`${name}: ${JSON.stringify(info)} -> ${path.relative(ROOT, file)}`);
      if (info.ready !== '1') failed = true;
    }

    if (SPRINGS) {
      await page.goto(`${base}#${LOOKS.farmhand.replace('paused=1&t=0.6&', '').replace('Idle_Loop', 'Jog_Fwd_Loop')}`, { waitUntil: 'load' });
      await settle(page);
      const result = await page.evaluate(async () => {
        const THREE = await import('three');
        const w = window.workshop;
        const c = w.character;
        // A four-link chain down the back, from the upper spine: a stand-in for a cape column.
        const spine = c.bones.get('spine_03');
        c.root.updateMatrixWorld(true);
        const chain = [];
        let parent = spine;
        // Start behind the spine, then hang straight down in world space.
        const start = new THREE.Vector3(0, 1.36, -0.17);
        const down = new THREE.Vector3(0, -0.145, -0.045);
        let worldPrev = start.clone();
        for (let i = 0; i < 5; i++) {
          const b = new THREE.Bone();
          b.name = `test_chain_${i}`;
          const world = i === 0 ? start.clone() : worldPrev.clone().add(down);
          parent.updateMatrixWorld(true);
          const local = parent.worldToLocal(world.clone());
          b.position.copy(local);
          // Keep the bone's world rotation identity, like a freshly modelled chain.
          const pq = parent.getWorldQuaternion(new THREE.Quaternion());
          b.quaternion.copy(pq.invert());
          parent.add(b);
          b.updateMatrixWorld(true);
          chain.push(b);
          parent = b;
          worldPrev = world;
        }
        const system = c.addSprings([chain], { stiffness: 0.8, drag: 0.35, gravity: 1.2, radius: 0.03 });
        // A visible polyline so the screenshot shows the chain.
        const geo = new THREE.BufferGeometry().setFromPoints(chain.map(() => new THREE.Vector3()));
        const line = new THREE.Line(geo, new THREE.LineBasicMaterial({ color: 0xff3355 }));
        w.view.scene.add(line);
        const dots = chain.map(() => {
          const m = new THREE.Mesh(new THREE.SphereGeometry(0.022, 12, 8), new THREE.MeshBasicMaterial({ color: 0xff3355 }));
          w.view.scene.add(m);
          return m;
        });
        const pts = chain.map(() => new THREE.Vector3());
        const colliders = system.colliders;
        let worstPen = 0;
        let worstDrift = 0;
        let finite = true;
        const anchor = chain[0].position.clone();
        const tailOf = () => system.joints[system.joints.length - 1].tail.clone();
        const run = (seconds) => {
          let t = 0;
          let i = 0;
          let last = tailOf();
          let speed = 0;
          while (t < seconds) {
            const dt = [1 / 30, 1 / 60, 1 / 144, 1 / 240, 1 / 45][i++ % 5];
            c.update(dt);
            t += dt;
            for (let k = 0; k < chain.length; k++) chain[k].getWorldPosition(pts[k]);
            if (!pts.every((p) => Number.isFinite(p.x + p.y + p.z))) finite = false;
            worstDrift = Math.max(worstDrift, chain[0].position.distanceTo(anchor));
            for (const j of system.joints) {
              for (const cap of colliders) {
                cap.refresh();
                const ab = cap.wb.clone().sub(cap.wa);
                const tt = THREE.MathUtils.clamp(j.tail.clone().sub(cap.wa).dot(ab) / ab.lengthSq(), 0, 1);
                const d = j.tail.distanceTo(cap.wa.clone().addScaledVector(ab, tt));
                worstPen = Math.max(worstPen, cap.radius - d);
              }
            }
            const now = tailOf();
            speed = now.distanceTo(last) / dt;
            last = now;
          }
          return { tail: last.toArray().map((x) => Number(x.toFixed(3))), speed: Number(speed.toFixed(4)) };
        };
        // Idle until settled, jog with jittery frame times from 30 to 240 fps, then idle again.
        w.character.player.play('Idle_Loop', 0);
        const idle1 = run(3);
        w.character.player.play('Jog_Fwd_Loop', 0.2);
        const jog = run(4);
        w.character.player.play('Idle_Loop', 0.2);
        const idle2 = run(4);
        geo.setFromPoints(pts);
        pts.forEach((p, k) => dots[k].position.copy(p));
        c.player.paused = true;
        w.view.preset('left', c.def.height);
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        return { finite, worstDrift, worstPenetration: Number(worstPen.toFixed(5)), idle1, jog, idle2 };
      });
      const file = path.join(OUT, 'springs.png');
      await page.screenshot({ path: file });
      console.log(`springs: ${JSON.stringify(result)} -> ${path.relative(ROOT, file)}`);
      if (!result.finite || result.worstDrift > 1e-6 || result.worstPenetration > 0.005) failed = true;
    }

    if (NOGEAR) {
      // Before build_gear.py has run: the catalogue folder has no gear.json.
      await page.route('**/assets/catalog/', (route) => route.fulfill({ status: 200, contentType: 'text/html', body: '<a href="humanoid.json">humanoid.json</a>' }));
      await page.goto('about:blank'); // A real reload, not a hash change.
      await page.goto(`${base}#${LOOKS.farmhand}`, { waitUntil: 'load' });
      await settle(page);
      const note = await page.locator('.note').first().textContent().catch(() => '');
      const gear = await page.evaluate(() => window.workshop.gear);
      console.log(`nogear: gear=${JSON.stringify(gear)} note=${JSON.stringify(note)}`);
      if (gear !== null || !note.includes('No gear built yet')) failed = true;
      await page.unroute('**/assets/catalog/');
    }

    if (UI) {
      // Drive the panel like a person would, then confirm the scene followed.
      await page.goto('about:blank');
      await page.goto(base, { waitUntil: 'load' });
      await settle(page);
      const idle = () => page.evaluate(() => window.workshop.queue);
      const rowSelect = (label) => page.locator('.row', { has: page.locator(`label:text-is("${label}")`) }).locator('select').first();
      await page.click('button:text("Slender frame")');
      await idle();
      await rowSelect('Hair').selectOption('hair.buns');
      await idle();
      await rowSelect('Shirt or jerkin').selectOption('ranger.top');
      await idle();
      await rowSelect('Head').selectOption('ranger.hood');
      await idle();
      const gearRows = await page.locator('.row label:text-is("Main hand")').count();
      if (gearRows) {
        await rowSelect('Main hand').selectOption('sword.arming');
        await idle();
        await page.click('label.toggle:has-text("Weapons drawn")');
        await idle();
      }
      await rowSelect('Clip').selectOption('Walk_Loop');
      await idle();
      await page.click('button:text("Pause")');
      await idle();
      await page.locator('input.scrub').fill('0.5');
      await page.click('.grid3 button:text("Back")');
      await idle();
      await page.click('label.toggle:has-text("Region colours")');
      await idle();
      const after = await page.evaluate(() => {
        const w = window.workshop;
        const c = w.character;
        const sword = c.gear.get('main_hand')?.object;
        return {
          body: c.bodyType,
          hash: location.hash,
          hair: [...c.hair.keys()],
          hairShown: c.hair.get('hair')?.entries.every((e) => e.mesh.visible),
          pieces: [...c.pieces.keys()],
          clip: c.player.name,
          paused: c.player.paused,
          swordSocket: sword?.parent?.name || null,
          cam: w.view.presetName,
        };
      });
      const file = path.join(OUT, 'ui.png');
      await page.screenshot({ path: file });
      console.log(`ui: ${JSON.stringify(after)} -> ${path.relative(ROOT, file)}`);
      const ok = after.body === 'female' && after.hair.includes('hair') && after.hairShown === false
        && after.pieces.includes('ranger.top') && after.clip === 'Walk_Loop' && after.paused && after.cam === 'back'
        && (!gearRows || after.swordSocket === 'socket_hip_l');
      if (!ok) failed = true;
    }

    await browser.close();
    if (problems.length) {
      failed = true;
      console.log('Console problems:\n' + [...new Set(problems)].join('\n'));
    } else {
      console.log('No console errors or warnings.');
    }
  } finally {
    server.kill();
  }
  process.exit(failed ? 1 : 0);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
