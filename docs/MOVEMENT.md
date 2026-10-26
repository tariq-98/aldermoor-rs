# Movement

How bodies move in the sim (`crates/sim/src/movement.rs`). The numbers are constants at the
top of that file. The ground is `world::Valley::ground`, and the river is
`world::Valley::water_depth`.

- **Run** 6 m/s, **walk** 2.5 m/s (the walk toggle), **backpedal** at 0.6× while facing away
  from the way you move.
- **Sprint** (Shift) at 1.5× the run speed. It works out of combat only (6 s after the last
  blow given or taken), on land, and not while guarding, casting, downed or feared. It costs
  nothing.
- **Getting going** takes a moment, so bodies have some weight without losing the quick starts
  and stops dodging needs (WoW and Guild Wars 2 are near instant; action games take 50 to
  150 ms). On foot, standing to a run takes 0.08 s (`START_UP`), a run to a stop 0.06 s
  (`STOP_TIME`), and a run into a sprint 0.3 s (`SPRINT_UP`). Turning round drops the backward
  part of your speed at once, so reversing is as quick as starting, and turning keeps your speed.
  Monsters ramp the same way. Rolls, leaps and climbs, swimming and the air are unchanged, and a
  root or stun still stops you dead.
- **Jumps** forgive a little: a jump pressed up to 0.15 s before landing goes off as you land,
  and one pressed up to 0.1 s after running off an edge still jumps (coyote time). The windows are
  `JUMP_BUFFER` and `COYOTE_TIME`; rolls have their own (`DODGE_BUFFER`, in COMBAT.md).
