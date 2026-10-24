// One assembled character: a body with its skeleton, plus clothing, hair and gear bound to it.
//
// Follows docs/ASSETS.md: pieces are rebound to the body's bones by name, regions are hidden by
// layer, skin and hair are tinted, rigid gear rides on socket bones, clips' curves and events
// move hands and items (perform.js), and capes swing on springs.

import * as THREE from 'three';
import { bitmask, prepareMesh } from './regions.js';
import { Player, prepareClips } from './anim.js';
import { Performer } from './perform.js';
import { SpringBones, defaultColliders, resolveColliders } from './springs.js';

export const ASSETS = '/assets/';

/** A mesh file at a level of detail: `x.gltf` is level 0, `x.lod1.gltf` level 1 and so on. */
export function lodFile(file, lod) {
  return lod ? file.replace(/\.gltf$/, `.lod${lod}.gltf`) : file;
}

function disposeObject(root) {
  root.traverse((o) => {
    if (!o.isMesh) return;
    o.geometry?.dispose();
    const mats = Array.isArray(o.material) ? o.material : [o.material];
    for (const m of mats) m?.dispose();
    o.customDepthMaterial?.dispose();
  });
}

function textureStem(tex) {
  if (!tex || !tex.name) return '';
  return tex.name.split('/').pop().replace(/\.(png|jpe?g|webp)$/i, '');
}

function materialsOf(mesh) {
  return Array.isArray(mesh.material) ? mesh.material : [mesh.material];
}

const textureLoader = new THREE.TextureLoader();
const textureCache = new Map();

function loadTexture(name, like) {
  const key = name;
  if (!textureCache.has(key)) {
    const tex = textureLoader.load(`${ASSETS}humanoid/textures/${name}.jpg`);
    tex.flipY = false; // glTF convention.
    tex.colorSpace = THREE.SRGBColorSpace;
    tex.name = `${name}.jpg`;
    textureCache.set(key, tex);
  }
  const tex = textureCache.get(key);
  if (like) {
    tex.wrapS = like.wrapS;
    tex.wrapT = like.wrapT;
    tex.minFilter = like.minFilter;
    tex.magFilter = like.magFilter;
    tex.anisotropy = like.anisotropy;
  }
  return tex;
}

export class Character {
  /** ctx: { humanoid, gear, loader, library, lod } */
  static async create(ctx, bodyType) {
    const c = new Character(ctx, bodyType);
    await c.loadBody();
    return c;
  }

  constructor(ctx, bodyType) {
    this.ctx = ctx;
    this.cat = ctx.humanoid;
    this.gearCat = ctx.gear;
    this.bodyType = bodyType;
    this.def = this.cat.body_types[bodyType];
    this.group = new THREE.Group();
    this.group.name = `character:${bodyType}`;
    this.pieces = new Map(); // id -> { def, meshes: [entry], extras, layer }
    this.hair = new Map(); // kind -> { def, meshes: [entry] }
    this.gear = new Map(); // slot -> { def, object, sheath, skinned, springs }
    this.tone = null;
    this.hairColour = null;
    this.variants = {}; // piece id -> texture stem
    this.dyes = {}; // piece id -> [main cloth dye id, trim dye id] (null keeps the painted colour)
    this.drawn = true;
    this.springs = [];
    this.helpers = null;
  }

  async loadBody() {
    const gltf = await this.ctx.loader.loadAsync(ASSETS + lodFile(this.def.file, this.ctx.lod));
    this.root = gltf.scene;
    this.group.add(this.root);
    this.armature = this.root.getObjectByName('skeleton') || this.root.children[0];
    this.bones = new Map();
    this.root.traverse((o) => {
      if (o.isBone) this.bones.set(o.name, o);
    });
    this.bodyMeshes = [];
    const meshes = [];
    this.root.traverse((o) => {
      if (o.isMesh) meshes.push(o);
    });
    for (const m of meshes) {
      const entry = prepareMesh(m);
      entry.kind = entry.hasRegion ? 'body' : 'eyes';
      this.bodyMeshes.push(entry);
    }
    // Clips for this skeleton: the pelvis translation scales with leg length.
    const pelvis = this.bones.get('pelvis');
    const ratio = this.def.pelvis[1] / this.cat.animations.pelvis[1];
    this.clips = prepareClips(this.ctx.library, pelvis.position.clone(), ratio);
    this.player = new Player(this.root, this.clips);
    this.performer = new Performer(this);
    this.root.updateMatrixWorld(true);
    // Fur: a markings mask on the body, and a tail on springs.
    if (this.def.markings) {
      const tex = textureLoader.load(ASSETS + this.def.markings);
      tex.flipY = false;
      tex.colorSpace = THREE.NoColorSpace;
      for (const e of this.bodyMeshes) if (e.dye) e.dye.markings.value = tex;
    }
    if (this.def.springs) this.bodySprings = this.makeSprings(this.def.springs);
    // Rest-pose snapshot of the socket frames, for the helper display.
    this.sockets = this.def.sockets || [...this.bones.keys()].filter((n) => n.startsWith('socket_'));
  }

