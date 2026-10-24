// Clip curves and events, applied the way docs/ASSETS.md says the game should apply them.
//
// After the mixer has posed the skeleton each frame:
// - `off_hand` pulls the off hand onto a two-handed item, sliding along it to where the clip has
//   that hand (within `grips.off_hand.range`);
// - `reach` pulls the drawing hand onto the item where it rides, while a clip takes or stows it;
// - between a clip's `grab` and its end, the item rides the hand, sliding from the point the
//   hand took it to its grip; before a `stow` it slides the other way;
// - `string` lets the bowstring follow the drawing hand (the bow's `draw` morph) and `arrow`
//   shows the nocked arrow.

import * as THREE from 'three';

export function curve(keys, t) {
  if (!keys || !keys.length) return 0;
  if (t < keys[0][0]) return keys[0][1];
  for (let i = 0; i + 1 < keys.length; i++) {
    const [t0, v0] = keys[i];
    const [t1, v1] = keys[i + 1];
    if (t0 <= t && t < t1) return v0 + ((v1 - v0) * (t - t0)) / (t1 - t0);
  }
  return keys[keys.length - 1][1];
}

function smooth(x) {
  const c = THREE.MathUtils.clamp(x, 0, 1);
  return c * c * (3 - 2 * c);
}

const UP = new THREE.Vector3(0, 1, 0);

/** A socket-like frame (Y up, Z front) as a matrix in its parent's space. */
function frameMatrix(at, up, front) {
  const y = new THREE.Vector3(...up).normalize();
  const f = new THREE.Vector3(...front);
  const z = f.sub(y.clone().multiplyScalar(f.dot(y))).normalize();
  const x = new THREE.Vector3().crossVectors(y, z);
  return new THREE.Matrix4().makeBasis(x, y, z).setPosition(...at);
}

function worldMatrix(obj) {
  obj.updateWorldMatrix(true, false);
  return obj.matrixWorld.clone();
}

/** Turns a bone (in world space) so `from` points along `to`, by `weight`. */
function turnBone(bone, from, to, weight) {
  const q = new THREE.Quaternion().setFromUnitVectors(from, to);
  q.slerp(new THREE.Quaternion(), 1 - weight);
  const world = bone.getWorldQuaternion(new THREE.Quaternion());
  const parent = bone.parent.getWorldQuaternion(new THREE.Quaternion());
  bone.quaternion.copy(parent.invert().multiply(q.multiply(world)));
  bone.updateWorldMatrix(false, true);
}

/**
 * Two-bone IK on an arm: puts the hand's socket on `target` (a world matrix), keeping the
 * elbow on the side the clip bent it, blended by `weight`.
 */
export function armTo(bones, side, target, weight) {
  if (weight <= 0) return;
  const upper = bones.get(`upperarm_${side}`);
  const lower = bones.get(`lowerarm_${side}`);
  const hand = bones.get(`hand_${side}`);
  const socket = bones.get(`socket_hand_${side}`);
  if (!upper || !lower || !hand || !socket) return;
  const want = target.clone().multiply(socket.matrix.clone().invert());
  const goal = new THREE.Vector3().setFromMatrixPosition(want);
  const s = upper.getWorldPosition(new THREE.Vector3());
  const e = lower.getWorldPosition(new THREE.Vector3());
  const w = hand.getWorldPosition(new THREE.Vector3());
  const l1 = s.distanceTo(e);
  const l2 = e.distanceTo(w);
  const d = goal.clone().sub(s);
  const dist = THREE.MathUtils.clamp(d.length(), 1e-4, (l1 + l2) * 0.9995);
  const dn = d.normalize();
  const cos = THREE.MathUtils.clamp((l1 * l1 + dist * dist - l2 * l2) / (2 * l1 * dist), -1, 1);
  const side_ = e.clone().sub(s);
  side_.sub(dn.clone().multiplyScalar(side_.dot(dn)));
  if (side_.lengthSq() < 1e-10) side_.set(0, -1, 0);
  side_.normalize();
  const elbow = s.clone().addScaledVector(dn, l1 * cos).addScaledVector(side_, l1 * Math.sqrt(Math.max(0, 1 - cos * cos)));
  turnBone(upper, e.clone().sub(s).normalize(), elbow.clone().sub(s).normalize(), weight);
  const e2 = lower.getWorldPosition(new THREE.Vector3());
  const w2 = hand.getWorldPosition(new THREE.Vector3());
  turnBone(lower, w2.clone().sub(e2).normalize(), goal.clone().sub(e2).normalize(), weight);
  // The hand takes the target's orientation.
  const wantQ = new THREE.Quaternion().setFromRotationMatrix(want);
  const now = hand.getWorldQuaternion(new THREE.Quaternion());
  now.slerp(wantQ, weight);
  const parent = hand.parent.getWorldQuaternion(new THREE.Quaternion());
  hand.quaternion.copy(parent.invert().multiply(now));
  hand.updateWorldMatrix(false, true);
}

