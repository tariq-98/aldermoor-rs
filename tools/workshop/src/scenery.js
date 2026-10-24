// Scenery from assets/catalog/environment.json: buildings, trees, rocks, plants and props, the
// way the game should draw them. Foliage sways by its `_WIND` vertex attribute and moving parts
// (the windmill's sails) turn about their axis. It offers the same surface the app uses for
// characters and beasts (group, def, player, update, dispose).

import * as THREE from 'three';
import { ASSETS, lodFile } from './character.js';

// Shared by every swaying material: the clock and the wind's strength.
export const windTime = { value: 0 };
export const windStrength = { value: 1 };

/**
 * The sway: a vertex moves along the wind by `_WIND` × strength, in a slow gust plus a quicker
 * flutter, phased by its position so a canopy ripples instead of moving as one block.
 */
function addWind(material) {
  material.onBeforeCompile = (shader) => {
    shader.uniforms.uWindTime = windTime;
    shader.uniforms.uWindStrength = windStrength;
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', `#include <common>
attribute float _wind;
uniform float uWindTime;
uniform float uWindStrength;`)
      .replace('#include <begin_vertex>', `#include <begin_vertex>
	{
		vec4 worldPos = modelMatrix * vec4(transformed, 1.0);
		float phase = dot(worldPos.xz, vec2(0.35, 0.27));
		float gust = sin(uWindTime * 0.9 + phase) * 0.6 + sin(uWindTime * 2.3 + phase * 1.7) * 0.25;
		float flutter = sin(uWindTime * 7.0 + phase * 5.0) * 0.08;
		vec3 dir = normalize(vec3(1.0, 0.0, 0.35));
		transformed += (inverse(mat3(modelMatrix)) * dir) * _wind * uWindStrength * (gust + flutter) * 0.12;
	}`);
  };
  material.customProgramCacheKey = () => 'aldermoor-wind';
  material.needsUpdate = true;
}

export class Scenery {
  static async create(ctx, id) {
    const def = ctx.environment.props.find((p) => p.id === id);
    if (!def) throw new Error(`no prop ${id}`);
    const gltf = await ctx.loader.loadAsync(ASSETS + lodFile(def.file, ctx.lod));
    const parts = [];
    for (const part of def.parts || []) {
      const g = await ctx.loader.loadAsync(ASSETS + part.file);
      parts.push({ part, object: g.scene });
    }
    return new Scenery(def, gltf, parts);
  }

  constructor(def, gltf, parts) {
    this.def = { ...def, height: Math.max(def.height || 1, 1) };
    this.group = new THREE.Group();
    this.root = gltf.scene;
    this.group.add(this.root);
    this.bones = new Map();
    this.sockets = [];
    this.parts = parts;
    for (const { part, object } of parts) {
      object.position.set(...part.at);
      this.root.add(object);
    }
    this.root.traverse((o) => {
      if (!o.isMesh) return;
      o.castShadow = true;
      o.receiveShadow = true;
      const mats = Array.isArray(o.material) ? o.material : [o.material];
      for (const m of mats) {
        if (o.geometry.attributes.color) m.vertexColors = true;
        if (o.geometry.attributes._wind) addWind(m);
      }
    });
    this.player = { paused: false, speed: 1, time: 0, duration: 0, play() { return true; }, seek() {}, update() {}, dispose() {} };
    this.springs = [];
    this.pieces = new Map();
    this.hair = new Map();
    this.masks = { body: 0 };
    this.clock = 0;
  }

  clipNames() {
    return [];
  }

  update(dt) {
    this.clock += dt;
    windTime.value = this.clock;
    for (const { part, object } of this.parts) {
      if (!part.spin) continue;
      const axis = new THREE.Vector3(...part.spin.axis).normalize();
      object.quaternion.setFromAxisAngle(axis, (this.clock * part.spin.rpm * Math.PI * 2) / 60);
    }
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
    return Math.round(n);
  }

  dispose() {
    this.group.removeFromParent();
    this.root.traverse((o) => {
      if (!o.isMesh) return;
      o.geometry.dispose();
      for (const m of Array.isArray(o.material) ? o.material : [o.material]) m.dispose();
    });
  }
}
