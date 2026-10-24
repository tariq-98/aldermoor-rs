// Body regions: hide covered triangles in the vertex shader, and colour them for debugging.
//
// Every skinned piece carries a float vertex attribute `_region` (glTF `_REGION`), the index of
// its body region (see docs/ASSETS.md). A per-mesh integer bitmask uniform says which regions to
// drop; their vertices are pushed outside clip space, so the whole triangle is clipped.
//
// The same patch applies dyes: a mask (R main cloth, G trim) recolours the base texture as
// out = mix(out, base * dye / max(reference, 0.01), mask), before lighting.

import * as THREE from 'three';

// One colour per region index for the debug view (matches the Blender review palette).
const PALETTE = [
  [0.95, 0.85, 0.2], [1.0, 0.55, 0.6], [0.9, 0.3, 0.9], [0.55, 0.35, 0.2],
  [0.2, 0.5, 1.0], [0.1, 0.8, 0.8], [0.4, 0.2, 0.7],
  [1.0, 0.3, 0.2], [1.0, 0.65, 0.2], [0.3, 0.85, 0.3], [0.1, 0.5, 0.15], [0.95, 0.95, 0.95],
  [0.6, 0.6, 1.0], [0.2, 0.2, 0.6], [0.9, 0.5, 0.1], [0.55, 0.25, 0.05], [0.5, 0.5, 0.5],
  [0.1, 0.1, 0.1],
];

const PALETTE_GLSL = `const vec3 regionPalette[${PALETTE.length}] = vec3[](${PALETTE
  .map((c) => `vec3(${c.map((x) => x.toFixed(3)).join(', ')})`)
  .join(', ')});`;

// Shared by every patched material: the debug colouring switch.
export const regionDebug = { value: 0 };

const VERTEX_HEAD = `#include <common>
attribute float _region;
uniform int uHidden;
varying float vRegion;`;

const VERTEX_TAIL = `#include <project_vertex>
	vRegion = _region;
	int regionBit = int(_region + 0.5);
	if (regionBit >= 0 && regionBit < 31 && ((uHidden >> regionBit) & 1) == 1) {
		gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
	}`;

const FRAGMENT_HEAD = `#include <common>
uniform int uRegionDebug;
uniform sampler2D uDyeMap;
uniform sampler2D uMarkings;
uniform vec3 uMarkLight;
uniform vec3 uMarkDark;
uniform float uMarkOn;
uniform vec2 uDyeOn;
uniform vec3 uDyeR;
uniform vec3 uDyeRefR;
uniform vec3 uDyeG;
uniform vec3 uDyeRefG;
varying float vRegion;
${PALETTE_GLSL}`;

const FRAGMENT_DYE = `#include <map_fragment>
#if defined(USE_MAP) && !defined(DYEABLE)
	// Fur markings: the tint (the coat) gives way to the light and dark fur where the mask says.
	if (uMarkOn > 0.0) {
		vec4 marks = texture2D(uMarkings, vMapUv);
		vec3 coat = diffuse;
		vec3 fur = mix(mix(coat, uMarkLight, marks.r), uMarkDark, marks.g);
		diffuseColor.rgb *= fur / max(coat, vec3(0.001));
	}
#endif
#if defined(USE_MAP) && defined(DYEABLE)
	if (uDyeOn.x > 0.0 || uDyeOn.y > 0.0) {
		vec4 dyeMask = texture2D(uDyeMap, vMapUv);
		vec3 dyeBase = diffuseColor.rgb;
		diffuseColor.rgb = mix(diffuseColor.rgb, dyeBase * uDyeR / max(uDyeRefR, vec3(0.01)), dyeMask.r * uDyeOn.x);
		diffuseColor.rgb = mix(diffuseColor.rgb, dyeBase * uDyeG / max(uDyeRefG, vec3(0.01)), dyeMask.g * uDyeOn.y);
	}
#endif`;

const FRAGMENT_COLOUR = `#include <color_fragment>
	if (uRegionDebug == 1) {
		int r = clamp(int(vRegion + 0.5), 0, ${PALETTE.length - 1});
		diffuseColor.rgb = regionPalette[r];
	}`;