export class Performer {
  constructor(character) {
    this.c = character;
    this.meta = new Map((character.cat.animations.clips || []).map((c) => [c.name, c]));
    this.grabbed = new Map(); // clip name + item id -> the point (metres up the item) the hand took
  }

  /** The equipped item a clip takes out or puts away, if any. */
  itemFor(clip) {
    for (const rec of this.c.gear.values()) {
      const clips = rec.def?.clips;
      if (clips && (clips.ready === clip || clips.stow === clip) && rec.object) return rec;
    }
    return null;
  }

  heldSide(def) {
    return def.held === 'socket_hand_l' ? 'l' : 'r';
  }

  /** How far up the stowed item's axis the hand's grip sits, within the item's grab range. */
  alongStowed(rec, side) {
    const stowed = this.c.bones.get(this.c.stowedSocket(rec.def));
    const hand = this.c.bones.get(`socket_hand_${side}`);
    if (!stowed || !hand) return 0;
    const m = worldMatrix(stowed);
    const origin = new THREE.Vector3().setFromMatrixPosition(m);
    const up = UP.clone().transformDirection(m);
    const h = hand.getWorldPosition(new THREE.Vector3());
    const scale = this.c.def?.gear_scale || 1;
    const [lo, hi] = rec.def.grab_range || [0, 0];
    return THREE.MathUtils.clamp(h.sub(origin).dot(up) / scale, lo, hi);
  }

  /** The point the hand takes a stowed item at, for a clip seeked past its grab. */
  grabPoint(name, meta, rec, side) {
    const key = `${name}:${rec.def.id}`;
    if (this.grabbed.has(key)) return this.grabbed.get(key);
    const clip = this.c.clips.get(name);
    const t = meta.events.find((e) => e.name === 'grab').t;
    // Pose the skeleton at the grab, measure, and put the pose back.
    const saved = [];
    for (const track of clip.tracks) {
      const [node, prop] = [track.name.slice(0, track.name.lastIndexOf('.')), track.name.slice(track.name.lastIndexOf('.') + 1)];
      const bone = this.c.bones.get(node);
      if (!bone || (prop !== 'quaternion' && prop !== 'position')) continue;
      saved.push([bone, prop, bone[prop].clone()]);
      const v = track.createInterpolant().evaluate(t);
      bone[prop].fromArray(v);
    }
    this.c.root.updateMatrixWorld(true);
    this.reach(rec, side, 1);
    const s = this.alongStowed(rec, side);
    for (const [bone, prop, v] of saved) bone[prop].copy(v);
    this.c.root.updateMatrixWorld(true);
    this.grabbed.set(key, s);
    return s;
  }

  reach(rec, side, weight) {
    const stowed = this.c.bones.get(this.c.stowedSocket(rec.def));
    if (!stowed) return;
    const s = this.alongStowed(rec, side) * (this.c.def?.gear_scale || 1);
    const target = worldMatrix(stowed).multiply(new THREE.Matrix4().makeTranslation(0, s, 0));
    armTo(this.c.bones, side, target, weight);
  }