  // --- Skinned pieces ---------------------------------------------------------------------

  async loadSkinned(file) {
    const gltf = await this.ctx.loader.loadAsync(ASSETS + lodFile(file, this.ctx.lod));
    const scene = gltf.scene;
    scene.updateMatrixWorld(true);
    const pieceBones = new Map();
    scene.traverse((o) => {
      if (o.isBone) pieceBones.set(o.name, o);
    });
    // Bones the body lacks (cape chains) move under the body's bone of the same parent name.
    const extras = [...pieceBones.values()].filter((b) => !this.bones.has(b.name));
    const extraSet = new Set(extras);
    const attached = [];
    for (const b of extras) {
      if (b.parent && b.parent.isBone && !extraSet.has(b.parent)) {
        const target = this.bones.get(b.parent.name);
        if (target) {
          target.add(b); // Keeps the local transform: both skeletons share the parent's frame.
          attached.push(b);
        }
      }
    }
    const extraMap = new Map(extras.map((b) => [b.name, b]));
    const meshes = [];
    scene.traverse((o) => {
      if (o.isSkinnedMesh) meshes.push(o);
    });
    const entries = [];
    for (const m of meshes) {
      const bones = m.skeleton.bones.map((b) => this.bones.get(b.name) || extraMap.get(b.name) || b);
      const skel = new THREE.Skeleton(bones, m.skeleton.boneInverses);
      this.armature.add(m);
      m.bind(skel, m.bindMatrix);
      entries.push(prepareMesh(m));
    }
    return { entries, extras: attached, gltf };
  }

  removeSkinned(rec) {
    for (const e of rec.entries) {
      e.mesh.removeFromParent();
      disposeObject(e.mesh);
    }
    for (const b of rec.extras || []) b.removeFromParent();
  }

  findPiece(id) {
    return this.cat.pieces.find((p) => p.id === id);
  }

  /** Equips a clothing or armour piece by id (replacing whatever holds its slot). */
  async setPiece(slot, id) {
    for (const [pid, rec] of [...this.pieces]) {
      if (rec.def.slot === slot) {
        this.removeSkinned(rec);
        this.pieces.delete(pid);
      }
    }
    if (id) {
      const def = this.findPiece(id);
      const file = def?.files?.[this.bodyType];
      if (file) {
        const rec = await this.loadSkinned(file);
        rec.def = def;
        rec.layer = def.layer;
        this.pieces.set(id, rec);
        this.applyVariant(id);
        this.applyDyes(id);
      }
    }
    this.applyTints();
    this.refreshVisibility();
  }

  async setHair(kind, id) {
    const old = this.hair.get(kind);
    if (old) {
      this.removeSkinned(old);
      this.hair.delete(kind);
    }
    if (id) {
      const def = this.cat.hair.find((h) => h.id === id);
      const file = def?.files?.[this.bodyType];
      if (file) {
        const rec = await this.loadSkinned(file);
        rec.def = def;
        this.hair.set(kind, rec);
      }
    }
    this.applyTints();
    this.refreshVisibility();
  }

  // --- Tints and colourways ----------------------------------------------------------------

  /** The skin or fur tone: `linear`, plus `light` and `dark` for bodies with fur markings. */
  setTone(rgb, tone = null) {
    this.tone = rgb;
    const on = this.def.markings && tone?.light && tone?.dark ? 1 : 0;
    for (const e of this.bodyMeshes) {
      if (!e.dye) continue;
      e.dye.markOn.value = on;
      if (on) {
        // Doubled like the tone itself: the fur detail map averages 0.5.
        e.dye.light.value.setRGB(...tone.light.map((c) => 2 * c), THREE.LinearSRGBColorSpace);
        e.dye.dark.value.setRGB(...tone.dark.map((c) => 2 * c), THREE.LinearSRGBColorSpace);
      }
    }
    this.applyTints();
  }

