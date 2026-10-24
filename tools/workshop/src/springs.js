// Spring bones for capes and other hanging cloth: a VRM-style verlet chain solver.
//
// Each joint keeps a simulated tail point in world space. Every fixed step the tail carries its
// inertia, is pulled back toward the bone's animated rest direction (stiffness), sinks (gravity),
// keeps its length and is pushed out of capsule colliders. Every frame each bone is then turned to
// point at its tail, so the chain's root stays exactly on the animated body.
//
// Units follow VRM: stiffness and gravity are metres per second of pull; drag is the share of
// velocity lost per 1/60 s. The solver steps at a fixed 120 Hz, so it behaves the same at any
// frame rate.

import * as THREE from 'three';

const STEP = 1 / 120;
const MAX_STEPS = 8;

const _parent = new THREE.Quaternion();
const _swing = new THREE.Quaternion();
const _local = new THREE.Quaternion();
const _from = new THREE.Vector3();
const _to = new THREE.Vector3();
const _head = new THREE.Vector3();
const _rest = new THREE.Vector3();
const _next = new THREE.Vector3();
const _vel = new THREE.Vector3();
const _ab = new THREE.Vector3();
const _ap = new THREE.Vector3();
const _closest = new THREE.Vector3();
const _inv = new THREE.Matrix4();

class Joint {
  constructor(bone, child, radius) {
    this.bone = bone;
    this.radius = radius;
    this.rest = bone.quaternion.clone();
    if (child) {
      this.axis = child.position.clone();
    } else {
      // The last joint gets a virtual tail continuing its own offset from the parent.
      this.axis = bone.position.lengthSq() > 1e-10 ? bone.position.clone() : new THREE.Vector3(0, -0.07, 0);
      this.axis.setLength(Math.min(this.axis.length(), 0.08));
    }
    this.length = this.axis.length();
    this.axis.normalize();
    this.tail = new THREE.Vector3();
    this.prev = new THREE.Vector3();
  }

  /** The rest direction in world space under the current parent pose. */
  restDirection(out) {
    this.bone.parent.getWorldQuaternion(_parent);
    _local.copy(_parent).multiply(this.rest);
    return out.copy(this.axis).applyQuaternion(_local);
  }

  reset() {
    this.bone.quaternion.copy(this.rest);
    this.bone.updateMatrixWorld(true);
    this.bone.getWorldPosition(this.tail).addScaledVector(this.restDirection(_rest), this.length);
    this.prev.copy(this.tail);
  }

  /** Turns the bone to point at its simulated tail. */
  aim() {
    this.bone.quaternion.copy(this.rest);
    this.bone.updateMatrixWorld(true);
    this.restDirection(_from);
    this.bone.getWorldPosition(_head);
    _to.copy(this.tail).sub(_head);
    if (_to.lengthSq() < 1e-12) return;
    _to.normalize();
    _swing.setFromUnitVectors(_from, _to);
    this.bone.parent.getWorldQuaternion(_parent);
    // local = parent⁻¹ · swing · parent · rest
    _local.copy(_parent).invert().multiply(_swing).multiply(_parent).multiply(this.rest);
    this.bone.quaternion.copy(_local);
    this.bone.updateMatrixWorld(true);
  }
}

class Capsule {
  constructor(bone, a, b, radius) {
    this.bone = bone;
    this.a = new THREE.Vector3().fromArray(a);
    this.b = new THREE.Vector3().fromArray(b);
    this.radius = radius;
    this.wa = new THREE.Vector3();
    this.wb = new THREE.Vector3();
  }

  refresh() {
    this.wa.copy(this.a).applyMatrix4(this.bone.matrixWorld);
    this.wb.copy(this.b).applyMatrix4(this.bone.matrixWorld);
  }

  /** Pushes p out of the capsule, padded by r. Returns true if it moved. */
  push(p, r) {
    _ab.copy(this.wb).sub(this.wa);
    const len2 = _ab.lengthSq();
    const t = len2 > 1e-12 ? THREE.MathUtils.clamp(_ap.copy(p).sub(this.wa).dot(_ab) / len2, 0, 1) : 0;
    _closest.copy(this.wa).addScaledVector(_ab, t);
    _ap.copy(p).sub(_closest);
    const dist = _ap.length();
    const min = this.radius + r;
    if (dist >= min) return false;
    if (dist < 1e-6) _ap.set(0, 0, -1);
    else _ap.divideScalar(dist);
    p.copy(_closest).addScaledVector(_ap, min);
    return true;
  }
}