  /** Places a ready/stow clip's item and pulls the hand onto it. Returns true if it did. */
  drawing(name, meta, t) {
    const rec = this.itemFor(name);
    if (!rec) return false;
    const def = rec.def;
    const side = this.heldSide(def);
    const grab = meta.events?.find((e) => e.name === 'grab')?.t;
    const stow = meta.events?.find((e) => e.name === 'stow')?.t;
    const scale = this.c.def?.gear_scale || 1;
    const w = curve(meta.curves?.reach, t);
    // Everything else moves when this item does (a shield comes off the back as the sword is drawn).
    this.c.placeGear(grab !== undefined ? t >= grab : t < stow, rec);
    if (grab !== undefined) {
      if (t < grab) {
        this.c.attach(rec.object, this.c.stowedSocket(def));
        this.reach(rec, side, w);
        this.grabbed.set(`${name}:${def.id}`, this.alongStowed(rec, side));
      } else {
        this.reach(rec, side, w);
        const s = this.grabPoint(name, meta, rec, side);
        this.c.attach(rec.object, def.held);
        rec.object.position.y = -s * scale * (1 - smooth((t - grab) / Math.max(meta.seconds - grab, 1e-3)));
      }
    } else if (stow !== undefined) {
      if (t < stow) {
        this.reach(rec, side, w);
        const s = this.alongStowed(rec, side);
        this.c.attach(rec.object, def.held);
        rec.object.position.y = -s * scale * smooth(t / Math.max(stow, 1e-3));
      } else {
        this.c.attach(rec.object, this.c.stowedSocket(def));
      }
    }
    return true;
  }

  apply() {
    const player = this.c.player;
    const name = player.name;
    const meta = name && this.meta.get(name);
    this.c.root.updateMatrixWorld(true);
    const t = player.time;
    if (!meta || !this.drawing(name, meta, t)) this.c.placeGear();
    if (!meta) return this.bows(null, 0);
    const main = this.c.gear.get('main_hand');
    const off = curve(meta.curves?.off_hand, t);
    const grip = main?.def?.grips?.off_hand;
    if (off > 0 && grip && main.object) {
      const side = this.heldSide(main.def) === 'r' ? 'l' : 'r';
      const obj = main.object;
      obj.updateWorldMatrix(true, false);
      // Slide along the item to where the clip has the hand, within the grip's range.
      const hand = this.c.bones.get(`socket_hand_${side}`).getWorldPosition(new THREE.Vector3());
      const local = obj.worldToLocal(hand);
      const up = new THREE.Vector3(...grip.up).normalize();
      const [lo, hi] = grip.range || [grip.at[1], grip.at[1]];
      const at = new THREE.Vector3(...grip.at);
      const s = THREE.MathUtils.clamp(local.dot(up), lo, hi);
      at.addScaledVector(up, s - at.dot(up));
      const m = frameMatrix(at.toArray(), grip.up, grip.front);
      // The item's scale belongs to the item, not the hand.
      const world = obj.matrixWorld.clone().multiply(m);
      const pos = new THREE.Vector3();
      const quat = new THREE.Quaternion();
      world.decompose(pos, quat, new THREE.Vector3());
      armTo(this.c.bones, side, new THREE.Matrix4().compose(pos, quat, new THREE.Vector3(1, 1, 1)), off);
    }
    this.bows(meta, t);
  }

  /** The bowstring follows the drawing hand while the clip holds it; the arrow shows when nocked. */
  bows(meta, t) {
    for (const rec of this.c.gear.values()) {
      const s = rec.def?.string;
      if (!s || !rec.object) continue;
      const side = this.heldSide(rec.def) === 'l' ? 'r' : 'l';
      const hold = meta ? curve(meta.curves?.string, t) : 0;
      const arrow = meta ? curve(meta.curves?.arrow, t) : 0;
      const hand = this.c.bones.get(`socket_hand_${side}`)?.getWorldPosition(new THREE.Vector3());
      rec.object.traverse((o) => {
        if (!o.isMesh || !o.morphTargetDictionary) return;
        const local = hand ? o.worldToLocal(hand.clone()) : new THREE.Vector3();
        const rest = new THREE.Vector3(...s.rest);
        const pull = new THREE.Vector3(...s.pull);
        const w = THREE.MathUtils.clamp(local.sub(rest).dot(pull) / pull.lengthSq(), 0, 1) * hold;
        const di = o.morphTargetDictionary.draw;
        const ai = o.morphTargetDictionary.arrow;
        if (di !== undefined) o.morphTargetInfluences[di] = w;
        if (ai !== undefined) o.morphTargetInfluences[ai] = arrow;
      });
    }
  }
}