  setHairColour(rgb) {
    this.hairColour = rgb;
    this.applyTints();
  }

  allMeshEntries() {
    const out = [...this.bodyMeshes];
    for (const rec of this.pieces.values()) out.push(...rec.entries);
    for (const rec of this.hair.values()) out.push(...rec.entries);
    for (const rec of this.gear.values()) if (rec.skinned) out.push(...rec.skinned.entries);
    return out;
  }

  applyTints() {
    for (const e of this.allMeshEntries()) {
      for (const m of materialsOf(e.mesh)) {
        const name = m.name || '';
        const rgb = name.startsWith('skin') ? this.tone : name.startsWith('hair') ? this.hairColour : null;
        if (rgb) m.color.setRGB(2 * rgb[0], 2 * rgb[1], 2 * rgb[2], THREE.LinearSRGBColorSpace);
      }
    }
  }

  /** Colourways a piece offers: [base stem, ...alternatives] per textured material. */
  variantsOf(id) {
    const rec = this.pieces.get(id);
    const table = this.cat.texture_variants || {};
    if (!rec) return [];
    const stems = new Set();
    for (const e of rec.entries) {
      for (const m of materialsOf(e.mesh)) {
        const stem = m.userData.baseMapStem ?? textureStem(m.map);
        if (table[stem]) stems.add(stem);
      }
    }
    return [...stems].map((base) => [base, ...table[base]]);
  }

  setVariant(id, stem) {
    if (stem) this.variants[id] = stem;
    else delete this.variants[id];
    this.applyVariant(id);
  }

  applyVariant(id) {
    const rec = this.pieces.get(id);
    if (!rec) return;
    const table = this.cat.texture_variants || {};
    const want = this.variants[id];
    for (const e of rec.entries) {
      for (const m of materialsOf(e.mesh)) {
        if (!m.map) continue;
        if (m.userData.baseMap === undefined) {
          m.userData.baseMap = m.map;
          m.userData.baseMapStem = textureStem(m.map);
        }
        const base = m.userData.baseMapStem;
        const options = table[base];
        if (!options) continue;
        const next = want && options.includes(want) ? loadTexture(want, m.userData.baseMap) : m.userData.baseMap;
        if (m.map !== next) {
          m.map = next;
          m.needsUpdate = true;
        }
      }
    }
  }

  // --- Dyes -------------------------------------------------------------------------------

  /** Dyes a piece's main cloth (channel 0) or trim (channel 1); null restores the paint. */
  setDye(id, channel, dyeId) {
    const pair = [...(this.dyes[id] || [null, null])];
    pair[channel] = dyeId || null;
    if (pair.some(Boolean)) this.dyes[id] = pair;
    else delete this.dyes[id];
    this.applyDyes(id);
  }

  applyDyes(id) {
    const rec = this.pieces.get(id);
    const def = rec?.def;
    if (!def?.dye) return;
    if (!rec.dyeMask) {
      const tex = textureLoader.load(ASSETS + def.dye);
      tex.flipY = false; // glTF convention.
      tex.colorSpace = THREE.NoColorSpace;
      rec.dyeMask = tex;
    }
    const palette = new Map((this.cat.dyes || []).map((d) => [d.id, d.linear]));
    const refs = Object.fromEntries((def.dye_channels || []).map((c) => [c.channel, c.reference]));
    const [main, trim] = this.dyes[id] || [null, null];
    for (const e of rec.entries) {
      if (!e.dye) continue;
      e.dye.map.value = rec.dyeMask;
      const set = (colour, ref, key, refKey, on) => {
        const rgb = colour && palette.get(colour);
        e.dye.on.value[on] = rgb ? 1 : 0;
        if (rgb) e.dye[key].value.setRGB(...rgb, THREE.LinearSRGBColorSpace);
        if (ref) e.dye[refKey].value.setRGB(...ref, THREE.LinearSRGBColorSpace);
      };
      set(main, refs.r, 'r', 'refR', 'x');
      set(trim, refs.g, 'g', 'refG', 'y');
    }
  }

  // --- Visibility -------------------------------------------------------------------------

