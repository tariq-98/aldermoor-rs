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
2. **Sample it** into `local`, starting from the rest pose (`apply_clip` with `Bones::All`). A
   moving humanoid plays two of its walk, jog and sprint loops blended (`apply_step`), picked and
   timed by its stride (see Locomotion below). Each clip's tracks are bound to the skeleton's bones
   once (`ClipSet`, #204), and every character with the same skeleton and clips shares it through
   `ModelCache::clip_set`. A clip samples only the tracks the body uses: humanoid clips turn every
   bone but move only the pelvis, and never scale.
3. **Legs under actions** (#186): an attack or cast on the move keeps the stride on the legs
   (`walk_under`, `Bones::Lower`), and notes where the action faced the chest.
4. **Upper-body layers** (`Bones::Upper` is `spine_01` and everything under it), in this order, the
   last one applied winning: a drawn weapon's idle while moving, the guard (or the parry), and the
   ready or stow transition.
5. **Hips toward the travel** (`turn_hips`, #110): the hips turn round the vertical toward where
   the body travels, taking the legs and planted feet with them, and the spine turns the chest back,
   spread evenly over its three bones: to the front, or under an action to where the action faced
   it. The chest stops `MAX_TWIST` (60°) round from the hips, or at the clip's own twist if that is
   more.
6. **World matrices**, then `perform`: the hand reaching for the sheath, the off hand on a
   two-handed grip, and the bow's string and arrow morphs.
7. **The blender** (`anim::Blender`): when anything switched since the last frame it starts
   carrying the old motion into the new, and while a switch settles it changes `local`, so the
   world matrices are rebuilt. It runs after `perform` so that a switch also smooths the hands
   letting go of or taking a grip.
8. **Spring bones** (capes) settle on top.

## Locomotion

The sim moves bodies at any speed in any direction, and under action controls the facing and the
travel are separate: strafing, backpedalling and running round in circles while facing one way.
The clips are three loops that run straight ahead in place (`LOOPS`: walk, jog and sprint).
`anim::locomotion` (#110) fits them to the motion, the way Paragon and Lyra do:

- **Gaits, measured once.** `Gait::measure` reads each loop's feet (the `ball_l` and `ball_r`
  bones) at 64 times through its cycle: when each foot touches down, and how fast a planted foot
  slides back, which is the speed the loop would carry the body. The first character to play a
  set of clips measures them (`measure_gaits`, kept in `ClipSet::gaits`), so every body measures
  its own: the male's walk is 1.03 m/s, jog 6.02 and sprint 9.25; the goblin's 0.64, 3.93 and
  5.73. A body whose loops don't measure plays them as before, by `stride_rate`.
- **One stride phase** (`anim::Stride`): 0 is the left foot touching down and 0.5 the right.
  `Gait::time_at` maps it to each loop's own time, so both loops in a blend plant the same foot
  together, and changing speed never restarts the feet.
- **Blend by speed** (`locomotion::blend`): within a tenth of a loop's own speed (`SOLO`) that loop
  plays alone, a little slower or faster, so the sim's run and sprint sample one clip. Between two
  loops' ranges the pair blends, at the weight whose blended stride over its blended cycle is the
  speed, so the clips keep close to their own tempo and the weight reaches 0 and 1 just where the
  ranges start. A sim walk of 2.5 m/s is the walk about two thirds and the jog one third.
- **The phase follows the ground.** Each frame it moves on by the speed over the stride length of
  the step under way, so a planted foot moves back exactly as fast as the body moves on. Below the
  walk's speed and above the sprint's it plays between 0.8 and 1.25 of the loops' pace (`PACE`),
  and past that the feet slide, until stride warping (#214) lengthens or shortens the steps.
- **Hips toward the travel** (`locomotion::hip_turn`): turned toward the travel up to 100° round
  from the facing, and past that toward the opposite way with the loops playing backward, so
  backpedalling is the run in reverse. The change between the two is 10° either side of square
  (`HYSTERESIS`), so a strafe straight to the side doesn't flicker. The phase moves on by the
  travel's share along the hips, so while the hips are still swinging round the stride slows
  instead of the feet skating.
- **Springs.** The sim changes velocity at once. The speed and the hip turn the legs show follow it
  on critically damped springs, with half-lives of 0.1 s and 0.08 s, and snap when the character
  starts moving.

Not done yet: steps lengthened or shortened to the speed and feet pinned to the ground (#214),
slopes, turning on the spot (#211), and a single loop at the furthest LOD (#215). Slow walking
slides most: the walk clip's trailing foot slides forward as it lifts, about 4 cm a step even at
the clip's own 1 m/s, and between the walk's speed and the jog's (backing off behind a raised
guard is 1.5 m/s) the blend can't hide it. Foot locking (#214) is the fix.

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
  settled blender shows the clips exactly. For locomotion: measuring finds synthetic feet's
  touchdowns and speed (and refuses feet that never lift), every loop plants its feet on the same
  phases, a loop plays alone near its own speed and the blend between moves steadily with the
  speed, the hips turn toward the travel and change to backward only past 100° (80° coming back),
  and changing speed and direction never restarts the stride.
- `cargo test -p render` (macOS): `switches_carry_the_motion_on_instead_of_popping` plays jog,
  attack on the move, guard, stop, roll and run through a real character and checks that every
  bone carries on at every switch, where cuts jump by over 0.5 rad. The other clip-stack tests
  set `Method::Off`. `planted_feet_stay_put_walking_running_and_sprinting_every_way` walks, runs
  and sprints a knight ahead, to each side and backward, and fails if a planted foot slides 2 cm
  over the ground (backing off at a walk, no more than the walk clip slides on its own).
  `strafing_turns_the_hips_and_keeps_the_chest_to_the_front` checks the hips against the travel
  and the chest against the facing, beside a twin running straight with the same stride.
- The client's `--gait` run (`crates/client/src/gait.rs`) is for captures: facing one way, the
  player runs ahead, strafes, backpedals, sweeps the travel round every direction, walks, sprints
  and walks; then turns round, draws a weapon and runs the pattern again. It logs the frame each
  leg starts on, so a capture can be cut by leg.

## Cost

`cargo run --release -p render --example anim_bench -- [characters] [frames] [case]` times
`update_combat` (the pose) and `draws` (the skinning palettes) for a plate knight with sword,
shield and cape, a ranger with a bow and an elf mage with a staff, at each LOD. The cases: `idle`,
`walk` (2.5 m/s, between the walk and jog loops), `run` and `strafe` (running square to the facing)
hold their pose, `slash` and `run + slash` start the slash over every half second, and
`start-stop` runs and stops every half second, so those three are blending most of the time.

Instructions per `update_combat`, from callgrind (6 characters, 240 frames, every LOD):

| Case | Before #210 | After #210 | After #204 | After #110 |
| --- | --- | --- | --- | --- |
| idle | 166,229 | 140,466 | 83,313 | 83,697 |
| walk | | | 88,445 | 104,860 |
| run | 210,673 | 184,868 | 88,894 | 93,491 |
| strafe | | | 88,893 | 93,947 |
| slash | | 162,570 | 106,448 | 106,852 |
| run + slash | 228,680 | 222,870 | 113,124 | 115,674 |
| start-stop | 187,279 | 185,645 | 110,360 | 112,854 |

On the M1, #210 measured 11 to 16 µs a character a frame. Holding a pose, the blender only keeps
a copy of the last two poses shown, and it took out the old crossfade, which slerped every bone
every frame even when nothing was blending. While a switch settles it runs the spring for each
bone that still has an offset and rebuilds the world matrices a second time, which the one-pass
rebuild (parents first, into the same buffer) pays for on x86; on the M1, cases that blend all the
time came out 2% to 4% dearer than before #210. #204 stopped looking up each track's bone by name
and sampling tracks the body ignores, which was a third to a half of the instructions. #110 samples
a second loop while the speed is between two loops' (walking at 2.5 m/s, between the walk and the
jog: 19% more) and turns the hips and spine (running ahead or strafing: 5% to 6% more). Near a
loop's own speed that loop plays alone.

Where the time goes now (callgrind, `run`): half in cape springs, about 15% each in sampling clips
and in world matrices, and about 15% in cloning per frame (`perform` copies the weapon's item and
the clip's metadata, and clip names are `String`s). #215 takes on both: springs only at full
detail, and no allocations per frame.

## Next

In the plan's order: #211 turning in place (a root yaw offset ahead of the clip stack), #215
animation LOD, #214 foot IK and foot locking (after the blender, so planted feet stay planted while
it settles), #212 aim pitch and look-at, #217 lean, landings and the idle phase, #218 hit flinches
and hitstop (on `Event::Hit`, #191), and #213 traversal (after #192).

One thing to watch: combat clips take their time from `UnitCombat::action_time` in each snapshot.
Snapshots go out every tick today; if the send rate drops to 20–30 Hz (#73), attacks would play in
steps unless the client runs their clock between snapshots.