export class SpringBones {
  /**
   * chains: arrays of bones, root first. options: { stiffness, drag, gravity, radius }.
   * colliders: [{ bone, radius, a, b }], capsules in the bone's local space.
   */
  constructor(chains, options = {}, colliders = []) {
    this.stiffness = options.stiffness ?? 0.6;
    this.drag = options.drag ?? 0.4;
    this.gravity = options.gravity ?? 1.0;
    this.gravityDir = new THREE.Vector3(0, -1, 0);
    const radius = options.radius ?? 0.02;
    this.chains = chains
      .map((bones) => bones.filter(Boolean))
      .filter((bones) => bones.length)
      .map((bones) => bones.map((bone, i) => new Joint(bone, bones[i + 1] || null, radius)));
    this.colliders = colliders.map((c) => new Capsule(c.bone, c.a, c.b, c.radius));
    this.acc = 0;
    this.ready = false;
  }

  get joints() {
    return this.chains.flat();
  }

  reset() {
    for (const chain of this.chains) for (const j of chain) j.reset();
    this.acc = 0;
    this.ready = true;
  }

  /** Puts every bone back to its rest rotation (before removing the system). */
  restore() {
    for (const j of this.joints) j.bone.quaternion.copy(j.rest);
  }

  step(dt) {
    for (const c of this.colliders) c.refresh();
    const keep = Math.pow(1 - THREE.MathUtils.clamp(this.drag, 0, 1), dt * 60);
    for (const chain of this.chains) {
      for (const j of chain) {
        j.bone.quaternion.copy(j.rest);
        j.bone.updateMatrixWorld(true);
        j.bone.getWorldPosition(_head);
        j.restDirection(_rest);
        _vel.copy(j.tail).sub(j.prev);
        _next.copy(j.tail)
          .addScaledVector(_vel, keep)
          .addScaledVector(_rest, this.stiffness * dt)
          .addScaledVector(this.gravityDir, this.gravity * dt);
        _next.sub(_head).setLength(j.length).add(_head);
        for (const c of this.colliders) {
          if (c.push(_next, j.radius)) _next.sub(_head).setLength(j.length).add(_head);
        }
        j.prev.copy(j.tail);
        j.tail.copy(_next);
        j.aim();
      }
    }
  }

  /** Runs after the animation mixer, every frame. */
  update(dt) {
    if (!this.ready) this.reset();
    this.acc = Math.min(this.acc + Math.max(dt, 0), STEP * MAX_STEPS);
    let steps = 0;
    while (this.acc >= STEP && steps < MAX_STEPS) {
      this.step(STEP);
      this.acc -= STEP;
      steps++;
    }
    if (steps === 0) {
      // No step this frame: still follow the body, aiming at the last simulated tails.
      for (const j of this.joints) j.aim();
    }
  }
}

/**
 * Fallback collision capsules from the skeleton itself, for bodies whose catalogue entry has no
 * `colliders` yet: the torso, pelvis, thighs, shins and upper arms, each from a bone to its child.
 */
export function defaultColliders(bones, scale = 1) {
  const spec = [
    ['pelvis', 'spine_01', 0.125],
    ['spine_01', 'spine_02', 0.13],
    ['spine_02', 'spine_03', 0.135],
    ['spine_03', 'neck_01', 0.14],
    ['thigh_l', 'calf_l', 0.085],
    ['thigh_r', 'calf_r', 0.085],
    ['calf_l', 'foot_l', 0.06],
    ['calf_r', 'foot_r', 0.06],
    ['upperarm_l', 'lowerarm_l', 0.055],
    ['upperarm_r', 'lowerarm_r', 0.055],
  ];
  const out = [];
  for (const [from, to, r] of spec) {
    const a = bones.get(from);
    const b = bones.get(to);
    if (!a || !b) continue;
    a.updateMatrixWorld(true);
    _inv.copy(a.matrixWorld).invert();
    const tip = b.getWorldPosition(new THREE.Vector3()).applyMatrix4(_inv);
    out.push({ bone: a, a: [0, 0, 0], b: tip.toArray(), radius: r * scale });
  }
  return out;
}

/** Catalogue colliders ({ bone: name, radius, a, b }) resolved against a bone map. */
export function resolveColliders(list, bones) {
  return (list || [])
    .map((c) => ({ ...c, bone: bones.get(c.bone) }))
    .filter((c) => c.bone);
}