  /** Everything that can hide regions: pieces plus skinned gear. */
  layered() {
    const out = [];
    for (const [id, rec] of this.pieces) out.push({ id, def: rec.def, layer: rec.def.layer, rec });
    for (const rec of this.gear.values()) {
      if (rec.skinned) out.push({ id: rec.def.id, def: rec.def, layer: rec.def.layer ?? 4, rec: rec.skinned });
    }
    return out;
  }

  refreshVisibility() {
    const regions = this.cat.regions;
    const bt = this.bodyType;
    const layered = this.layered();
    const hidesOn = (target, layer) => {
      const names = [];
      for (const p of layered) {
        if (p.layer <= layer) continue;
        names.push(...(p.def.hides?.[bt]?.[target] || []));
      }
      return bitmask(names, regions);
    };
    const bodyMask = hidesOn('body', 0);
    const faceHidden = (bodyMask & bitmask(['face'], regions)) !== 0;
    for (const e of this.bodyMeshes) {
      if (e.hasRegion) e.hidden.value = bodyMask;
      else e.mesh.visible = !faceHidden; // Eyes.
    }
    for (const p of layered) {
      const mask = hidesOn(p.id, p.layer);
      for (const e of p.rec.entries) e.hidden.value = mask;
    }
    const coversHair = layered.some((p) => p.def.covers_hair);
    for (const [kind, rec] of this.hair) {
      const visible = kind === 'hair' ? !coversHair : !faceHidden;
      for (const e of rec.entries) e.mesh.visible = visible;
    }
    this.masks = { body: bodyMask };
  }

  // --- Gear ---------------------------------------------------------------------------------

  findGear(id) {
    return (this.gearCat?.items || []).find((g) => g.id === id);
  }

  async loadRigid(file, morphs = false) {
    // Bows keep their full mesh: their morph targets have no levels of detail.
    const gltf = await this.ctx.loader.loadAsync(ASSETS + (morphs ? file : lodFile(file, this.ctx.lod)));
    const obj = gltf.scene;
    obj.traverse((o) => {
      if (o.isMesh) {
        o.castShadow = true;
        o.receiveShadow = true;
      }
    });
    return obj;
  }

  backKind() {
    const back = this.gear.get('back');
    return back?.def?.back_kind || null;
  }

  stowedSocket(def) {
    const s = def.stowed;
    if (!s) return null;
    if (typeof s === 'string') return s;
    const kind = this.backKind();
    return (kind && s[kind]) || s.default || null;
  }

  attach(obj, socketName) {
    const socket = this.bones.get(socketName);
    if (!socket) {
      console.warn(`no socket ${socketName} on ${this.bodyType}`);
      return;
    }
    socket.add(obj);
    obj.position.set(0, 0, 0);
    obj.quaternion.identity();
    // Rigid gear scales with the race (body_types.<id>.gear_scale).
    const s = this.def?.gear_scale || 1;
    obj.scale.set(s, s, s);
  }

  /** Puts gear on its held or stowed sockets (`drawn` overrides the character's state). */
  placeGear(drawn = this.drawn, skip = null) {
    for (const rec of this.gear.values()) {
      if (rec === skip) continue;
      const def = rec.def;
      if (rec.object) {
        const socket = drawn || def.slot === 'back' || def.slot === 'ammo' ? def.held || this.stowedSocket(def) : this.stowedSocket(def) || def.held;
        if (socket) this.attach(rec.object, socket);
      }
      if (rec.sheath) {
        const socket = this.stowedSocket(def);
        if (socket) this.attach(rec.sheath, socket);
      }
    }
  }

  /** Equips gear in a slot: main_hand (also two-handers), off_hand, back or ammo. */
  async setGear(slot, id) {
    const def = id ? this.findGear(id) : null;
    const clear = new Set([slot]);
    if (def?.slot === 'two_hand') clear.add('off_hand');
    if (slot === 'off_hand') {
      const main = this.gear.get('main_hand');
      if (main?.def?.slot === 'two_hand') clear.add('main_hand');
    }
    for (const s of clear) this.removeGear(s);
    if (def) {
      const rec = { def };
      if (def.file) rec.object = await this.loadRigid(def.file, !!def.string);
      if (def.sheath) rec.sheath = await this.loadRigid(def.sheath);
      const skinnedFile = def.files?.[this.bodyType];
      if (skinnedFile) {
        rec.skinned = await this.loadSkinned(skinnedFile);
        if (def.springs) rec.springs = this.makeSprings(def.springs);
      }
      this.gear.set(slot, rec);
    }
    this.placeGear();
    this.applyTints();
    this.refreshVisibility();
  }

