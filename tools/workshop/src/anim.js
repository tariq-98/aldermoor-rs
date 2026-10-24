// The shared clip library and per-body playback.
//
// Every humanoid skeleton shares the library's rest orientations, so rotation tracks apply as
// they are. Bone translations come from each skeleton's own rest pose, except the pelvis, whose
// clip translation is remapped by leg length: rest_body + (clip - rest_anim) * ratio.

import * as THREE from 'three';

/** Loads anims.glb: the clips plus the library skeleton's rest pelvis translation. */
export async function loadLibrary(loader, url) {
  const gltf = await loader.loadAsync(url);
  let pelvis = null;
  gltf.scene.traverse((o) => {
    if (o.name === 'pelvis') pelvis = o.position.clone();
  });
  return { clips: gltf.animations, restPelvis: pelvis };
}

/** The library clips rewritten for one skeleton. */
export function prepareClips(library, restBodyPelvis, ratio) {
  const out = new Map();
  for (const clip of library.clips) {
    if (clip.name.startsWith('A_TPose')) continue; // The library's reference pose, not a move.
    const tracks = [];
    for (const track of clip.tracks) {
      const dot = track.name.lastIndexOf('.');
      const node = track.name.slice(0, dot);
      const prop = track.name.slice(dot + 1);
      if (prop === 'scale') continue;
      if (prop === 'position') {
        if (node !== 'pelvis') continue;
        const v = Float32Array.from(track.values);
        const r = library.restPelvis;
        for (let i = 0; i < v.length; i += 3) {
          v[i] = restBodyPelvis.x + (v[i] - r.x) * ratio;
          v[i + 1] = restBodyPelvis.y + (v[i + 1] - r.y) * ratio;
          v[i + 2] = restBodyPelvis.z + (v[i + 2] - r.z) * ratio;
        }
        tracks.push(new THREE.VectorKeyframeTrack(track.name, track.times, v));
        continue;
      }
      tracks.push(track);
    }
    out.set(clip.name, new THREE.AnimationClip(clip.name, clip.duration, tracks));
  }
  return out;
}

/** Drives one skeleton: play, crossfade, pause, speed and scrubbing. */
export class Player {
  constructor(root, clips) {
    this.mixer = new THREE.AnimationMixer(root);
    this.clips = clips;
    this.action = null;
    this.actions = new Set();
    this.name = null;
    this.paused = false;
    this.speed = 1;
  }

  play(name, fade = 0.25) {
    const clip = this.clips.get(name);
    if (!clip) return false;
    const next = this.mixer.clipAction(clip);
    next.setLoop(THREE.LoopRepeat, Infinity);
    next.enabled = true;
    next.setEffectiveTimeScale(1);
    next.setEffectiveWeight(1);
    if (this.action && this.action !== next && fade > 0) {
      next.reset().play();
      this.action.crossFadeTo(next, fade, false);
    } else {
      if (this.action && this.action !== next) this.action.stop();
      next.reset().play();
    }
    this.actions.add(next);
    this.action = next;
    this.name = name;
    return true;
  }

  /** Plays a clip once and holds its last frame; `onDone` runs when it ends. */
  playOnce(name, fade = 0.2, onDone = null) {
    if (!this.play(name, fade)) return false;
    const action = this.action;
    action.setLoop(THREE.LoopOnce, 1);
    action.clampWhenFinished = true;
    if (onDone) {
      const done = (e) => {
        if (e.action !== action) return;
        this.mixer.removeEventListener('finished', done);
        if (this.action === action) onDone();
      };
      this.mixer.addEventListener('finished', done);
    }
    return true;
  }

  get duration() {
    return this.action ? this.action.getClip().duration : 0;
  }

  get time() {
    return this.action ? this.action.time : 0;
  }

  /** Jumps to a time and stops any running crossfade, so the pose is exact. */
  seek(t) {
    if (!this.action) return;
    for (const a of this.actions) if (a !== this.action) a.stop();
    this.action.setEffectiveWeight(1);
    this.action.time = THREE.MathUtils.clamp(t, 0, this.duration);
    this.mixer.update(0);
  }

  update(dt) {
    if (!this.paused) this.mixer.update(dt * this.speed);
  }

  dispose() {
    this.mixer.stopAllAction();
    this.mixer.uncacheRoot(this.mixer.getRoot());
  }
}