- **Cliffs:** ground steeper than 2:1 (about 63°) is a wall. Walking into one either climbs it
  or slides along it.
  - A face that tops out within **1.5 m** of your feet (chest height) is climbed: a 0.6 s pull-up
    onto the top, as a forced move (no skills meanwhile, pose `Jump` until climbing art exists).
  - A taller face stops you, unless you jump at it or run up it (a wall run, below). In a jump,
    a face that tops out within **2.2 m** of your feet at that moment is caught and pulled up
    onto in 0.5 s (a mantle, `ForcedKind::Mantle`), as long as you aren't yet falling faster than
    3 m/s. So a jump at the right moment gets you up a ledge about 3.3 m high. A climb or mantle
    needs room on the top for the body: not a sill's top against the wall it stands out from,
    nor a cliff's ledge too narrow to stand on below the next. On a narrow top it stands you as
    near its front as you fit.
  - **Vaulting:** running (faster than a walk) at a wall no higher than **1.2 m** and shallower
    than **0.7 m**, with clear, level ground beyond, hops it at the speed you ran in (0.2 to
    0.45 s) and lands you running at that speed (`ForcedKind::Vault`). Deeper walls, such as the
    0.9 m field walls, are climbed onto, and walking at a low wall doesn't vault it. In Brambleford
    today only the ruin's fallen stones are thin enough to vault.
  - Not while guarding, casting, swimming or downed. Mantles and vaults are forced moves, so no
    skills meanwhile.
  - The valley has few low faces yet: a few ledges in the limestone terraces west of the
    village. Most of the terraces and the east terrace are tall cliffs; two stretches of them
    climb by their ledges (below).
  - Walking or running off the top of a cliff is a fall (see below), not a slide down it. You
    only stay on the ground going down ground no steeper than 2:1.
  - The bridge has stone parapets along both sides (solids in `world`), so you can't climb onto
    the deck from beside it; get on at either end.
  - Walls and ruins can be climbed too. A `world::Solid` with a `top` is ground, not an
    obstacle: its sides are faces like any cliff's, so one that tops out within 1.5 m is climbed
    and stood on, and a taller one stops you a body's width short of it, as a building's wall
    does (#216; `world::Valley::off_faces`). It stops rolls, leaps, charges, knockbacks and
    blinks the same way, rather than carrying you over it (#311; `world::Valley::slide_below`).
    The shed, the windmill and trees have no top and only block; the cottages are tops (below).
    A top may rise along the box (`rise`), so a wall following a slope is walked up, not climbed
    a step at a time.
  - The dry-stone field walls along the north road stand about a metre high and 0.9 m wide on
    top: climb on and walk along them. The ruined hall on the hill south-west of the village
    (#160) breaks down in steps along its east wall, each a climb from the last, up to a lookout
    4.2 m up under its gable; the gable's buttresses and the door's jambs are too tall to climb
    (`crates/world/src/masonry.rs`). The gable itself climbs (#249): from the lookout a jump pulls
    you onto its raked coping, 2.5 m up, which you walk up (at 40°) to the empty bellcote on its
    apex. A jump at the bellcote pulls you onto its top, 10 to 10.8 m up, the highest point in
    the ruin. The gable's outer face climbs too, by its grips (below).
  - The cottages climb too (#249; `crates/world/src/cottage.rs`). Each is a top: its roof's two
    slopes run down to its eaves, 2.6 to 3.4 m up, and only 0.12 m past its gable ends, with the
    ridge's roll or cap and the chimney stack on top. Walking at one stops you a body's width
    short of its eaves or gable end. A jump at the eaves pulls you onto the roof (or, with Shift
    held, catches them to climb up), and a wall run up the side pulls straight onto it. The roof
    is walked up and over its ridge (under 2:1 all the way), and the chimney, 2 to 2.5 m above
    the roof beside it, is a jump and a mantle. The gable ends climb by their timbers, which are
    grips: the window heads and the lintel over the door, 2.4 to 3.2 m up and caught in a jump
    from the ground, then the wall plate across the top of the wall, the gable window's sill and
    head (on the front) and the collar, each a climb or a climb leap from the last, then the
    verge, the roof's sloping edge, and up onto the roof. The back of the house by the river
    stands over its bank, so its timbers start out of reach.
- **Climbing** on handholds (#245, part of the climbing epic #251;
  `crates/sim/src/climbing.rs`). A hold is an edge the hands close on: `world::Valley::holds`,
  every top edge of a `Solid` with a top that drops at least 1.6 m (`world::HANG_MIN_DROP`),
  joined along straight walls, and every grip (`world::Grip`): a dressed stone or a timber
  standing proud of a face, held by its top, with no room above it to climb onto. Grips don't
  stand in anyone's way. In the valley that's the ruined hall's taller broken walls, its gable's
  coping and bellcote, and a line of grips up the gable's outer face: the lancet's sill (2.6 m,
  caught in a jump from the ground), two jamb stones and a label stop up the window's east side
  (each a climb from the last), the keystone over its arch (a climb leap), the oculus's sill, and
  the bell's sill in the bellcote (a climb leap), from which a last climb leap reaches the
  bellcote's coping. The cottages add their eaves and verges and the timbers up their gable ends
  (above), and two cliffs their ledges:
  - **Cliff ledges** (#249; `crates/world/src/ledges.rs`): bands of pale, harder stone along the
    strata of two cliffs, 1.7 to 1.8 m apart, so each front is a hold: the west terrace's face
    behind the village (16 m) and the east terrace's below the windmill (26 m). The rest of the
    cliffs stay sheer. Each ledge runs back into the rock, and its front stands right above where
    the cliff meets the ledge below, so the next ledge up overhangs it. A jump catches the first
    from the foot of the cliff. Where a ledge is deep enough to stand on (about half a metre),
    you climb onto it, and a jump at the cliff pulls you onto the next or catches it. Where the
    cliff is steeper and the ledges are narrower, you hang, and a climb leap reaches the next
    ledge. The last is a course of capstones along the edge of the terrace, which you climb onto
    and walk off onto the terrace.
  - **Hanging:** hands on the hold, feet 1.9 m below it (`HANG_DROP`) or on the ground if that's
    higher, the body out from the wall by its radius and facing it. Pose `Hang`.
  - **Moving on the wall** goes by the direction pressed, against the wall: along it shimmies at
    1.2 m/s (`SHIMMY_SPEED`), stopping 0.2 m short of the hold's ends or hopping in 0.3 s to a
    hold that carries on within 0.6 m (`HOLD_GAP`). Past the end, or up against a wall across
    it, it wraps round the corner onto a hold meeting this one at a right angle within 0.6 m,
    round an outer corner or into an inner one, in 0.5 s (`CORNER_TIME`, `ForcedKind::Corner`),
    swinging round the corner clear of it. Into the wall climbs onto the top in 0.8 s when
    there's room (`CLIMB_UP_TIME`), or else to a hold up to 1 m higher (`HOLD_CLIMB`, in 0.4 s).
    Away from the wall climbs down to a hold up to 1 m lower, and does nothing if there isn't
    one, so you never fall by accident.
  - **Leaps and ejects:** Space while hanging, by the direction pressed (#246). Into the wall or
    none: a climb leap to a hold 1 to 1.8 m higher and up to 0.8 m to either side, in 0.5 s
    (`LEAP_REACH`, `LEAP_SIDE`, `LEAP_TIME`); with none in reach, Space adds nothing to the
    direction. Along the wall: a side eject to a hold 0.8 to 2.5 m that way and up to 1 m up or
    down (`EJECT_REACH`, `EJECT_RISE`), in 0.55 s with a hop, or, with none there, a jump off the
    wall that way (4 m/s along it, 4.5 m/s up). Away from the wall: a back eject, kicking off at
    5 m/s out and 5 m/s up (`BACK_EJECT`), about 2.5 m out by the time you're back at the height
    you left if you keep pressing away, turning as you go to catch what's behind you. For a
    second after any leap or jump off a wall, holds are caught without Shift (`CATCH_WINDOW`), so
    chains need no key held.
  - **Roll** lets go: you drop, pushing off the wall. Standing on a top within 1 m of its edge,
    Roll toward the drop (or with no direction, backing onto it) lowers you over the edge to hang
    from it in 0.6 s. Rolling along the edge or away from it still rolls.
  - **Catching:** with Shift held out of combat, falling or jumping past a hold grabs it when the
    hands (2.1 m above the feet) pass within 0.3 m of it, settling in 0.2 s. A hold you just let
    go of can't be caught again for 0.4 s, so letting go with Shift held catches the next hold
    down. Without Shift you fall past. Heading into the face of a top this jump can pull up
    onto, with room on it, you mantle it instead of hanging.
  - **Wall runs:** sprinting (Shift, out of combat) on the ground straight at a wall too tall to
    climb runs up it (`ForcedKind::WallRun`): a face more than 1.5 m above your feet, or a
    building's side at least 2 m wide (not a tree or a parapet; `world::Valley::wall_ahead`). The
    feet rise 1.8 m in 0.5 s, slowing (`WALL_RUN_RISE`, `WALL_RUN_TIME`). A top the hands reach,
    up to 4.2 m above where the feet set off, is pulled straight up onto if there's room on it;
    otherwise the hands catch the first hold they pass, up to that height. Space kicks off
    backwards, as a back eject. With nothing caught, you drop back off at the end. Pose `Jump`
    until it has a clip.
  - **Roll landing:** Roll in the air readies a roll landing instead of rolling. Pressed up to
    0.2 s before you land (`LANDING_ROLL`), you roll on 3 m the way you were going, and falls up
    to 8 m don't hurt (`ROLL_SAFE_FALL`; past that each metre costs as usual). Pressed earlier,
    it's lost.
  - No skills or guard while climbing ("You can't fight while climbing."). A knockback throws
    you off the wall; being downed makes you let go, and the dead drop where they are. Hanging
    bodies neither push nor are pushed.
  - Leaps, ejects and corners play the hang's climbing clips (#247) until they have their own.
- A low wall too thin for the climb's ground samples (under about 0.5 m deep) is climbed onto
  like a deep one (#238).
- **Swimming:** the river is no longer a wall.
  - Water deeper than 0.5 m is swum (until it's shallower than 0.4 m). Swimmers float with
    their feet 0.5 m under the surface (`world::WATER_LEVEL`), at 0.45× run speed.
  - Shallower water is waded at 0.7×.
  - Swimmers can't use skills, guard, roll, jump or sprint. Starting to swim drops your guard,
    breaks off a cast and stops auto-attacking. `PlayerState.swimming` tells the HUD, which
    shows a Swimming badge; refusals such as "You can't fight while swimming." show as
    messages at the top of the screen.
  - Monsters don't wander into water or spawn in it.
  - The bridge deck is still a single height over the river, so you can't swim under it.
- **Falls** of up to 4 m are free. Past that, each metre costs 6% of max health (a 10 m drop
  costs 36%), which can down or kill you. The fall is measured from the highest point since you
  last stood on ground or water. Landing in deep water breaks the fall.

- **Bodies don't overlap.** Two living bodies that overlap are eased apart, half the overlap
  every other step (30 times a second, to keep the cost down at crowd sizes). The push leans a
  little sideways, so bodies slide round each other instead of queueing in a line. The lighter one
  moves further: bigger bodies outweigh smaller ones, and players count four times their size, so
  a pack spreads round a player instead of shoving them about. Nobody is
  pushed up a wall or off a cliff. Rolls, leaps, lunges and climbs pass through bodies, and
  whoever they land on is eased apart afterwards.
- **Friendly players pass through each other.** Two players who can't harm each other overlap
  freely, so nobody can block a doorway, a bridge or a vendor (as in WoW and Guild Wars 2).
  Players on opposing sides still collide, and so do players and monsters, so body-blocking stays
  part of a fight.
- **Packs surround their prey.** Once a melee monster is close to its target, it makes for its own
  place on a ring just inside its reach. It starts from the side it came in on and moves round
  until it has its share of the circle: evenly split between everyone fighting that target, or
  at least a body's width once a crowd is too big for that. Three wolves end up about 100°
  apart. Places are rechecked four times a second.

Every unit's `traversal` in the snapshot says whether it's wading, swimming, hanging from a hold
or in a forced move (roll, leap, charge, climb, a move onto a hold or round a corner, a wall run,
knockback, with where and when it starts and ends), so animation can show it on other players
and monsters too (CONTRACT.md).

Not yet: free climbing anywhere (holds only, by design), mounts, clips for leaps, ejects, wall
runs and corners, and swimming animations (walk and idle in water).