/** Bitmask for a list of region names. */
export function bitmask(names, regions) {
  let mask = 0;
  for (const name of names) {
    const i = regions.indexOf(name);
    if (i >= 0 && i < 31) mask |= 1 << i;
  }
  return mask;
}

/**
 * Patches a material (already cloned for this mesh) to hide regions. Returns the mesh's uniform,
 * whose `value` is the bitmask of hidden regions.
 */
export function patchMaterial(material, hidden, dye) {
  // Skin and hair take tints, not dyes.
  const dyeable = !/^(skin|hair)/.test(material.name || '');
  if (dyeable) material.defines = { ...(material.defines || {}), DYEABLE: '' };
  material.onBeforeCompile = (shader) => {
    shader.uniforms.uHidden = hidden;
    shader.uniforms.uRegionDebug = regionDebug;
    shader.uniforms.uDyeMap = dye.map;
    shader.uniforms.uMarkings = dye.markings;
    shader.uniforms.uMarkLight = dye.light;
    shader.uniforms.uMarkDark = dye.dark;
    shader.uniforms.uMarkOn = dye.markOn;
    shader.uniforms.uDyeOn = dye.on;
    shader.uniforms.uDyeR = dye.r;
    shader.uniforms.uDyeRefR = dye.refR;
    shader.uniforms.uDyeG = dye.g;
    shader.uniforms.uDyeRefG = dye.refG;
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', VERTEX_HEAD)
      .replace('#include <project_vertex>', VERTEX_TAIL);
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', FRAGMENT_HEAD)
      .replace('#include <map_fragment>', FRAGMENT_DYE)
      .replace('#include <color_fragment>', FRAGMENT_COLOUR);
  };
  material.customProgramCacheKey = () => (dyeable ? 'aldermoor-regions-dye' : 'aldermoor-regions');
  material.needsUpdate = true;
}

/** A shadow-casting material that hides the same regions, so hidden skin casts no shadow. */
export function depthMaterial(hidden) {
  const m = new THREE.MeshDepthMaterial({ depthPacking: THREE.RGBADepthPacking });
  m.onBeforeCompile = (shader) => {
    shader.uniforms.uHidden = hidden;
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', `#include <common>
attribute float _region;
uniform int uHidden;`)
      .replace('#include <project_vertex>', `#include <project_vertex>
	int regionBit = int(_region + 0.5);
	if (regionBit >= 0 && regionBit < 31 && ((uHidden >> regionBit) & 1) == 1) {
		gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
	}`);
  };
  m.customProgramCacheKey = () => 'aldermoor-regions-depth';
  return m;
}

/**
 * Prepares a mesh for region hiding: clones its materials (uniforms are per mesh) and patches
 * them if the geometry has the `_region` attribute. Returns { mesh, hidden, hasRegion }.
 */
export function prepareMesh(obj) {
  const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
  const cloned = mats.map((m) => m.clone());
  obj.material = Array.isArray(obj.material) ? cloned : cloned[0];
  obj.castShadow = true;
  obj.receiveShadow = true;
  obj.frustumCulled = false; // Skinned bounds don't follow the animation.
  const hasRegion = !!obj.geometry.getAttribute('_region');
  const hidden = { value: 0 };
  const dye = {
    map: { value: null },
    on: { value: new THREE.Vector2(0, 0) },
    r: { value: new THREE.Color(1, 1, 1) },
    refR: { value: new THREE.Color(1, 1, 1) },
    g: { value: new THREE.Color(1, 1, 1) },
    refG: { value: new THREE.Color(1, 1, 1) },
    markings: { value: null },
    light: { value: new THREE.Color(1, 1, 1) },
    dark: { value: new THREE.Color(0, 0, 0) },
    markOn: { value: 0 },
  };
  if (hasRegion) {
    for (const m of cloned) patchMaterial(m, hidden, dye);
    obj.customDepthMaterial = depthMaterial(hidden);
  }
  return { mesh: obj, hidden, dye, hasRegion };
}

/** prepareMesh for every mesh under `root`. */
export function prepareMeshes(root) {
  const meshes = [];
  root.traverse((obj) => {
    if (obj.isMesh) meshes.push(obj);
  });
  return meshes.map(prepareMesh);
}
