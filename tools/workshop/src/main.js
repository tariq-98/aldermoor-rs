// The character workshop: wires the catalogue, the character, the panel and the URL hash.

import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { SkeletonHelper } from 'three';
import { View } from './view.js';
import { ASSETS, Character } from './character.js';
import { loadLibrary } from './anim.js';
import { regionDebug } from './regions.js';
import { renderPanel } from './ui.js';

THREE.Cache.enabled = true;

const DEFAULTS = {
  body: 'male',
  tone: 'warm',
  hair: 'hair.simple_parted',
  brows: 'brows.strong',
  beard: null,
  haircol: 'chestnut',
  pieces: { torso_under: 'peasant.top', legs_under: 'peasant.trousers', feet: 'peasant.shoes' },
  variants: {},
  gear: {},
  drawn: true,
  clip: 'Idle_Loop',
  speed: 1,
  paused: false,
  t: null,
  cam: 'three_quarter',
  turn: false,
  debug: [],
};

// --- URL hash ---------------------------------------------------------------------------------

function parseHash(hash) {
  const s = structuredClone(DEFAULTS);
  const params = new URLSearchParams(hash.replace(/^#/, ''));
  if (![...params.keys()].length) return s;
  const none = (v) => (v === '' || v === 'none' ? null : v);
  if ([...params.keys()].some((k) => k.startsWith('p.'))) s.pieces = {};
  for (const [k, v] of params) {
    if (k === 'body') s.body = v;
    else if (k === 'tone') s.tone = v;
    else if (k === 'hair') s.hair = none(v);
    else if (k === 'brows') s.brows = none(v);
    else if (k === 'beard') s.beard = none(v);
    else if (k === 'haircol') s.haircol = v;
    else if (k.startsWith('p.')) {
      if (none(v)) s.pieces[k.slice(2)] = v;
    } else if (k.startsWith('v.')) s.variants[k.slice(2)] = v;
    else if (k.startsWith('g.')) {
      if (none(v)) s.gear[k.slice(2)] = v;
    } else if (k === 'drawn') s.drawn = v !== '0';
    else if (k === 'clip') s.clip = v;
    else if (k === 'speed') s.speed = Number(v) || 1;
    else if (k === 'paused') s.paused = v === '1';
    else if (k === 't') s.t = Number(v);
    else if (k === 'cam') s.cam = v;
    else if (k === 'turn') s.turn = v === '1';
    else if (k === 'dbg') s.debug = v ? v.split(',') : [];
  }
  return s;
}

function writeHash(s) {
  const p = new URLSearchParams();
  p.set('body', s.body);
  p.set('tone', s.tone);
  p.set('hair', s.hair || 'none');
  p.set('haircol', s.haircol);
  p.set('brows', s.brows || 'none');
  p.set('beard', s.beard || 'none');
  for (const [slot, id] of Object.entries(s.pieces)) if (id) p.set(`p.${slot}`, id);
  if (!Object.values(s.pieces).some(Boolean)) p.set('p.torso_under', 'none');
  for (const [id, v] of Object.entries(s.variants)) if (v && s.pieces && Object.values(s.pieces).includes(id)) p.set(`v.${id}`, v);
  for (const [slot, id] of Object.entries(s.gear)) if (id) p.set(`g.${slot}`, id);
  if (!s.drawn) p.set('drawn', '0');
  p.set('clip', s.clip);
  if (s.speed !== 1) p.set('speed', String(s.speed));
  if (s.paused) {
    p.set('paused', '1');
    if (s.t !== null) p.set('t', s.t.toFixed(3));
  }
  p.set('cam', s.cam);
  if (s.turn) p.set('turn', '1');
  if (s.debug.length) p.set('dbg', s.debug.join(','));
  const hash = `#${p.toString()}`;
  if (location.hash !== hash) history.replaceState(null, '', hash);
}

// --- App --------------------------------------------------------------------------------------

async function fetchJson(url) {
  const r = await fetch(url);
  if (!r.ok) throw new Error(`${url}: ${r.status}`);
  return r.json();
}

/**
 * The gear catalogue is optional until art/blender/build_gear.py has run. Check the folder
 * listing first (the dev server provides one), so a missing file doesn't log a 404.
 */
async function optionalCatalog(name) {
  try {
    const listing = await fetch(`${ASSETS}catalog/`);
    if (listing.ok && (listing.headers.get('content-type') || '').includes('html')) {
      const html = await listing.text();
      if (!html.includes(`href="${name}"`)) return null;
    }
    const r = await fetch(`${ASSETS}catalog/${name}`);
    return r.ok ? r.json() : null;
  } catch {
    return null;
  }
}

class App {
  async start() {
    this.status('Loading catalogue…');
    this.humanoid = await fetchJson(`${ASSETS}catalog/humanoid.json`);
    this.gear = await optionalCatalog('gear.json');
    this.loader = new GLTFLoader();
    this.library = await loadLibrary(this.loader, ASSETS + this.humanoid.animations.file);
    this.view = new View(document.getElementById('stage'));
    this.state = parseHash(location.hash);
    if (!this.humanoid.body_types[this.state.body]) this.state.body = Object.keys(this.humanoid.body_types)[0];
    this.queue = Promise.resolve();
    this.timer = new THREE.Timer();
    this.timer.connect(document);
    this.frames = 0;
    this.fpsTime = 0;
    this.fps = 0;
    this.actions = this.makeActions();
    await this.build();
    this.view.preset(this.state.cam, this.character.def.height, this.headY());
    this.view.turntable = this.state.turn;
    this.renderPanel();
    window.addEventListener('hashchange', () => this.onHash());
    window.addEventListener('keydown', (e) => this.onKey(e));
    this.loop();
    document.body.dataset.ready = '1';
  }

  status(text) {
    const s = document.getElementById('status');
    if (s) s.textContent = text;
    this.statusText = text;
  }

  headY() {
    const head = this.character?.bones.get('Head');
    if (!head) return null;
    this.character.root.updateMatrixWorld(true);
    return head.getWorldPosition(new THREE.Vector3()).y + 0.08;
  }

  /** Builds the character for the current state from scratch. */
  async build() {
    const s = this.state;
    this.status('Loading character…');
    const next = await Character.create({ humanoid: this.humanoid, gear: this.gear, loader: this.loader, library: this.library }, s.body);
    next.setTone(this.tone(s.tone));
    next.setHairColour(this.hairColour(s.haircol));
    next.variants = { ...s.variants };
    const jobs = [];
    for (const [kind, id] of [['hair', s.hair], ['brows', s.brows], ['beard', s.beard]]) if (id) jobs.push(next.setHair(kind, id));
    for (const [slot, id] of Object.entries(s.pieces)) if (id) jobs.push(next.setPiece(slot, id));
    await Promise.all(jobs);
    for (const slot of ['back', 'main_hand', 'off_hand', 'ammo']) {
      if (s.gear[slot]) await next.setGear(slot, s.gear[slot]);
    }
    next.setDrawn(s.drawn);
    next.applyTints();
    next.refreshVisibility();
    if (this.character) this.character.dispose();
    this.character = next;
    this.view.scene.add(next.group);
    this.applyDebug();
    this.playClip(s.clip, 0);
    if (s.paused && s.t !== null) {
      next.player.paused = true;
      next.player.seek(s.t);
      next.update(0);
      next.settleSprings();
    } else {
      next.player.paused = s.paused;
    }
    next.player.speed = s.speed;
    this.status('');
  }

  tone(id) {
    return (this.humanoid.skin_tones.find((t) => t.id === id) || this.humanoid.skin_tones[0]).linear;
  }

  hairColour(id) {
    return (this.humanoid.hair_colours.find((t) => t.id === id) || this.humanoid.hair_colours[0]).linear;
  }

  playClip(name, fade = 0.25) {
    const c = this.character;
    if (!c.player.play(name, fade)) {
      c.player.play('Idle_Loop', fade);
      this.state.clip = 'Idle_Loop';
    }
  }

  /** Runs async character edits one at a time, then refreshes the hash and panel. */
  run(fn, rebuildPanel = true) {
    this.queue = this.queue
      .then(fn)
      .catch((e) => {
        console.error(e);
        this.status(`Error: ${e.message}`);
      })
      .then(() => {
        writeHash(this.state);
        if (rebuildPanel) this.renderPanel();
      });
    return this.queue;
  }

  makeActions() {
    const s = () => this.state;
    return {
      setBody: (id) => this.run(async () => {
        s().body = id;
        // Keep the tone valid for the race: goblins have their own greens.
        const race = this.humanoid.body_types[id]?.race;
        const tone = this.humanoid.skin_tones.find((t) => t.id === s().tone);
        const raceTones = this.humanoid.skin_tones.filter((t) => t.races?.includes(race));
        if (raceTones.length && !raceTones.includes(tone)) s().tone = raceTones[0].id;
        if (!raceTones.length && tone?.races) s().tone = this.humanoid.defaults?.[id]?.tone || 'warm';
        await this.build();
        this.view.preset(s().cam, this.character.def.height, this.headY());
      }),
      setTone: (id) => this.run(async () => {
        s().tone = id;
        this.character.setTone(this.tone(id));
      }),
      setHairColour: (id) => this.run(async () => {
        s().haircol = id;
        this.character.setHairColour(this.hairColour(id));
      }),
      setHair: (kind, id) => this.run(async () => {
        s()[kind] = id;
        await this.character.setHair(kind, id);
      }),
      setPiece: (slot, id) => this.run(async () => {
        s().pieces[slot] = id;
        if (!id) delete s().pieces[slot];
        await this.character.setPiece(slot, id);
      }),
      setVariant: (id, stem) => this.run(async () => {
        if (stem) s().variants[id] = stem;
        else delete s().variants[id];
        this.character.setVariant(id, stem);
      }),
      setGear: (slot, id) => this.run(async () => {
        const def = id ? this.gear?.items?.find((g) => g.id === id) : null;
        if (id) s().gear[slot] = id;
        else delete s().gear[slot];
        if (def?.slot === 'two_hand') delete s().gear.off_hand;
        const main = this.gear?.items?.find((g) => g.id === s().gear.main_hand);
        if (slot === 'off_hand' && id && main?.slot === 'two_hand') delete s().gear.main_hand;
        await this.character.setGear(slot, id);
        // A drawn weapon's own idle replaces the generic idle.
        const clips = this.character.gearClips();
        if (def && clips.length && s().drawn && s().clip === 'Idle_Loop') {
          s().clip = clips[0];
          this.playClip(clips[0]);
        }
      }),
      setDrawn: (on) => this.run(async () => {
        s().drawn = on;
        this.character.setDrawn(on);
      }),
      setClip: (name) => this.run(async () => {
        s().clip = name;
        s().t = null;
        this.playClip(name);
        if (s().paused) this.character.player.seek(0);
      }, false),
      togglePause: () => this.run(async () => {
        s().paused = !s().paused;
        this.character.player.paused = s().paused;
        s().t = s().paused ? this.character.player.time : null;
        this.panel?.setPlaying(s().paused);
      }, false),
      setSpeed: (v) => {
        s().speed = v;
        this.character.player.speed = v;
        writeHash(s());
      },
      scrub: (f) => {
        const p = this.character.player;
        s().paused = true;
        p.paused = true;
        s().t = f * p.duration;
        p.seek(s().t);
        this.panel?.setPlaying(true);
        writeHash(s());
      },
      setCamera: (name) => this.run(async () => {
        s().cam = name;
        this.view.preset(name, this.character.def.height, this.headY());
      }),
      setTurntable: (on) => this.run(async () => {
        s().turn = on;
        this.view.turntable = on;
      }, false),
      setDebug: (flag, on) => this.run(async () => {
        const set = new Set(s().debug);
        if (on) set.add(flag);
        else set.delete(flag);
        s().debug = [...set];
        this.applyDebug();
      }, false),
      screenshot: () => this.view.screenshot(`aldermoor-${s().body}-${s().clip}.png`),
    };
  }

  applyDebug() {
    const d = new Set(this.state.debug);
    regionDebug.value = d.has('regions') ? 1 : 0;
    const wire = d.has('wire');
    this.character.group.traverse((o) => {
      if (!o.isMesh) return;
      for (const m of Array.isArray(o.material) ? o.material : [o.material]) m.wireframe = wire;
    });
    if (this.helpers) {
      for (const h of this.helpers) h.removeFromParent();
      this.helpers = null;
    }
    if (d.has('skel')) {
      const skel = new SkeletonHelper(this.character.root);
      skel.material.depthTest = false;
      skel.material.transparent = true;
      this.view.scene.add(skel);
      this.helpers = [skel];
      for (const name of this.character.sockets) {
        const bone = this.character.bones.get(name);
        if (!bone) continue;
        const axes = new THREE.AxesHelper(0.09);
        axes.material.depthTest = false;
        axes.renderOrder = 10;
        bone.add(axes);
        this.helpers.push(axes);
      }
    }
  }

  renderPanel() {
    this.panel = renderPanel(document.getElementById('panel'), {
      state: this.state,
      humanoid: this.humanoid,
      gear: this.gear,
      character: this.character,
      actions: this.actions,
    });
    this.status(this.statusText || '');
  }

  async onHash() {
    const next = parseHash(location.hash);
    if (JSON.stringify(next) === JSON.stringify(this.state)) return;
    this.state = next;
    await this.run(async () => {
      await this.build();
      this.view.preset(this.state.cam, this.character.def.height, this.headY());
      this.view.turntable = this.state.turn;
    });
  }

  onKey(e) {
    if (e.target && ['INPUT', 'SELECT', 'TEXTAREA'].includes(e.target.tagName)) return;
    if (e.code === 'Space') {
      e.preventDefault();
      this.actions.togglePause();
    } else if (e.key === 't') this.actions.setTurntable(!this.state.turn);
    else if (e.key >= '1' && e.key <= '6') this.actions.setCamera(['front', 'back', 'left', 'right', 'three_quarter', 'head'][Number(e.key) - 1]);
  }

  loop() {
    const tick = (time) => {
      requestAnimationFrame(tick);
      this.timer.update(time);
      const dt = Math.min(this.timer.getDelta(), 0.1);
      this.character?.update(dt);
      this.view.render();
      const p = this.character?.player;
      if (p && this.panel) this.panel.setTime(p.time, p.duration);
      this.frames++;
      this.fpsTime += dt;
      if (this.fpsTime >= 0.5) {
        this.fps = this.frames / this.fpsTime;
        this.frames = 0;
        this.fpsTime = 0;
        if (!this.statusText) {
          const s = document.getElementById('status');
          if (s) s.textContent = `${Math.round(this.fps)} fps · ${this.character.triangles().toLocaleString()} triangles`;
        }
      }
    };
    requestAnimationFrame(tick);
  }
}

const app = new App();
window.workshop = app;
app.start().catch((e) => {
  console.error(e);
  const s = document.getElementById('status');
  if (s) s.textContent = `Error: ${e.message}`;
  document.body.dataset.ready = 'error';
});
