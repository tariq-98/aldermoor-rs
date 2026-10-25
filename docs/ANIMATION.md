# Character animation

How a character's pose is built each frame, for whoever works on it next. The clip stack is
`Character::update` in `crates/render/src/character.rs`. Everything that doesn't need the renderer
lives in `crates/anim`, which builds and tests on Linux CI (`render` only builds on macOS). The
research and the plan behind this, and the order of the remaining work, are in the
[animation plan](https://claude.ai/artifact/9MxHac5iBszDUr7wKMsoi5).

The sim is authoritative and moves bodies at 60 Hz. Clips play in place, and the client fits them
to the motion it is shown; nothing on the client moves a body.

## A frame

`Character::update_combat` (the client calls it once a frame for each character in view) records the
unit's combat view and calls `update`, which builds the pose in this order:

1. **Base clip** (`clip_for`): the action's clip if one is playing (`action_clip`), otherwise the
   pose's: idle, walk, jog or sprint by speed, jump, fall, roll, hit or death. Beasts map poses to
   their role clips.
2. **Sample it** into `local`, starting from the rest pose (`apply_clip` with `Bones::All`). Walk,
   jog and sprint all run on one stride phase (`stride`), so changing between them never restarts
   the feet. Each clip's tracks are bound to the skeleton's bones once (`ClipSet`, #204), and every
   character with the same skeleton and clips shares it through `ModelCache::clip_set`. A clip
   samples only the tracks the body uses: humanoid clips turn every bone but move only the pelvis,
   and never scale.
3. **Legs under actions** (#186): an attack or cast on the move keeps the jog on the legs
   (`walk_under`, `Bones::Lower`) and turns the chest back to where the action faced it, within
   `MAX_TWIST` of the hips.
4. **Upper-body layers** (`Bones::Upper` is `spine_01` and everything under it), in this order, the
   last one applied winning: a drawn weapon's idle while moving, the guard (or the parry), and the
   ready or stow transition.
5. **World matrices**, then `perform`: the hand reaching for the sheath, the off hand on a
   two-handed grip, and the bow's string and arrow morphs.
6. **The blender** (`anim::Blender`): when anything switched since the last frame it starts
   carrying the old motion into the new, and while a switch settles it changes `local`, so the
   world matrices are rebuilt. It runs after `perform` so that a switch also smooths the hands
   letting go of or taking a grip.
7. **Spring bones** (capes) settle on top.

## Switches

A switch is any change in what plays: the base clip's name, the legs' clip, the pose, the upper
layer, an action starting over (`update_combat` sees a new action, even with the same clip) or a
LOD swap (`set_lod`). Each is given a kind (`switch_kind`), and each kind has its half-life in one
table, `anim::Switch::half_life`:

| Kind | Half-life | When |
| --- | --- | --- |
| `Locomotion` | 0.09 s | idle, walk, jog and sprint changing, and the legs under an action |
| `Action` | 0.05 s | an attack, cast, roll, jump or hit starting, or starting over |
| `Recover` | 0.10 s | back to moving or standing as an action ends |
| `Layer` | 0.07 s | the guard, ready, stow or a drawn idle coming or going |
| `Land` | 0.06 s | touching down from a jump or fall |
| `Detail` | 0.05 s | a LOD swap |

The half-life is the spring's, in Holden's sense: a difference with no speed of its own is down to
60% after one, 8% after three and under 1% after five.

**When adding to the stack:** anything that can change the pose in a jump has to show up in the
switch check. Give a new layer a name in `upper`, compare a new field in the same place, or set
`forced_switch`. A switch nobody needed costs a few microseconds and shows nothing; a missed one
pops.

## The blender

`anim::Blender` reads the old motion from the poses it actually showed on the last two frames, so
a switch that lands while another is still settling starts from wherever that one got to, and
switches stack without popping. Three methods, picked with the client's `--blend` flag:

- **`Method::Inertialize`** (the default; Bollo, "Inertialization", GDC 2018; Holden,
  "Spring-It-On"). The switch frame shows the old motion carried on by its own velocity. Each
  bone's difference from the new pose becomes an offset that is added to the new clip and settles
  on a critically damped spring. On the next frame, when the new clip's velocity is known, the
  offset is given the velocity that keeps the shown motion at the old motion's speed, so velocity
  as well as pose is continuous. That velocity goes through the inverse of SO(3)'s Jacobian,
  because the spring runs on the offset's rotation vector: Holden's code leaves that out, which
  shows as a jolt on big offsets. Bones the switch didn't change come out with no offset and keep
  moving exactly as before. Offsets under 0.06° are dropped and cost nothing more.
- **`Method::DeadBlend`** (`--blend dead`; Holden, "Dead Blending", 2023, and Unreal's Dead
  Blending node). The old pose carries on with its velocity dying away, slowly if it heads toward
  the new pose and quickly otherwise, and cross-fades into the new pose over 2.5 half-lives. Bones
  the switch barely moved take the new pose at once. Kept to compare in captures.
- **`Method::Off`** (`--blend off`) cuts on every switch. The clip-stack tests use it.

## Tests

- `cargo test -p anim` (Linux and macOS): pose and velocity stay continuous through one switch
  and through three inside 100 ms (velocity changes from frame to frame shrink in step with the
  frame time, which a pop or a velocity jump can't do), a 90° offset is under 10% after three
  half-lives and under 1° after five, bones a switch leaves alone carry on untouched, and a
  settled blender shows the clips exactly.
- `cargo test -p render` (macOS): `switches_carry_the_motion_on_instead_of_popping` plays jog,
  attack on the move, guard, stop, roll and run through a real character and checks that every
  bone carries on at every switch, where cuts jump by over 0.5 rad. The other clip-stack tests
  set `Method::Off`.

## Cost

`cargo run --release -p render --example anim_bench -- [characters] [frames] [case]` times
`update_combat` (the pose) and `draws` (the skinning palettes) for a plate knight with sword,
shield and cape, a ranger with a bow and an elf mage with a staff, at each LOD. The cases: `idle`
and `run` hold their pose, `slash` and `run + slash` start the slash over every half second, and
`start-stop` runs and stops every half second, so those three are blending most of the time.

Instructions per `update_combat`, from callgrind (6 characters, 240 frames, every LOD):

| Case | Before #210 | After #210 | After #204 |
| --- | --- | --- | --- |
| idle | 166,229 | 140,466 | 83,313 |
| run | 210,673 | 184,868 | 88,894 |
| slash | | 162,570 | 106,448 |
| run + slash | 228,680 | 222,870 | 113,124 |
| start-stop | 187,279 | 185,645 | 110,360 |

On the M1, #210 measured 11 to 16 µs a character a frame. Holding a pose, the blender only keeps
a copy of the last two poses shown, and it took out the old crossfade, which slerped every bone
every frame even when nothing was blending. While a switch settles it runs the spring for each
bone that still has an offset and rebuilds the world matrices a second time, which the one-pass
rebuild (parents first, into the same buffer) pays for on x86; on the M1, cases that blend all the
time came out 2% to 4% dearer than before #210. #204 stopped looking up each track's bone by name
and sampling tracks the body ignores, which was a third to a half of the instructions.

Where the time goes now (callgrind, `run`): half in cape springs, about 15% each in sampling clips
and in world matrices, and about 15% in cloning per frame (`perform` copies the weapon's item and
the clip's metadata, and clip names are `String`s). #215 takes on both: springs only at full
detail, and no allocations per frame.

## Next

In the plan's order: #110 direction blending with orientation and stride warping, #211 turning in
place (a root yaw offset ahead of the clip stack), #215 animation LOD, #214 foot IK and foot
locking (after the blender, so planted feet stay planted while it settles), #212 aim pitch and
look-at, #217 lean, landings and the idle phase, #218 hit flinches and hitstop (on `Event::Hit`,
#191), and #213 traversal (after #192).

One thing to watch: combat clips take their time from `UnitCombat::action_time` in each snapshot.
Snapshots go out every tick today; if the send rate drops to 20–30 Hz (#73), attacks would play in
steps unless the client runs their clock between snapshots.