  removeGear(slot) {
    const rec = this.gear.get(slot);
    if (!rec) return;
    for (const o of [rec.object, rec.sheath]) {
      if (o) {
        o.removeFromParent();
        disposeObject(o);
      }
    }
    if (rec.springs) {
      rec.springs.restore();
      this.springs = this.springs.filter((s) => s !== rec.springs);
    }
    if (rec.skinned) this.removeSkinned(rec.skinned);
    this.gear.delete(slot);
  }

  setDrawn(drawn) {
    this.drawn = drawn;
    this.placeGear();
  }

  /** The clip that takes the gear out (drawing) or puts it away, if the gear has one. */
  transitionClip(drawing) {
    for (const slot of ['main_hand', 'off_hand']) {
      const clips = this.gear.get(slot)?.def?.clips;
      const name = drawing ? clips?.ready : clips?.stow;
      if (name && this.clips.has(name)) return name;
    }
    return null;
  }

  /** The idle for the drawn gear (the main hand's first). */
  gearIdle() {
    for (const slot of ['main_hand', 'off_hand']) {
      const idle = this.gear.get(slot)?.def?.clips?.idle;
      if (idle && this.clips.has(idle)) return idle;
    }
    return null;
  }

  /** The idle with weapons away: a worn piece may name its own (robes hold the arms out). */
  restIdle() {
    for (const rec of this.pieces.values()) {
      const idle = rec.def?.idle;
      if (idle && this.clips.has(idle)) return idle;
    }
    return 'Idle_Loop';
  }

  attacks() {
    return [].concat(this.gear.get('main_hand')?.def?.clips?.attacks || []).filter((n) => this.clips.has(n));
  }

  /** The drawn weapons' preferred clips, in order. */
  gearClips() {
    const out = [];
    for (const slot of ['main_hand', 'off_hand']) {
      const rec = this.gear.get(slot);
      const clips = rec?.def?.clips;
      if (!clips) continue;
      for (const v of Object.values(clips)) for (const name of [].concat(v)) if (name && !out.includes(name)) out.push(name);
    }
    return out;
  }

  // --- Springs -----------------------------------------------------------------------------

  colliders() {
    const listed = this.def.colliders;
    if (listed && listed.length) return resolveColliders(listed, this.bones);
    return defaultColliders(this.bones, this.bodyType === 'female' ? 0.92 : 1);
  }

  makeSprings(spec) {
    this.root.updateMatrixWorld(true);
    const chains = (spec.chains || []).map((names) => names.map((n) => this.bones.get(n) || this.findBone(n)));
    // A chain never collides with capsules on its own bones (a tail with the tail's capsules).
    const own = new Set(chains.flat());
    const system = new SpringBones(chains, spec, this.colliders().filter((c) => !own.has(c.bone)));
    this.springs.push(system);
    return system;
  }

  findBone(name) {
    let found = null;
    this.root.traverse((o) => {
      if (!found && o.isBone && o.name === name) found = o;
    });
    return found;
  }

  /** For tests: a spring system over bones that are already in the skeleton. */
  addSprings(chains, options) {
    this.root.updateMatrixWorld(true);
    const system = new SpringBones(chains, options, this.colliders());
    this.springs.push(system);
    return system;
  }

  // --- Frame --------------------------------------------------------------------------------

  update(dt) {
    this.player.update(dt);
    this.group.updateMatrixWorld(true);
    this.performer.apply();
    this.group.updateMatrixWorld(true);
    for (const s of this.springs) s.update(this.player.paused ? 0 : dt * this.player.speed);
  }

  /** Re-seeds every spring chain from the current pose (after a seek). */
  settleSprings(seconds = 1.5) {
    for (const s of this.springs) s.reset();
    const steps = Math.round(seconds * 120);
    for (let i = 0; i < steps; i++) for (const s of this.springs) s.step(1 / 120);
  }

  triangles() {
    let n = 0;
    this.group.traverse((o) => {
      if (o.isMesh && o.visible) {
        const g = o.geometry;
        n += (g.index ? g.index.count : g.attributes.position.count) / 3;
      }
    });
    return Math.round(n);
  }

  dispose() {
    this.player?.dispose();
    this.group.removeFromParent();
    disposeObject(this.group);
  }
}
