# Movement

How bodies move in the sim (`crates/sim/src/movement.rs`). The numbers are constants at the
top of that file. The ground is `world::Valley::ground`, and the river is
`world::Valley::water_depth`.

- **Run** 6 m/s, **walk** 2.5 m/s (the walk toggle), **backpedal** at 0.6× while facing away
  from the way you move.
- **Sprint** (Shift) at 1.5× the run speed. It works out of combat only (6 s after the last
  blow given or taken), on land, and not while guarding, casting, downed or feared. It costs
  nothing.
- **Cliffs:** ground steeper than 2:1 (about 63°) is a wall. Walking into one either climbs it
  or slides along it.
  - A face that tops out within **1.5 m** of your feet (chest height) is climbed: a 0.6 s pull-up
    onto the top, as a forced move (no skills meanwhile, pose `Jump` until climbing art exists).
  - A taller face stops you. Jumping at a face measures from the feet at that moment, so a jump
    reaches about 1 m higher ledges.
  - The valley has few low faces yet: a few ledges in the limestone terraces west of the
    village. Most of the terraces and the east terrace are tall cliffs.
  - Walking or running off the top of a cliff is a fall (see below), not a slide down it. You
    only stay on the ground going down ground no steeper than 2:1.
  - The bridge has stone parapets along both sides (solids in `world`), so you can't climb onto
    the deck from beside it; get on at either end.
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

Not yet: free climbing anywhere, mounts, and climbing or swimming animations (pose `Jump` for
climbing; walk and idle in water).
