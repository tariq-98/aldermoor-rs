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
   pose's: idle (each unit at its own phase), walk, jog or sprint by speed, the jump's take-off and
   loop on one timeline, roll, hit or death. Beasts map poses to their role clips. Just before it,
   `anim::Landing` notes a touchdown, and a standing body that fell far enough to hurt plays
   `Jump_Land` (#217, see Weight below). A body that flinches from a blow shows the pose it would
   have without it (`shown_pose`, #218), and while a hitstop holds, the pose clocks stand still
   (see Hits below). A move the sim carries the body through (a roll, a leap, a charge, a climb)
   plays its clip by how far the body has come, and the rest of it after the move by the clock;
   afloat, the swim loops play (#213, see Traversal below).
2. **Body yaw** (`anim::Turn`, #211): the yaw the body is drawn at (`body_yaw`), which lags the
   sim's while turning on the spot, and where the upper body aims. The stride reads the travel
   against it. Carried through a roll or a climb, the body faces the way it goes.
3. **Sample it** into `local`, starting from the rest pose (`apply_clip` with `Bones::All`). A
   moving humanoid plays two of its walk, jog and sprint loops blended (`apply_step`), picked and
   timed by its stride (see Locomotion below). Each clip's tracks are bound to the skeleton's bones
   once (`ClipSet`, #204), and every character with the same skeleton and clips shares it through
   `ModelCache::clip_set`. A clip samples only the tracks the body uses: humanoid clips turn every
   bone but move only the pelvis, and never scale.
4. **Reaching for the ground** (#217): falling, a humanoid's legs mix toward `Jump_Land`'s first
   frame by when the body will touch down.
5. **Legs under actions** (#186): an attack or cast on the move keeps the stride on the legs
   (`walk_under`, `Bones::Lower`), and notes where the action faced the chest.
6. **Upper-body layers** (`Bones::Upper` is `spine_01` and everything under it), in this order, the
   last one applied winning: a drawn weapon's idle while moving, the guard (or the parry), and the
   ready or stow transition.
7. **Hips toward the travel** (`turn_hips`, #110): the hips turn round the vertical toward where
   the body travels, taking the legs and planted feet with them, and the spine turns the chest back,
   spread evenly over its three bones: to the front, or under an action to where the action faced
   it, and on by however far the aim is round from the body. The chest stops `MAX_TWIST` (60°)
   round from the hips, or at the clip's own twist if that is more, and the neck and head share up
   to `HEAD_TWIST` (30°) of what it can't take.
8. **Feet stepping round** (`step_round`, #211): while the body turns on the spot, the legs reach
   with two-bone IK to where the feet stand.
9. **Lean** (`anim::Lean`, `lean_body`, #217): the body tips toward its acceleration, the pelvis
   about the ground beneath it and the spine the rest of the way.
10. **Gaze** (`follow_gaze`, #212): the spine, neck and head turn toward where the character
    looks, so a drawn bow points where the crosshair does and heads turn to targets (see Aim and
    look-at below). It takes the lean back out of the pitch. Before `perform`, so the hands keep
    their grips on what the arms now point.
11. **World matrices**, then `perform`: the hand reaching for the sheath, the off hand on a
    two-handed grip, and the bow's string and arrow morphs.
12. **The blender** (`anim::Blender`): when anything switched since the last frame it starts
    carrying the old motion into the new, and while a switch settles it changes `local`, so the
    world matrices are rebuilt. It runs after `perform` so that a switch also smooths the hands
    letting go of or taking a grip.
13. **Flinch** (`anim::Flinch`, `flinch_body`, #218): a blow tips the spine, neck and head away
    from it, and a heavy one the pelvis too, and the world matrices are built again (see Hits
    below). After the blender, so a switch never smooths a flinch away. Not at quarter detail.
14. **Hands on the edge** (`grip_edge`, #213): climbing, the hands take hold of the edge while
    they can reach it (see Traversal below). After the blender, which would only bring them there
    once the body had been lifted out of reach.
15. **Feet on the ground** (`place_feet`, #214): at full detail the pelvis drops (and dips on
    landing, #217) and the legs reach for the ground, and planted feet hold still (see Feet on the
    ground below). After the blender, so nothing smooths the feet off the ground, and the blender
    keeps the pose from before it. Not while the body is carried through a move, nor afloat.
16. **Spring bones** (capes) settle on top, under the body drawn at its yaw, at full detail only.

## Locomotion

The sim moves bodies at any speed in any direction, and under action controls the facing and the
travel are separate: strafing, backpedalling and running round in circles while facing one way.
The clips are three loops that run straight ahead in place (`LOOPS`: walk, jog and sprint).
`anim::locomotion` (#110) fits them to the motion, the way Paragon and Lyra do:

- **Gaits, measured once.** `Gait::measure` reads each loop's feet (the `ball_l` and `ball_r`
  bones) at 64 times through its cycle: when each foot touches down and lifts off, how high the
  ball of a planted foot is (`floors`, the median), and how fast a planted foot slides back, which
  is the speed the loop would carry the body. The first character to play a
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
  and past that stride warping (#214) shortens or lengthens the steps (`Step::warp`). Each step
  also says which feet are planted, by where the phase is in the blend's stances (`Step::planted`),
  and how high the blend holds the ball of each (`Step::floors`), for foot locking.
- **Hips toward the travel** (`locomotion::hip_turn`): turned toward the travel up to 100° round
  from the facing, and past that toward the opposite way with the loops playing backward, so
  backpedalling is the run in reverse. The change between the two is 10° either side of square
  (`HYSTERESIS`), so a strafe straight to the side doesn't flicker. The phase moves on by the
  travel's share along the hips, so while the hips are still swinging round the stride slows
  instead of the feet skating.
- **Springs.** The sim changes velocity at once. The speed and the hip turn the legs show follow it
  on critically damped springs, with half-lives of 0.1 s and 0.08 s, and snap when the character
  starts moving.

Between the walk's speed and the jog's (a sim walk is 2.5 m/s, backing off behind a raised guard
1.5 m/s) the blend can't keep planted feet still on its own, and the walk clip's trailing foot
slides forward as it lifts, about 4 cm a step even at the clip's own 1 m/s. Foot locking (below)
holds them.

## Feet on the ground

The clips are made on flat ground and play in place at the root, which the sim keeps on the ground
under the middle of the body. On a slope, a terrace or the bridge's ramps one foot would sink and
the other float, and wherever the stride can't match the body exactly (two loops blended, a turn,
past the loops' paces) planted feet slide. `anim::Feet` (#214, `crates/anim/src/feet.rs`) puts
the feet on the ground at full detail, the way Unreal's foot placement and the Game Animation
Sample's leg IK do. `Character::set_ground` gives it the ground; the client's is
`world::Valley::ground`, with the bridge deck and the tops of solids, and without one the ground is
level with the unit.

- **Each foot** finds the ground under its ankle and the slope there, measured 10 cm either side
  and taking the gentler side (`SPAN`), so a step or the foot of a wall doesn't tip it. It keeps
  the height above the ground that the clip gives it, square to the slope, and turns to the slope
  up to 30° (`MAX_TILT`), following it with a half-life of 0.04 s so it turns evenly where the
  ground bends. A jump in the ground under a foot (a step, a ledge) eases out (`EASE_HALF_LIFE`).
- **The pelvis** drops on a spring with a half-life of 0.05 s as far as the lower foot needs, and
  further, at up to 2 m/s, while a held leg would otherwise be straighter than 99.8% of its length
  (`REACH`, `REACH_RATE`). When the body steps up or down, the drop takes up the step, so the pelvis
  settles from where it was rather than dipping twice. Then two-bone IK (`anim::ik`, as for
  stepping round) bends each leg to its foot.
- **Foot locking.** A foot planted in its stride holds still in the world. The ball of the foot is
  what holds, so the heel still rises as the clip rolls over it. It holds once the clip has the
  ball within 1.2 cm of its planted height (`DOWN`; the gait's `floors`, higher running on the toes
  than walking flat footed, and each body's own), not yet as the heel strikes with the toes up. It
  lets go once the ball is 2.5 cm above that (`UP`), the stride lifts the foot, the clip carries it
  forward faster than half the body's speed as the toes push off (`LEAVE`), or the body gets a
  quarter of the leg's length away (`STRETCH`). Between `DOWN` and `UP` the sprint's planted feet
  jitter 3 cm up and down without letting go. Letting go, the foot eases from where it was held
  back onto the clip, on a spring that starts slowly so it doesn't jump after the clip.
- **Stride warping.** Past the loops' paces the steps shorten or lengthen along the line the legs
  stride along (`Step::warp`, between half and 1.6 times the clips'), so the feet keep up with the
  body instead of sliding.
- **Off the ground** (jumping, falling, rolling, on a wall (#247), downed or dead, below full
  detail, or 20 cm or more above the ground: in the air or afloat, `AIRBORNE`) the placement fades
  out over 0.15 s, eased in and out, and fades back in on landing (`FADE`). Moved 1.5 m in a frame
  (a teleport, `JUMP`), the feet let go at once.

Over the frames feet are held, the clips alone would slide them 18 to 94 cm (four seconds of
walking, jogging, sprinting or backpedalling on each of the five bodies); held, they move under
3 mm in all. `--foot-ik off` leaves the feet as the clips have them, to compare.

Not done yet: the walk's trailing foot still slides as its toes push off, since the hold lets go
as the clip carries the foot on (held longer, the foot is left behind and races after the clip;
pinning the toes needs the toe joint to hold instead of the ball). A goblin sprinting at a human's
9 m/s overstretches its legs, and its held feet drag 6.5 cm.

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

## Aim and look-at

Third-person games put two procedural layers on top of their clips: an aim offset, which pitches
the spine with the aim so the arms and what they hold point where the crosshair does, and a
look-at, which turns the head toward what matters. `anim::Gaze` (#212) does both for the five
bones from `spine_01` to `Head`, and `Character::look_at` tells it where to look each frame, in
the world:

- **What it looks at** is the client's choice. You look where the crosshair points while aiming
  (the ground under it, or 100 m along it into the sky, `FAR_GAZE`). Anyone with a target looks at
  the target's head (`EYES`, 93% of its height) while it's within 40 m (`GAZE_RANGE`), and an NPC
  with nothing targeted looks at the nearest player within 5 m (`NPC_GAZE_RANGE`). Anyone else
  looks ahead: other players' aim isn't in the snapshot yet (#244). A point within 0.5 m of the
  head (`NEAREST_GAZE`) isn't looked at, and the showcase looks at nothing.
- **Aiming or looking.** With a weapon drawn it aims: the spine takes all the pitch (30%, 30% and
  40% from the bottom, `AIM_PITCH`), so the bow, wand or staff points along the aim, up to 50° up
  and 45° down (`AIM_UP`, `AIM_DOWN`: a bow up a cliff, or down from the bridge), following on a
  critically damped spring with a half-life of 0.05 s (`AIM_HALF_LIFE`). Otherwise it only looks:
  the neck and head take 85% of the pitch (`LOOK_PITCH`), up to 40° either way (`LOOK_LIMIT`), at a
  person's pace (0.15 s, `LOOK_HALF_LIFE`). Drawing or stowing moves between the two on a 0.1 s
  spring (`FADE_HALF_LIFE`).
- **Round** from where the upper body faces (the sim's yaw, closely followed, after #211's turn),
  the gaze turns up to 70° (`MAX_YAW`): a fifth of it in the chest, the rest in the neck and head
  (`LOOK_YAW`). A point more than 110° round (`LET_GO`) is let go of, and the gaze comes back
  ahead, rather than swinging from one shoulder to the other as the point passes behind. Under
  action controls the sim turns you to the camera, so while you aim there is little to turn.
- **Each bone** turns round the vertical by its share, then pitches about the line across the way
  it now faces, so the head ends up turned and pitched by the whole of it. The gaze goes on before
  `perform`, so the hands keep their grips, and before the blender, so a switch smooths it with the
  rest of the pose.
- **A swing owns the head.** While an action's clip plays (an attack, a cast, a roll, a flinch,
  lying downed and getting back up, but not a shot, a bolt or a channel, which are aimed) and at
  quarter detail, the gaze fades out on a 0.1 s spring, and back in after. A shot keeps its aim,
  so the arrow leaves the bow where the crosshair points.
- **Settled, it's free.** A spring that has all but got where it's going, and all but stopped,
  stops there and isn't run again until it has somewhere new to go, so a character looking ahead
  turns no bones, and one faded right out (at quarter detail, say) skips the gaze altogether. Fading
  back in, it starts turned the way it now looks, rather than sweeping across from where it looked
  before.

## Weight

The clips play upright and the same for everyone, and the sim changes a body's speed at once. On
their own, a run sets off, stops and corners as if the body weighed nothing, a jump plays all of
`Jump_Start`'s crouch after the body has left the ground, landings go straight back to running,
and a crowd spawned together breathes in step. #217 adds the small procedural layers that give a
body its weight (Overgrowth's approach, and Fortnite Chapter 5's procedural layering), in
`anim::Lean`, `anim::Landing` and `anim::idle`:

- **Lean.** `Lean::update` follows the body's velocity over the ground on a critically damped
  spring with a half-life of 0.1 s (`VELOCITY_HALF_LIFE`), and the spring's rate of change is the
  body's acceleration, smoothed. The body tips toward it by about a third (`SHARE`) of the angle
  that would balance it (`atan(a / g)`, as a runner leans into a bend), up to 8° forward or back
  and 10° to the side (`MAX_PITCH`, `MAX_BANK`). Below a jog (`FULL_SPEED`, 5 m/s) the caps shrink
  in step with the speed, so a walker barely leans. The tilt follows on a spring of its own (0.07
  s, `TILT_HALF_LIFE`), so it builds and goes without a jump whether the sim changes the speed at
  once, as now, or ramps it (#195). Setting off at a run, the body leans the full 8° for about a
  third of a second; running round at 6 m/s and 70° a second, it banks the full 10°.
- **Where the lean goes.** The pelvis takes 60% of it (`LEAN_BODY`), tipping the whole body about
  the point on the ground beneath it so the feet stay about where they were, and the spine bends
  the rest of the way, a third at each bone. The gaze, after it, takes the lean back out of its
  pitch, so an aimed bow stays on the crosshair. In the air, on a wall (#247), rolling, dead,
  downed or getting back up, and at quarter detail, the body comes upright on the tilt's spring. At
  a steady pace it leans no way at all, and costs nothing. `--lean off` keeps everyone upright, to
  compare.
- **Jumps.** The sim's jump leaves the ground at once at 6.6 m/s and tops out 1.09 m up a third
  of a second later, while `Jump_Start` crouches for its first frames and pushes off. One timeline
  runs through the air: it starts 0.07 s into `Jump_Start` (`TAKEOFF`), the legs already straight,
  plays at whatever pace brings the clip's tuck (0.3 s in, `TUCK`) to the top of the jump, and runs
  on into `Jump_Loop` where `Jump_Start` ends. `Pose::Jump` and `Pose::Fall` share it, so the
  change from rising to falling changes nothing, and falling off an edge starts in the loop. On a
  wall (#247) the climbing clips play instead: climbing up from a hang, which the sim also poses
  as a jump, isn't in the air.
- **Reaching for the ground.** Falling, a humanoid's legs reach for the ground as it nears:
  `Jump_Land`'s first frame (legs out, the feet just meeting the ground) mixes in over the last 0.2
  s before touching down (`REACH`), eased in and out. It's timed by when the body will land under
  the sim's gravity (`anim::land::time_to_land`, from the height over the ground and how fast it
  falls), not by the clip, the way Paragon's distance-matched landings are, so a hop and a long
  fall both meet the ground with the legs ready. The reach changes no faster than 10 a second
  (`REACH_RATE`, more than any fall needs), so passing over the edge of a ledge it lands on doesn't
  snap the legs down. Not while an action plays.
- **Landing.** Touching down, `Landing::update` says how fast the body fell and how far, and the
  pelvis dips 1 cm for every m/s (`DIP`), up to 15 cm (`MAX_DIP`): 6.6 cm from a jump on the flat.
  The dip is a critically damped spring kicked downward (0.1 s, `DIP_HALF_LIFE`), so it bottoms out
  after about 0.07 s and is back after about 0.4 s, without bouncing. It goes on the pelvis's drop
  in the feet step, so the knees bend and the feet stay on the ground, at full detail only.
  Catching a hold on a wall (#247) isn't touching down.
- **A fall that hurts** (past the sim's safe height, 4 m, `SAFE_FALL`) onto a body standing still
  plays `Jump_Land` in full, a deep squat and back up, and ends as soon as the body moves. On the
  move, the stride goes on and the dip goes the full 15 cm: the landing clip plays in place, so the
  feet would slide.
- **Idle phases.** Each unit's idle starts at its own phase and plays at its own pace
  (`anim::idle::time`). The phase steps by the golden ratio's fraction from one id to the next, and
  ids handed out together are consecutive, so neighbours start at least 0.38 of a loop apart and
  any five at least a seventh. The pace is up to 5% faster or slower (`SPREAD`), so none stay in
  step. The drawn idle layered over a moving upper body is offset the same way.

## Hits

Every blow used to play `Hit_Chest` on the whole body for the quarter second the sim holds
`Pose::Hit`, whichever way it came from and however hard, with the legs stopped: a runner hit from
behind skidded to a standstill mid-stride. #218 makes hits read without wrecking the movement. For
every blow #191's `Event::Hit` says who struck whom, the way it travelled, how hard it was
(`HitWeight`) and how long both bodies' poses may hold: `hitstop_ms`, from
`content::combat::HITSTOP`, 40 ms for a light blow up to 120 ms for a big one, half for missiles.
The client hands it to the victim's `Character::hit` and the attacker's `Character::landed`.

- **The flinch.** `anim::Flinch` kicks a critically damped spring on the chest's tilt (half-life
  0.085 s, `HALF_LIFE`) with the speed that takes it out as far as the blow was hard and back: it
  peaks after about 0.06 s and is back within 0.35 s, without bouncing. How far goes with the
  weight: 0.18 rad for a light blow, 0.28 heavy, 0.32 critical and 0.4 big (`LIGHT` to `BIG`), and
  0.08 or 0.04 for one a guard stopped or a parry turned. Blows landing together add up to no more
  than 0.55 (`MOST`). A heavy, critical or big blow kicks the pelvis too, by 0.3 of it (`PELVIS`).
  The head follows chest and pelvis on a quicker spring of its own (0.06 s), so it lags behind them
  and then whips past. The spine takes the chest's tilt 25%, 35% and 40% up its three bones
  (`SPINE`), and the neck and head share the head's lag. A light blow tips the head about 5 cm,
  and a heavy one from the front takes the hips back 6.5 cm.
- **Under it, the body carries on.** The flinch goes on after the blender, each bone tipping about
  its own axis (`tip`, which the lean shares), the pelvis about the ground beneath it so the feet
  stay put, and the feet go on the ground after it. A blow on a body standing, walking or running
  shows it standing, walking or running by the sim's speeds (`shown_pose`), so the stride carries
  on, and attacks and casts play through. Carried through a move or hanging on a wall (#247), it
  keeps the sim's pose for that, so a roll or the hang plays on under the flinch. A stagger
  (`Hit_Knockback`, or `Idle_Shield_Break` from a guard), a knockback, a blow that catches the body
  in the air (`Hit_Chest`) and a body that can't flinch (beasts, with their own hit clip) still play
  their clips. At quarter detail the springs run but the body doesn't tip.
- **The hitstop.** As a blow lands, both bodies' pose clocks hold for the hitstop: while it holds,
  `Flinch::update` hands back no time to move them on, and the spine shakes along the blow at 17 Hz
  by a fifth of the flinch (`SHAKE`), fading as the hold runs out. Then the clips play on from
  where they held, at their own pace. Those the sim's clocks time (`pose_time`, `action_time`) run
  behind them by however long they were held (`Flinch::behind`), until the next switch lets them
  catch up, so nothing jumps either way. The sim never pauses, so whatever follows the body through
  the world keeps going: the stride over the ground it covers, the turn, the lean, the feet, the
  capes and the blender.
- **To compare and capture.** `--flinch off` plays the hit clip in full on every blow, without the
  hitstop. `--hit-churn` stages blows every 0.75 s: in the showcase the first character swings and
  lands on the rest, from the front, behind and either side and light, heavy and big in turn; with
  `--gait`, the running player takes them.

## Traversal

The sim carries a body through a roll, a leap, a charge, a climb, a mantle, a vault or a knockback
along a set path (#192's `Traversal::Forced`: where it starts and ends, and when), at a pace of its
own: a roll starts fast and slows to a stop, and a climb rises up the face before it goes over the
top. By the clock the clips didn't fit the moves. All of `Roll` (1.47 s, standing up included) was
squeezed into the 0.45 s roll, so the body stood up while the sim still slid it the last 0.83 m; a
climb played the jump, legs tucked, while the body was lifted up the wall; the sword's lunge played
a standing thrust as the body glided 8 m; and swimmers ran through the river. #213
(`anim::traverse`) fits the clips to the moves, the way Paragon and Lyra match their starts, stops
and landings to distance:

- **By distance.** Each move's clip marks the stretch of it that covers the path with a `travel`
  curve (docs/ASSETS.md): the share of the path covered by each moment. `traverse::share` says how
  much of its path the body has covered (along the ground, or for a climb the height gained and
  the way across together), and `traverse::clip_time` finds the moment of the clip that covers as
  much. So a roll's hands touch down as the body passes halfway and its feet plant as the move
  ends, and a lunge reaches full stretch as the body arrives, at whatever pace and over whatever
  length the sim moves it. `anim::Progress` only lets the clip play forward through a move,
  whatever the snapshots do, and starts afresh with the next; a path under 5 cm (`SHORTEST`) goes
  by the clock.
- **After the move**, the rest of the clip plays on by the clock (`Carried`): standing up out of a
  roll, a dash's blow and recovery. Standing, it plays on the whole body; running on, on the upper
  body over the stride. It ends with the clip, or on a new action, a fall that hurts or the water.
  While the pose of the action that began a leap or a charge holds, its clip plays on in place of
  the action's own.
- **The clips.** A roll plays `Roll`: the dive at 0.1 s, the hands down at 0.43 and the feet at
  0.87, then 0.6 s standing up. A climb or a mantle 0.4 m or higher (`STEP_UP`) plays `ClimbUp_1m`
  over the whole move; a lower one hops up with the jump. A charge plays the main item's `dash`
  (`Sword_Dash`), whatever the blow: the sim lands the blow on arrival, and the clip's lunge ends
  there (its `hit` marker comes 0.07 s later). A leap ahead plays the sword's or shield's `dash`,
  and a hop back `NinjaJump_Start`, springing off without lunging. A vault keeps the jump, turned
  toward the hop, and a knockback plays its hit by the clock, as before. The sim's run or jump pose
  after a leap or a charge, with the action still in the combat view, is just a run or a jump.
- **Facing the move.** Strafing, a roll goes sideways or back while the sim's yaw stays on the
  camera, and `Roll` rolls straight ahead: played under the yaw, it rolled sideways like a log.
  Carried through a roll, a climb, a mantle or a vault, the body turns to face the way it goes on a
  0.025 s spring (`Stance::Carried`, `CARRIED_HALF_LIFE`), 85% of the way round within 0.1 s, the
  upper body with it. Out of a roll it keeps facing that way until it has stood up, then turns back
  on the moving spring.
- **Hands on the edge.** Climbing, the hands take hold of the top where the climb goes over it
  (`traverse::edge`: the first point on the way across where the ground rises to within 0.1 m of
  the top), 25 cm either side of the climb's line (`HAND_SPREAD`), palms down and fingers over the
  edge, by two-bone IK. They take hold over 0.05 s (`TAKE_HOLD`), the more firmly the higher the
  climb (not at all at 0.4 m, fully from 0.7 m, `PULL_UP`), and let go over the last 0.1 m before
  the body is over the edge (`LET_GO`). The sim lifts a climber faster than it carries them across
  (the height comes in over the first 60% of the climb, easing out, while the way across eases in),
  so the shoulders rise out of reach of the edge within 3 to 5 frames. Each hand holds while its
  place is within 1 to 1.3 arm's lengths of the shoulder (`REACH`), then eases back into the clip,
  its hold halving every 0.05 s (`RELEASE_HALF_LIFE`). So for now the hands grab the edge and let
  go as the body rises: to hang on through the pull-up, the sim's climb has to rise more slowly or
  the mantle clips have to match its timing (#108).
- **On a wall** (#247), the climbing clips play instead (docs/CONTRACT.md, "Poses to animation"):
  the hang, shimmying whichever way the body slides along the wall, and the moves onto, between
  and off holds, climbing up from a hang included. The body isn't carried: no turn on the carried
  spring (the sim turns it to the wall) and no hands put on the edge. A climb from the ground or a
  mantle still plays `ClimbUp_1m` (`pulling_up`).
- **Climbing by distance** (#248, `anim::climb`). Each move's clip is made on the sim's own path
  for it (`sim::climbing::carry`), `k` of the clip `k` of the way along, so it plays at the share of
  its move the body has come (`climb::progress`): the nearest point of the path, seen from above
  for a move mostly across, so an eject's hop doesn't count. Where the path stands still, the clock
  carries it on: climbing straight up to the next hold, the body arrives with five eighths of the
  move done and the hands finish it. A wrap round a corner swings round it instead
  (`sim::climbing::corner_carry`), so it plays by the clock, its clip made on that swing (#305).
  The shimmy loops slide 0.48 m a loop, made at the sim's 1.2 m/s (`SHIMMY_STRIDE`), and play by
  the distance slid, so the hands keep still on the hold at any speed.
- **Hands on the hold, feet on the wall** (#248). The clips are made on the broad human frame, which
  hangs with its wrists 7 cm out from the wall and 3 cm below the hold (`GRIP_OUT`, `GRIP_DOWN`).
  Each skeleton measures its own hang once (`Hang_Idle_Loop`'s first frame, `ClipSet::reach`) and
  moves by the difference, up and in toward the wall: as the clips have them, dwarves and goblins
  hang 35 to 46 cm short of the hold and 5 to 6 cm back from it, and elves 8 to 12 cm past it. A
  hold less than 1.9 m above the feet, where the sim stands a climber on the ground under it (over
  half the valley's hold length), lowers the body by as much: the clips brace the feet half a metre
  up the wall, so they stay on it. All of it comes in as the body lowers itself or catches the hold,
  goes as it climbs onto the top, and eases in and out with a 0.05 s half-life (`RAISE_HALF_LIFE`).
  Then, at full and half detail, two-bone IK closes each hand on the hold nearest it, where the clip
  has it along the hold: firmly with the clip's wrist within 10 cm of there and not at all past
  25 cm (`HOLD`, `LET_GO`), lifting off as the clip lifts it. It braces the balls of the feet 1 cm
  out from the face below (`BALL_OUT`; the nearer face, round a corner), not up at the edge nor
  under an overhang. The holds are the client's own copy of `world`'s: the one the body hangs from,
  or the ones its move goes from and to, found from where the sim has it, among the holds filed
  once by the squares of ground they can be hung from over, so looking neither allocates nor hashes
  (`crates/client/src/holds.rs`, `Character::set_holds`).
- **Leaps, ejects, corners, wall runs and roll landings** (#305) have clips of their own, made on
  the sim's numbers as the others are. From a hang, a move onto a hold that takes longer than 0.45 s
  (`climb::LONG_MOVE`) is a climb leap, mostly up (`Hang_Leap_Up`: a pull on the hold and a push off
  the wall to a hold 1.4 m up), or a side eject (`Hang_Eject_L` / `_R`: a push off along the wall,
  flying 2 m with a 0.35 m hop, the leading hand reaching for the hold), not a climb or a hop. Round
  a corner, a body carried on more than 0.1 m ahead (`CORNER_AHEAD`) wraps round an outer one onto
  the wall's side (`Hang_Corner_Out_L` / `_R`), and otherwise turns into an inner one where it hangs
  (`Hang_Corner_In_L` / `_R`): the sim turns it a quarter at 4 rad/s, the clips are made turning
  with it, and the body follows on the carried spring. A wall run plays `Wall_Run` (three steps up
  the face, slowing, the hands reaching for the top) by how high the body has run
  (`climb::run_progress`), its hands and feet as the clip has them: there's no hold. Kicked off a
  hang or a wall run backwards, rising and thrown out from the wall faster than 2.5 m/s (`KICK`),
  `Hang_Eject_Back` (the hands letting go, the feet pushing off, the body opening out into the
  falling loop) plays by the clock as the body flies, for its 0.5 s. The sim's step off the wall
  doesn't move the body yet, so it plays from there until the body moves across, for up to 0.1 s
  (`KICK_SEEN`), and on only if the body is thrown out: a side eject into the air and letting go
  don't kick. A roll that starts in the air, the roll landing, plays `Roll_Land`: it touches down as
  `Jump_Land` does, folds over the knees with the hands going down and rolls over the shoulder as
  the vendor roll does, faster, by the 3 m it covers, its feet under it as the move ends; then it
  stands as the roll does.
- **Afloat**, the swim loops play: `Swim_Idle_Loop` treading water, and `Swim_Fwd_Loop` on the
  move, its strokes paced by the speed (made for 2.7 m/s, the sim's swim at a run, `SWIM_SPEED`;
  between half and 1.5 times their pace). The clips have the surface at their origin and the sim
  floats a swimmer with its feet 0.5 m under it, so the body is raised by that much (`SWIM_FLOAT`).
  Wading, the sim slows the body and the stride follows the ground it covers (#110), so the steps
  slow with it.
- **Carried or afloat**, actions play on the whole body, the body doesn't lean, the feet aren't put
  on the ground and no drawn idle goes over the strokes. Water breaks a fall: landing in it never
  plays the hard landing. Beasts play their clips as before.
- **To compare and capture.** `--traversal off` plays the moves by the clock and swimmers as
  runners, as before, and climbers' clips by the clock with their hands and feet as the clips have
  them. `--traverse` runs a course for captures (`crates/client/src/course.rs`),
  facing west as `--gait` does: the player strafes and rolls left, right, back and ahead in turn,
  running on out of two rolls and standing up out of the other two; climbs onto the field wall west
  of the north road and drops off its far side; runs through the village and swims across the river
  east of the bridge and back; climbs the wall from the other side; at the ruined hall, climbs onto
  its rear wall, lowers itself over the inner edge to hang (a low hold: the feet stand on the
  ground), shimmies round both corners of the wall's end onto its outer face and lets go; sprints
  at the hall's stepped east wall, runs up it and pulls onto the lookout; lowers itself over the
  lookout's edge, shimmies left, ejects right along the hold, shimmies back and climbs up; steps
  down, drops to hang from the step below and leaps up to the lookout's hold, then kicks off it
  backwards, turning in the air, and rolls as it lands (#247's clips, #248's timing and hands and
  feet, #305's corners, wall run, eject, leap, kick and roll landing); and lunges at the nearest
  wolf in the meadow. It logs each step as it starts, so captures can be cut by step. The valley's
  holds have no inner corner yet, so `Hang_Corner_In_L` / `_R` can't be seen on it.

## Switches

A switch is any change in what plays: the base clip's name, the legs' clip, the pose, the upper
layer, an action starting over (`update_combat` sees a new action, even with the same clip), a
LOD swap (`set_lod`) or a change of gear (`set_look`). Each is given a kind (`switch_kind`), and
each kind has its half-life in one table, `anim::Switch::half_life`:

| Kind | Half-life | When |
| --- | --- | --- |
| `Locomotion` | 0.09 s | idle, walk, jog and sprint changing, and the legs under an action |
| `Action` | 0.05 s | an attack, cast, roll, jump or hit starting, or starting over |
| `Recover` | 0.10 s | back to moving or standing as an action ends |
| `Layer` | 0.07 s | the guard, ready, stow or a drawn idle coming or going |
| `Land` | 0.06 s | touching down from a jump or fall |
| `Detail` | 0.05 s | a LOD swap |
| `Gear` | 0.07 s | new gear or clothes going on (see Changing gear) |

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

## Changing gear

Someone in view who changes weapon, armour or cape puts the new look on in place
(`Character::set_look`, #239). Building them again, as the client used to, started everything
over: the clips and the stride from their first frame, the blender with no old motion to carry on,
so the body cut to the new pose, and the cape back at its rest. Now:

- **Bones carry over by name.** Gear can bring bones of its own (a cape has 28) or take them away,
  so the new skeleton's bones are matched to the old ones by name. A matched bone keeps its pose,
  and the blender keeps its motion and any offset still settling (`Blender::remap`). A new bone
  starts from its rest, and so does one of the same name that rests somewhere else now (another
  cape on bones named alike).
- **The change is a switch** (`Gear`), so the new weapon's clips and grips blend in: the hands
  take a greatsword's two-handed hold over a few frames.
- **Capes swing on.** Each spring joint on a bone of the same name takes up the old joint's tail
  and speed (`SpringBones::carry_on`), so a cape kept through the change swings on, and another
  cape on bones named alike takes up its swing. A cape just put on starts from the pose.
- **The rest carries on untouched**: the stride, the feet, the turn, the gaze, the lean and the
  tier. The other levels of detail kept for swapping back held the old look, so they go.
- **Another body** (another art id) has nothing to carry on, so it is built again, keeping the
  client's settings.

The client puts on any new look this way, drawing or sheathing a weapon aside, which isn't a change
of look. `--showcase --look-churn` swaps the first character between sword and shield and a
greatsword every second, to compare in captures.

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
  settled blender shows the clips exactly. Following a change of skeleton, the old bones carry on
  through the switch under their new places and a new one starts as posed, where without the remap
  the arm pops. For locomotion: measuring finds synthetic feet's
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
  nearer, and over budget the higher tiers shrink and come back under it. For the feet on the
  ground, with a walking leg on synthetic ground: on a 30° slope the soles meet the ground through
  each contact (within 1 cm) going up, down, across and slantwise; a held foot moves under 2 mm
  through its contact going faster or slower than the clip and turning, on level ground, a gentle
  slope and a 30° one, and with the clip's planted feet jittering 2 cm, and it holds through most
  of each contact; the pelvis drop never jumps up a slope, over its top, down a 30 cm ledge,
  standing, in the air and landing; slower than the clips play, the steps shorten so planted feet
  stay put unheld; off the ground the placement fades out and comes back on landing; and a
  teleport lets go of the feet at once. Measuring a gait finds where each foot lifts off and how
  high its planted ball is, and each step says which feet are planted and how much to warp. For
  aim and look-at: the gaze turns no further than its limits, even on the way, and gets to them; it
  splits the pitch over the chain as specified (aiming, the spine takes all of it; looking, the
  neck and head 85%), and a look round turns the chest a fifth; it fades out smoothly through a
  swing and comes back after; faded right out it stops, and fades back in turned the way it now
  looks; and as the point jumps about, passes behind and goes, the head never turns 25° in a frame,
  and behind, it looks ahead. For weight: the body leans forward speeding up, back stopping and
  into turns either way, whichever way it faces, never past the caps but up to them, and the caps
  shrink with the speed (setting off at a walk, it barely leans); a speed that changes at once and
  one ramped over a quarter second both lean without a jump and settle exactly upright; a body
  first seen running doesn't lean, and one held upright comes back without a jump. Touching down
  says how fast and how far the body fell (jumps from 0, 2 and 5 m up); the dip goes with the fall
  speed up to its most, comes back without bouncing and settles; and the legs reach for the ground
  only falling, from 0.2 s out to all the way on touching down, and without a jump when the body
  passes over the edge of a ledge it lands on. Units with ids in a row start their idles well
  apart, neighbours don't come in step over four loops, and the pace stays within 5%. For hits: a
  blow tips the body its way, as far as it was hard, the pelvis only for a heavy one; a flinch
  comes back within four half-lives, never moving faster than the kick; the head lags the chest,
  then whips past it; the hitstop holds the pose clock and lets go without a jump; and blows
  landing together tip no further than the most. For traversal: a roll's, a leap's, a charge's
  and a low and a high climb's clips play only forward, whatever pace the sim moves the body at,
  and reach the end of their stretch as the move ends; halfway along a roll the clip is at its
  hands touching down, and it plays fastest at the start, as the body moves; a climb counts the
  height gained as much as the way across; a path too short to measure goes by the clock; a new
  move starts afresh, and a body jumping back between snapshots doesn't wind the clip back; curves
  hold outside their keys and skip flat stretches; and headings are the sim's yaw. The hands find a
  wall's edge from the ground (without one, 0.3 m short of where the climber ends up), take hold
  over 0.05 s, let go coming over it and keep off a step up; they hold within reach and ease off by
  halves out of it, letting go entirely under a hundredth. Carried into a roll the body faces it
  within 0.1 s, never turning back on the way, and turns back to the facing after on the moving
  springs.
- `cargo test -p render` (macOS): `switches_carry_the_motion_on_instead_of_popping` plays jog,
  attack on the move, guard, stop, roll and run through a real character and checks that every
  bone carries on at every switch, where cuts jump by over 0.5 rad (with the feet left as the
  clips have them: they go on the ground after the blender). The other clip-stack tests set
  `Method::Off`. `planted_feet_stay_put_walking_running_and_sprinting_every_way` walks, runs and
  sprints a knight ahead, to each side and backward without foot locking, and fails if a planted
  foot slides 2 cm over the ground (backing off at a walk, no more than the walk clip slides on its
  own). `held_feet_stay_put_through_each_contact_every_way` does the same with foot locking: each
  foot holds in every stride, for at least 4 frames each time, and moves under 5 mm held.
  `on_a_wall_the_feet_leave_the_ground_even_taking_a_blow` hangs a knight 10 cm off the ground
  (#247), struck: the feet don't reach for the ground.
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
  before; at quarter rate it holds between rebuilds.
  `a_new_character_shows_its_pose_on_its_first_frame_at_any_tier` checks that nobody shows the rest
  pose. `gear_changes_carry_the_motion_on_instead_of_popping` jogs a man through four changes of
  gear and cape (a greatsword for sword and shield, another cape on the same bones, the cape off and
  back on) beside a twin who keeps the first look and one built again at each change: the body and
  the stride carry on at every change, where building again pops by over 0.5 rad; a cape kept
  through a change hangs within 0.02 rad of the twin's, where built again it is over 0.1 rad off;
  and another cape takes up the swing.
  `a_new_look_survives_detail_swaps_and_a_new_body_starts_afresh` checks that a cape taken off
  doesn't come back from a level of detail kept for swapping, and that another body is built
  afresh with the settings kept.
  `below_full_detail_the_cape_fades_to_its_pose_and_the_feet_dont_step`
  drops a caped knight to half rate: the cape eases to the pose, its tip moving no more than a
  tenth of the way in a frame, and turning on the spot the feet stay put.
  `a_drawn_bow_pitches_the_spine_with_the_aim_and_a_look_mostly_the_head` has an archer look 30° up
  and down: the head pitches within 1.5° of it, and the chest by the spine's share (all of it with
  the bow drawn, so the bow hand follows within 3°, and 15% with it on the back).
  `heads_turn_toward_a_point_within_limits_and_let_go_behind` turns the head 60° either way, stops
  it at 70° for a point 100° round and looks ahead for one 150° round, the chest taking a fifth.
  `a_swing_takes_the_head_back_and_a_shot_keeps_its_aim` has a swordsman looking 45° round and 20°
  up slash: the gaze fades below 5% over the swing, never moving the head 5° in a frame, while an
  archer loosing a shot stays aimed within 3°.
  `a_jump_takes_off_straight_tucks_at_the_top_and_lands_with_a_dip` jumps a knight on flat ground:
  the pelvis leaves the ground over 0.8 m up, not crouched, the feet tuck highest within 3 frames of
  the top of the jump and are within 5 cm of the ground touching down, and the pelvis dips 4.5 to
  8.5 cm with the feet never sinking 1 cm.
  `a_fall_that_hurts_lands_in_full_standing_and_dips_on_the_run` drops one 6 m beside a twin that
  doesn't fall: standing, it plays `Jump_Land`, the pelvis going over 25 cm below the twin's;
  running, the stride goes on, the pelvis dipping within 3 cm of `MAX_DIP` and back within 2 cm of
  the twin's after 0.75 s. `runners_lean_forward_setting_off_back_stopping_and_into_turns` runs a
  knight beside one that doesn't lean: the head goes forward setting off, back stopping and to the
  inside running round, within the caps. `on_a_wall_the_body_neither_lands_nor_leans_nor_jumps`
  drops one onto a hold (#247), shimmies it along the wall and climbs it up: no dip, no lean and no
  timeline in the air. `neighbours_spawned_together_idle_out_of_step` checks that knights with ids
  in a row stand differently, and the same knight twice the same.
  `blows_tip_the_body_away_while_the_legs_jog_on` jogs a man taking a light blow from each side
  beside a twin who takes none: the legs and the stride stay the twin's, the head goes out 2 to 12
  cm the way the blow went and is back within 5 mm by 50 frames, a heavy blow from the front takes
  the hips back, and a big one with a hitstop doesn't hold the stride.
  `a_blow_on_a_wall_or_mid_roll_flinches_over_what_the_body_was_doing` strikes one hanging on a
  wall (#247) and one rolling: the hang and the roll play on, and the head still goes with the blow.
  `staggers_knockbacks_and_blows_in_the_air_still_play_their_clips` checks the clips that stay,
  that a wolf only holds, and that with flinching off a blow plays `Hit_Chest` and holds nothing.
  `the_hitstop_holds_a_swing_then_lets_it_go_on_without_a_jump` lands a slash with an 80 ms
  hitstop beside a twin's with none: the sword arm holds still through it, swings on no faster than
  the twin's, and once the swing ends both stand alike.
  `a_strafing_roll_turns_into_the_roll_plays_by_distance_and_turns_back_once_up` rolls a knight
  5 m to the side: the body faces the roll within 3° from the eighth frame, the clip plays only
  forward, at the hands touching down halfway and the feet planting as the move ends, then stands
  up by the clock, still facing the roll, and turns back to the sim's yaw.
  `running_out_of_a_roll_the_legs_run_and_the_roll_recovers_over_them` checks that running on out
  of a roll the legs jog, and the roll's recovery plays on the upper body for over half a second.
  `a_charge_plays_the_dash_by_distance_and_its_blow_on_arrival` lunges a swordsman 8 m with a
  thrust, as the sim's sword lunge does: `Sword_Dash` over the whole body, at full stretch arriving,
  playing on by the clock while the Attack pose holds, until the next blow. Running on after the
  greatsword's rush is a run, and with traversal off the dash plays by the clock over sprinting
  legs, as before. `climbs_and_mantles_play_the_climb_with_the_hands_on_the_edge` climbs a 1.2 m
  wall: `ClimbUp_1m` by distance, never the jump, the edge found within 5 cm of the wall's top, the
  nearer hand onto its place on it and the other within 5 cm, both let go of by the top; a 0.3 m
  step hops up with the hands off it, and a mantle onto 2.2 m caught in a jump climbs.
  `swimmers_play_the_swim_loops_at_the_surface` checks treading water and stroking, the body raised
  to the surface within 1 cm, and the run with traversal off.
  `a_leap_ahead_lunges_and_a_hop_back_springs_off_by_distance` leaps 6 m ahead behind a shield and
  hops 4 m back: `Shield_Dash` and `NinjaJump_Start`, each ending its stretch as the move ends,
  within a tenth. The model tests
  check that even keys are indexed and blended exactly (slerp for steps over 30°), uneven keys are
  resampled, a constant track keeps one key and step keys hold.
- The client's `--crowd` test checks that nobody in the crowd passes through anybody, even at 200,
  and that walkers and runners go as fast as their speed, the way they face.
- The client's `--gait` run (`crates/client/src/gait.rs`) is for captures: facing one way, the
  player runs ahead, strafes, backpedals, sweeps the travel round every direction, walks, sprints
  and walks; then turns round, draws a weapon and runs the pattern again; then turns on the spot
  (a small turn held, a long one, an about face and a spin) and circle-strafes once round; last it
  jumps standing, sets off into a running jump and stops, and runs round a circle the way it faces
  (#217). It logs the frame each leg starts on, so a capture can be cut by leg.
- The client's `--traverse` run (`crates/client/src/course.rs`, see Traversal) is for captures
  too. Its test runs the course against the valley as the client's own server starts it: every
  step is reached and every place arrived at, and on the way the player rolls four times, climbs
  the field wall twice, the ruin's east and rear walls and back up from a hang, swims, hangs from
  the rear wall and goes round both its corners, runs up the wall to the lookout and pulls onto it,
  shimmies and ejects along the lookout's hold, leaps up to it from the step below, kicks off it
  to land over 2 m out and rolls the landing on 3 m, comes down unhurt, and lunges.

## Cost

`cargo run --release -p render --example anim_bench -- [characters] [frames] [case] [tier] [feet]`
times `animate` (the pose) and `draws` (the skinning palettes) for a plate knight with sword,
shield and cape, a ranger with a bow and an elf mage with a staff, at each LOD at full detail, then
at half and quarter detail, and counts what posing allocates once it is going (nothing). They go
over the slopes west of the village with their feet on the ground, following with their eyes a
point that sweeps round and up and down in front of them; `off` for the feet leaves them as the
clips have them. The cases: `idle`, `walk` (2.5 m/s, between the walk and jog loops), `run`
and `strafe` (running square to the facing) hold their pose, `turn` stands looking 80° either way
and back every two seconds (the feet step round on every sweep), `slash` and `run + slash` start
the slash over every half second, and `start-stop` runs and stops every half second, so those
three are blending most of the time, and the body leans setting off and stopping. `circle` runs
round at 6 m/s, leaning into the turn, and `jump` runs and jumps every second, the legs reaching
for the ground on the way down and the pelvis dipping on landing.

Instructions per pose at full detail (`update_combat`, called through `animate` since #215), from
callgrind (6 characters, 240 frames, every LOD):

| Case | Before #210 | After #210 | After #204 | After #110 | After #211 | After #215 | After #214 | After #212 | After #217 | After #218 | After #213 | After #248 | After #305 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| idle | 166,229 | 140,466 | 83,313 | 83,697 | 83,992 | 39,856 | 50,644 | 53,008 | 53,322 | 53,520 | 53,724 | 53,865 | 53,866 |
| turn | | | | 83,657 | 90,315 | 48,176 | 59,242 | 61,652 | 61,985 | 62,189 | 62,394 | 62,445 | 62,454 |
| walk | | | 88,445 | 104,860 | 105,340 | 54,396 | 64,644 | 67,144 | 67,403 | 67,602 | 67,800 | 67,936 | 67,943 |
| run | 210,673 | 184,868 | 88,894 | 93,491 | 93,975 | 46,861 | 58,538 | 61,138 | 61,427 | 61,625 | 61,830 | 61,963 | 61,974 |
| strafe | | | 88,893 | 93,947 | 95,511 | 47,971 | 59,770 | 62,272 | 62,530 | 62,728 | 62,935 | 63,071 | 63,080 |
| slash | | 162,570 | 106,448 | 106,852 | 107,122 | 61,480 | 72,667 | 72,724 | 72,739 | 72,931 | 73,132 | 73,275 | 73,269 |
| run + slash | 228,680 | 222,870 | 113,124 | 115,674 | 116,695 | 68,288 | 80,085 | 80,130 | 80,204 | 80,397 | 80,593 | 80,722 | 80,733 |
| start-stop | 187,279 | 185,645 | 110,360 | 112,854 | 113,244 | 67,341 | 77,990 | 80,925 | 82,498 | 82,908 | 83,105 | 83,247 | 83,261 |
| circle | | | | | | | | 61,546 | 63,414 | 63,842 | 64,047 | 64,213 | 64,220 |
| jump | | | | | | | | 83,542 | 85,944 | 86,151 | 86,346 | 86,484 | 86,505 |
| run + hits | | | | | | | | | 93,452 | 91,832 | 92,041 | 92,238 | 92,256 |
| run + rolls | | | | | | | | | | 83,603 | 91,053 | 91,208 | 91,253 |
| climb | | | | | | | | | | 78,142 | 92,481 | 85,653 | 85,667 |
| swim | | | | | | | | | | 67,993 | 44,598 | 44,748 | 44,757 |
| hang | | | | | | | | | | | 60,810 | 83,782 | 83,783 |

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

#214 puts the feet on the ground at full detail: 9,500 to 11,200 more instructions a pose (15% to
27%). With the feet `off` the bench costs what it did after #215, within 1.3%. About a third of the
extra is the two-bone IK solves, a fifth rebuilding the legs' world matrices, a sixth sampling the
ground (under the body and each ankle, and either side of each for the slope: 15 samples), a tenth
dropping the pelvis, and the rest working out where each foot goes. The pelvis carries nearly the
whole skeleton, so the world matrices of everything on it move with it in one flat pass instead of
being rebuilt. Wall clock on the cloud runner, posing at full detail goes from 7.8 to 9.6 µs a
character a frame, so the 8 full-detail characters cost about 15 µs more a frame, however big the
crowd.

#212's gaze costs 2,400 to 2,900 instructions a pose while the character looks anywhere but
straight ahead (4% to 5%; the bench has everyone follow a point that never stops moving).
Looking ahead, it turns nothing, and held faded out (slashing over and over, or at quarter detail)
it isn't run at all. Wall clock on
the cloud runner can't tell it from the noise: posing at full detail measured 9.5 µs a character a
frame, against 9.6 after #214, and quarter detail is unchanged.

#217 costs 260 to 330 instructions a pose at a steady pace (0.4% to 0.6%), most of it a clip
lookup a frame for the idle's length, to phase it; the lean, settled, isn't worked out at all, and
while an action plays it costs next to nothing. Leaning costs more: setting off and stopping twice
a second (`start-stop`) 1,600 more (1.9%) and running round (`circle`, new with #217, as is `jump`;
their After #212 numbers are #212's code running them) 1,900 (3.0%), for the springs and turning
the pelvis and spine; jumping every second (`jump`) 2,400 (2.9%), with the legs reaching for the
ground (sampling `Jump_Land` on top of the loop) and the pelvis dipping. Wall clock on the cloud
runner is within the noise: the fastest of six runs of each case measured 1% to 6% slower at full
detail, where runs of the same build differ by up to 15%. At quarter detail the lean and the dip
are off, and what's left (the jump's timing, the reach and the idle's phase) costs next to nothing.

#218 costs about 150 instructions a pose when nobody is being hit (0.2% to 0.3%): the flinch,
settled, isn't worked out, and what's left is checking it and the pose the body shows. The cases
that lean come out about 220 dearer again (`start-stop` 0.4% in all, `circle` 0.6%). A runner taking
a blow every half second (`run + hits`, new with #218; its After #217 number is #239's code running
it, each blow playing `Hit_Chest` in full) costs 1,620 fewer (1.7%): the flinch's springs, tipping
six bones and building the world matrices again while it moves cost less than switching to the hit
clip and back. The column is against #239's code measured the same way, within 60 instructions of
the After #217 column throughout.

#213 costs about 200 instructions a pose for a body that isn't carried or afloat (0.2% to 0.4%):
looking for a move to follow, and the rest of one's clip and the facing, settled. Its three cases
are new (their After #218 numbers are #218's code running them, the moves' clips by the clock):
`run + rolls` runs and rolls 5 m to one side and then the other every second, and `climb` climbs a
1.2 m ledge every second and runs on over its top to the next, so both are carried through moves
about half the time and then recover from them; `swim` swims at 2.7 m/s. Rolling costs 7,450 more
(8.9%): the blender settles a switch more each time (4,300), as the legs take up the run under the
roll's standing up, and that plays on the upper body over the stride, two clips where #218 played
one (3,800). Climbing costs 14,300 more (18%), nearly all of it the hands holding the edge (13,100:
the arms' two-bone IK, and rebuilding each arm's bones as each joint turns), from the start of each
climb until they have let go. Swimming costs 23,400 fewer (34%): the swim loop plays alone, where a
body running through the river blended the walk and the jog on one stride (15,000), and nothing puts
the feet on the ground afloat (10,000). Wall clock on the cloud runner (the median of six runs at
full detail) goes the same way: rolling 15% dearer, climbing 17%, swimming half the cost, and `run`
within the noise.

#248 costs 33 to 47 instructions a pose for a body that isn't on a wall (under 0.1%): checking for a
wall, and for a raise still to come down from. Turning comes out 72 cheaper (0.1%) and climbing
6,960 (7.5%): the arms' two-bone IK now turns the shoulder and the elbow and rebuilds the arm's
bones once, where it rebuilt them after turning each joint, which pays wherever a hand reaches for
something (a staff's off hand in `turn`, the hands on the edge in `climb`). Its case is new (its
After #213 number is #213's code running it, the clips by the clock with the hands and feet where
they put them): `hang` hangs 1 m off the ground from a 6 m hold, shimmying right, then left, then
hanging still, 0.8 s each. Hanging costs 22,970 more (38%): moving the body by its reach about
2,100, as everything on the pelvis moves with it, and the hands on the hold and the feet on the
wall the other 20,900, two thirds of that the arms' IK (rebuilding each arm's bones, fingers and
all). The bench's hold is a closure; in the game, finding it in the client's copy of the valley's
holds costs about 1,440 more a look (860 searching the holds near the body, the rest the ground
under it), once a frame for a body hanging and twice for one moving between holds, and allocates
nothing. Wall clock on the cloud runner (the median of nine runs at full
detail), hanging goes from 8.9 to 12.2 µs a character a frame; at the rate #210 measured on the
M1, nearly twice the runner's, the difference is under 2 µs. At half detail it's paid every other
frame, and at quarter detail not at all. The column is against #213's code on today's main measured
the same way, 86 to 161 instructions above the After #213 column, which was measured on an older
main.

#305 moves every case by 45 instructions a pose or less, either way (under 0.05%), within the noise:
off the wall, a look at whether the body has just kicked off it, and on it, which of the moves'
clips plays. The new clips play as the others do, so a body leaping, ejecting, wrapping round a
corner or running up a wall costs what a body moving between holds does, and one kicking off or
rolling a landing what a jump or a roll does.

Where the time goes now (callgrind, `run` at full detail, a third of the characters caped): about
a third in cape springs, a third in world matrices and a fifth in sampling clips. The palettes are
the next cost to take on (#243): they're rebuilt every frame at every tier, and half of them are
copies of another part's.

## Next

In the plan's order: #244, remote players' real aim in the snapshot. Directional hit clips (#106,
picked by #111) go under the flinch when they land: it adds to whatever plays. Directional rolls
(#105) and the mantles and swims (#108) go into traversal when they land: a roll picks the clip
nearest its direction from the facing instead of turning the body, and each new clip brings its
`travel` curve.

One thing to watch: combat clips take their time from `UnitCombat::action_time` in each snapshot.
Snapshots go out every tick today; if the send rate drops to 20–30 Hz (#73), attacks would play in
steps unless the client runs their clock between snapshots.
