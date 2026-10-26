# Character animation

How a character's pose is built each frame, for whoever works on it next. The clip stack is
`Character::update` in `crates/render/src/character.rs`. Everything that doesn't need the renderer
lives in `crates/anim`, which builds and tests on Linux CI (`render` only builds on macOS). The
research and the plan behind this, and the order of the remaining work, are in the
[animation plan](https://claude.ai/artifact/9MxHac5iBszDUr7wKMsoi5).

The sim is authoritative and moves bodies at 60 Hz. Clips play in place, and the client fits them
to the motion it is shown; nothing on the client moves a body.

## A frame

The client calls `Character::animate` once a frame for each character in view. When the character
is due at its level of detail (see Level of detail below), it calls `update_combat`, which records
the unit's combat view and calls `update`, which builds the pose in this order:

1. **Base clip** (`clip_for`): the action's clip if one is playing (`action_clip`), otherwise the
   pose's: idle, walk, jog or sprint by speed, jump, fall, roll, hit or death. Beasts map poses to
   their role clips.
2. **Body yaw** (`anim::Turn`, #211): the yaw the body is drawn at (`body_yaw`), which lags the
   sim's while turning on the spot, and where the upper body aims. The stride reads the travel
   against it.
3. **Sample it** into `local`, starting from the rest pose (`apply_clip` with `Bones::All`). A
   moving humanoid plays two of its walk, jog and sprint loops blended (`apply_step`), picked and
   timed by its stride (see Locomotion below). Each clip's tracks are bound to the skeleton's bones
   once (`ClipSet`, #204), and every character with the same skeleton and clips shares it through
   `ModelCache::clip_set`. A clip samples only the tracks the body uses: humanoid clips turn every
   bone but move only the pelvis, and never scale.
4. **Legs under actions** (#186): an attack or cast on the move keeps the stride on the legs
   (`walk_under`, `Bones::Lower`), and notes where the action faced the chest.
5. **Upper-body layers** (`Bones::Upper` is `spine_01` and everything under it), in this order, the
   last one applied winning: a drawn weapon's idle while moving, the guard (or the parry), and the
   ready or stow transition.
6. **Hips toward the travel** (`turn_hips`, #110): the hips turn round the vertical toward where
   the body travels, taking the legs and planted feet with them, and the spine turns the chest back,
   spread evenly over its three bones: to the front, or under an action to where the action faced
   it, and on by however far the aim is round from the body. The chest stops `MAX_TWIST` (60°)
   round from the hips, or at the clip's own twist if that is more, and the neck and head share up
   to `HEAD_TWIST` (30°) of what it can't take.
7. **Feet stepping round** (`step_round`, #211): while the body turns on the spot, the legs reach
   with two-bone IK to where the feet stand.
8. **World matrices**, then `perform`: the hand reaching for the sheath, the off hand on a
   two-handed grip, and the bow's string and arrow morphs.
9. **The blender** (`anim::Blender`): when anything switched since the last frame it starts
   carrying the old motion into the new, and while a switch settles it changes `local`, so the
   world matrices are rebuilt. It runs after `perform` so that a switch also smooths the hands
   letting go of or taking a grip.
10. **Spring bones** (capes) settle on top, under the body drawn at its yaw, at full detail only.

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

Not done yet: steps lengthened or shortened to the speed and feet pinned to the ground (#214), and
slopes. Slow walking slides most: the walk clip's trailing foot slides forward as it lifts, about 4 cm a step even at
the clip's own 1 m/s, and between the walk's speed and the jog's (backing off behind a raised
guard is 1.5 m/s) the blend can't hide it. Foot locking (#214) is the fix.

## Turning on the spot

Under action controls the sim turns a player to the camera at once, every frame, and a remote
unit's yaw moves in steps between snapshots. Drawn at the sim's yaw, a standing body would spin on
its feet. `anim::Turn` (#211) keeps the yaw the body is drawn at apart from the sim's, as Lyra's
root yaw offset does, and the client draws each character at `Character::body_yaw`:

- **The aim** follows the sim's yaw on a critically damped spring with a half-life of 0.04 s, no
  faster than 520°/s (`AIM_HALF_LIFE`, `AIM_RATE`), so a sudden turn (a target picked behind, a
  roll) swings the chest round over a few frames instead of snapping it. The upper body faces the
  aim: the spine turns the chest up to `MAX_TWIST` from the hips, and the neck and head share up to
  30° more.
- **Standing**, the body stays where it faces and the upper body takes the turn. Past 70°
  (`STEP_AT`), or held past 20° for 0.75 s (`HOLD_AT`, `HOLD`), the feet step round while the body
  catches up on a spring with a half-life of 0.15 s, no faster than 230°/s (`STEP_RATE`). Spun round
  faster than that for long, it is dragged no more than 270° behind (`MAX_LAG`), and it always turns
  the way the sim did. Within 0.3° of the aim it takes the rest at once, so the twist is gone and a
  standing body costs no more than before.
- **The feet** (`anim::Steps`, which `step_round` drives) step one at a time, the one on the side of
  the turn first, from where they stand to where the clip stands them under the yaw the body turns
  to, at most 60° ahead of the body (`STEP_REACH`), so a long turn goes round in several steps. A
  foot steps once it is 3 cm or 10° out (`STRAY`, `STRAY_YAW`), takes 0.25 s (`SWING`) and lifts 0.4
  of how far it goes, up to 6 cm. The legs reach with two-bone IK (`anim::ik::two_bone`, Holden's
  analytic solver with a pole), the knee pointing the way the clip has it and the foot turned with
  its step. Once the body has stopped, a foot more than a third of the way out takes one last short
  step, the feet ease the rest of the way at no more than 5 cm/s (`EASE`), and once both are within
  2 mm of the clip's the IK lets go.
- **Moving or acting**, the body follows the aim on a spring with a half-life of 0.08 s
  (`FOLLOW_HALF_LIFE`), so a yaw that steps between snapshots, or flicks round with a roll, comes in
  over frames with no jump in the turning speed; an attack or cast turns the body to face where it
  strikes. Beasts always follow this way, and so does anyone below full detail (#215), whose feet
  don't step round.

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

## Level of detail

A crowd costs what its characters cost, so they don't all rebuild their poses every frame (#215).
Each frame the client ranks everyone in view with `anim::Crowd` (`crates/anim/src/lod.rs`), the way
Unreal's animation budget allocator and update rate optimisation do: the player, their target and
whoever targets them first, then the nearest to the camera. The rank gives each a tier
(`anim::Tier`), and `Character::animate` rebuilds the pose at that tier's rate:

| Tier | Who | Rebuilds | Between rebuilds |
| --- | --- | --- | --- |
| `Full` | the first 8 (`FULL`) | every frame | |
| `Half` | the next 16 (`HALF`) | every other frame | the last two rebuilds, blended |
| `Quarter` | everyone else | every fourth frame | the last rebuild, held |

- **Spread over the frames.** A character rebuilds when the frame number plus its unit id is a
  multiple of its tier's period (`Tier::due`), so about as many rebuild on each frame and the cost
  stays flat.
- **Between rebuilds**, a `Half` character shows its last two rebuilds blended matrix by matrix
  (`Between`), its body yaw with them, so it moves every frame, a rebuild (33 ms) behind. A
  `Quarter` one holds its pose while the sim moves it. A rebuild steps everything by the time since
  the last one, so clips, the stride, the blender and the springs keep time.
- **Below full detail** the cape springs fade out over 0.5 s and then cost nothing
  (`SpringBones::update`); back at full detail they start again from where the bones are and fade
  in. The feet don't step round on the spot either: the body follows the aim on its spring, as it
  does moving.
- **Hysteresis.** A character keeps its tier against one up to a fifth nearer (full) or a tenth
  nearer (half), so two at about the same distance don't trade tiers every frame. Moving up from
  `Half`, the pose catches up the one rebuild it was behind; moving down to it, it starts blending
  again from the pose it shows.
- **The budget.** The client times every `animate` and gives the frame's total to
  `Crowd::spent`. While its smoothed value (10% a frame) is over `BUDGET_MS` (1 ms, a third of the
  crowd's 3 ms CPU in [PERFORMANCE.md](PERFORMANCE.md)), the full and half tiers shrink by 5% of
  their size a frame, down to one full and two half; under three quarters of the budget they grow
  back by 1% a frame.
- **No threads yet.** At the rates above, 200 characters pose in well under the budget on one
  core (see Cost), so spreading them over threads would add more sync than it saves. Building
  their skinning palettes now costs more than posing them, every frame at every tier (#243).

F3 shows the frame's posing and palette time and how many characters are at each tier.
`--crowd N` adds N characters round the player, standing and looking about, walking and running
round lanes of their own, and fighting, in five outfits, two with capes. `--anim-tiers off` poses
everyone every frame, for comparison, and every run ends with an `ANIM` line: posing and palettes a
frame (median and 95th percentile) and how many characters were at each tier on average.

## Tests

- `cargo test -p anim` (Linux and macOS): pose and velocity stay continuous through one switch
  and through three inside 100 ms (velocity changes from frame to frame shrink in step with the
  frame time, which a pop or a velocity jump can't do), a 90° offset is under 10% after three
  half-lives and under 1° after five, bones a switch leaves alone carry on untouched, and a
  settled blender shows the clips exactly. For locomotion: measuring finds synthetic feet's
  touchdowns and speed (and refuses feet that never lift), every loop plants its feet on the same
  phases, a loop plays alone near its own speed and the blend between moves steadily with the
  speed, the hips turn toward the travel and change to backward only past 100° (80° coming back),
  and changing speed and direction never restarts the stride. For turning on the spot: standing,
  the body stays put while the upper body takes a turn, however long a small one is held; past 70°,
  or held past 20°, the feet step round and the body catches up no faster than they go; a quick
  half turn swings the chest and steps round one way; moving, the body follows snapshots 20 times
  a second without jumping in yaw or turning speed; the feet step one at a time, the lead foot
  first, and end where they should. The IK reaches targets in reach with the bones keeping their
  lengths, bends the knee toward the pole, and gets the end as near as the chain goes to a target
  out of reach. For level of detail: rebuilds spread evenly over the frames at every tier, the
  involved come first and then the nearest, a character keeps its tier against one a little
  nearer, and over budget the higher tiers shrink and come back under it.
- `cargo test -p render` (macOS): `switches_carry_the_motion_on_instead_of_popping` plays jog,
  attack on the move, guard, stop, roll and run through a real character and checks that every
  bone carries on at every switch, where cuts jump by over 0.5 rad. The other clip-stack tests
  set `Method::Off`. `planted_feet_stay_put_walking_running_and_sprinting_every_way` walks, runs
  and sprints a knight ahead, to each side and backward, and fails if a planted foot slides 2 cm
  over the ground (backing off at a walk, no more than the walk clip slides on its own).
  `strafing_turns_the_hips_and_keeps_the_chest_to_the_front` checks the hips against the travel
  and the chest against the facing, beside a twin running straight with the same stride.
  `turning_on_the_spot_the_feet_stay_put_while_the_chest_takes_the_turn` turns a knight 50°: the
  feet stay within 0.1 mm of a still twin's and the chest comes round 50°.
  `past_the_limit_the_feet_step_round_and_land_where_the_clip_stands_them` turns one 115° at once:
  one foot in the air at a time, planted feet never slide 2 mm in a frame, each foot steps, and
  they end within 3 mm of where the idle stands them, back on the clip.
  `an_attack_turns_the_body_to_face_where_it_strikes` checks that a slash takes the twist out.
  `half_rate_blends_the_frames_between_a_rebuild_behind_and_quarter_rate_holds` runs three knights
  side by side at each tier: at half rate the hand moves every frame, its biggest step within a
  quarter of the full-rate knight's, and stays within 1 cm of where that knight's was two frames
  before; at quarter rate it holds between rebuilds. `a_new_character_shows_its_pose_on_its_first_frame_at_any_tier` checks that
  nobody shows the rest pose. `below_full_detail_the_cape_fades_to_its_pose_and_the_feet_dont_step`
  drops a caped knight to half rate: the cape eases to the pose, its tip moving no more than a
  tenth of the way in a frame, and turning on the spot the feet stay put. The model tests check
  that even keys are indexed and blended exactly (slerp for steps over 30°), uneven keys are
  resampled, a constant track keeps one key and step keys hold.
- The client's `--crowd` test checks that nobody in the crowd passes through anybody, even at 200,
  and that walkers and runners go as fast as their speed, the way they face.
- The client's `--gait` run (`crates/client/src/gait.rs`) is for captures: facing one way, the
  player runs ahead, strafes, backpedals, sweeps the travel round every direction, walks, sprints
  and walks; then turns round, draws a weapon and runs the pattern again; then turns on the spot
  (a small turn held, a long one, an about face and a spin) and circle-strafes once round. It logs
  the frame each leg starts on, so a capture can be cut by leg.

## Cost

`cargo run --release -p render --example anim_bench -- [characters] [frames] [case] [tier]` times
`animate` (the pose) and `draws` (the skinning palettes) for a plate knight with sword, shield and
cape, a ranger with a bow and an elf mage with a staff, at each LOD at full detail, then at half
and quarter detail, and counts what posing allocates once it is going (nothing). The cases: `idle`,
`walk` (2.5 m/s, between the walk and jog loops), `run` and `strafe` (running square to the facing)
hold their pose, `turn` stands looking 80° either way and back every two seconds (the feet step
round on every sweep), `slash` and `run + slash` start the slash over every half second, and
`start-stop` runs and stops every half second, so those three are blending most of the time.

Instructions per pose at full detail (`update_combat`, called through `animate` since #215), from
callgrind (6 characters, 240 frames, every LOD):

| Case | Before #210 | After #210 | After #204 | After #110 | After #211 | After #215 |
| --- | --- | --- | --- | --- | --- | --- |
| idle | 166,229 | 140,466 | 83,313 | 83,697 | 83,992 | 39,856 |
| turn | | | | 83,657 | 90,315 | 48,176 |
| walk | | | 88,445 | 104,860 | 105,340 | 54,396 |
| run | 210,673 | 184,868 | 88,894 | 93,491 | 93,975 | 46,861 |
| strafe | | | 88,893 | 93,947 | 95,511 | 47,971 |
| slash | | 162,570 | 106,448 | 106,852 | 107,122 | 61,480 |
| run + slash | 228,680 | 222,870 | 113,124 | 115,674 | 116,695 | 68,288 |
| start-stop | 187,279 | 185,645 | 110,360 | 112,854 | 113,244 | 67,341 |

On the M1, #210 measured 11 to 16 µs a character a frame. Holding a pose, the blender only keeps
a copy of the last two poses shown, and it took out the old crossfade, which slerped every bone
every frame even when nothing was blending. While a switch settles it runs the spring for each
bone that still has an offset and rebuilds the world matrices a second time, which the one-pass
rebuild (parents first, into the same buffer) pays for on x86; on the M1, cases that blend all the
time came out 2% to 4% dearer than before #210. #204 stopped looking up each track's bone by name
and sampling tracks the body ignores, which was a third to a half of the instructions. #110 samples
a second loop while the speed is between two loops' (walking at 2.5 m/s, between the walk and the
jog: 19% more) and turns the hips and spine (running ahead or strafing: 5% to 6% more). Near a
loop's own speed that loop plays alone. #211 costs a body that isn't turning under 1%; turning back
and forth with the feet stepping most of the time is 8% dearer (mostly the leg IK while the feet
step, then twisting the spine), and strafing 1.7%, as the neck and head now take the turn the chest
can't.

#215 took out 41% to 53% of what was left. A track's keys are evenly spaced, so sampling indexes
straight into them (`Track::sample`): tracks with uneven keys are resampled at 30 a second when the
model loads, and a track that never changes keeps one key. Clip names are interned once, and the
bones the hands, spine and head use are found once, so posing allocates nothing. Each spring joint
now builds only its own world matrix, from its parent's as it has just turned, where before every
joint rebuilt the rest of its chain.

With the tiers, wall clock on the cloud runner (`anim_bench 30 600`, µs per character per frame,
averaged over the cases): posing takes 7.7 at full detail, 3.3 at half and 1.6 at quarter, and the
skinning palettes 3.6 at every tier. So 200 characters in the default tiers (8, 16 and 176) pose in
about 0.4 ms on one core, against about 1.5 ms all at full detail.

Where the time goes now (callgrind, `run` at full detail, a third of the characters caped): about
a third in cape springs, a third in world matrices and a fifth in sampling clips. The palettes are
the next cost to take on (#243): they're rebuilt every frame at every tier, and half of them are
copies of another part's.

## Next

In the plan's order: #214 foot IK and foot locking (after the blender, so
planted feet stay planted while it settles; `anim::ik` and the way `step_round` hands feet back to
the clip are its starting point), #212 aim pitch and look-at, #217 lean, landings and the idle phase, #218 hit flinches
and hitstop (on `Event::Hit`, #191), and #213 traversal (after #192).

One thing to watch: combat clips take their time from `UnitCombat::action_time` in each snapshot.
Snapshots go out every tick today; if the send rate drops to 20–30 Hz (#73), attacks would play in
steps unless the client runs their clock between snapshots.
