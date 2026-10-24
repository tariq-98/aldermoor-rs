// Beasts on their own rigs (wolves and, later, other quadrupeds): one glb with the mesh, the
// skeleton and its clips, from assets/catalog/creatures.json. It offers the same surface the
// app uses for characters (player, group, def, bones, update, dispose), minus clothing and gear.

import * as THREE from 'three';
import { ASSETS, lodFile } from './character.js';
import { Player } from './anim.js';

export class Beast {
  static async create(ctx, id) {
    const def = ctx.creatures.creatures.find((c) => c.id === id);
    if (!def) throw new Error(`no creature ${id}`);
    const gltf = await ctx.loader.loadAsync(ASSETS + lodFile(def.file, ctx.lod));
    return new Beast(def, gltf);
  }

  constructor(def, gltf) {
    this.def = def;
    this.group = new THREE.Group();
    this.root = gltf.scene;
    this.group.add(this.root);
    this.bones = new Map();
    this.sockets = [];
    this.root.traverse((o) => {
      if (o.isBone) this.bones.set(o.name, o);
      if (o.isMesh) {
        o.castShadow = true;
        o.receiveShadow = true;
        o.frustumCulled = false;
        if (o.geometry.attributes.color) o.material.vertexColors = true;
      }
    });
    this.clips = new Map(gltf.animations.map((c) => [c.name, c]));
    this.player = new Player(this.root, this.clips);
    this.springs = [];
    // Empty stand-ins for what characters carry, so tools that inspect either work.
    this.pieces = new Map();
    this.hair = new Map();
    this.masks = { body: 0 };
  }

  /** Clips in the order the catalogue lists them (idle, walk, run, attack…). */
  clipNames() {
    const order = Object.values(this.def.clips || {});
    return [...order.filter((n) => this.clips.has(n)), ...[...this.clips.keys()].filter((n) => !order.includes(n))];
  }

  update(dt) {
    this.player.update(dt);
  }

  settleSprings() {}

  gearClips() {
    return [];
  }

  setDrawn() {}

  applyTints() {}

  refreshVisibility() {}

  setTone() {}

  setHairColour() {}

  triangles() {
    let n = 0;
    this.root.traverse((o) => {
      if (o.isMesh) n += (o.geometry.index ? o.geometry.index.count : o.geometry.attributes.position.count) / 3;
    });
    return n;
  }

  dispose() {
    this.player.dispose();
    this.group.removeFromParent();
    this.root.traverse((o) => {
      if (!o.isMesh) return;
      o.geometry.dispose();
      for (const m of Array.isArray(o.material) ? o.material : [o.material]) m.dispose();
    });
  }
}
