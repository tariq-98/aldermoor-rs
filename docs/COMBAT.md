# How fighting works

The combat core (milestone 2), as built in `crates/sim`. The content tables (abilities, auras,
weapons, specialties and monsters) are RON files in `crates/content/combat/`, loaded and checked
by `crates/content/src/combat.rs` (a bad file names its line and column; the types there document
every field and its default); the rules are in `crates/sim/src/combat/`. What the client receives is in [CONTRACT.md](CONTRACT.md).

The feel: tab-targeting and a skill bar like Guild Wars 2 and World of Warcraft, with WASD
movement and action-game defence (rolling, guarding, parrying, aiming). Levels carry most of the
power, and player skill decides close fights (DESIGN.md, "Progression and player skill").

## The bar

| Keys | Skills | Cost |
|---|---|---|
| 1–3 | The main-hand weapon (1 is the basic attack, which repeats while you have a target) | Time only: cooldowns |
| 4–5 | The off-hand weapon | Time only |
| 1–5 | A two-handed weapon, all five | Time only |
| 6 | The specialty's heal | The specialty's resource |
| 7–9 | Specialty utilities (some specialties have more to choose from) | The resource |
| 0 | The specialty's elite skill | The resource, or a long cooldown |

- **Two weapon sets** per character; `SwapWeapons` changes over, then not again for 5 s.
- **Bare hands:** Punch.
- **The global cooldown** is 1 s. Basic attacks aren't on it and give way to any other skill.
  A skill pressed in the last half-second of the global cooldown goes off when it ends.

## Specialties

Health, power and armour are multipliers on the level's base numbers (below).

| Specialty | Resource | In combat | Out of combat | Starts | Health | Power | Armour | Weapon sets |
|---|---|---|---|---|---|---|---|---|
| Warrior | Rage, built by hitting and being hit | +0 /s | −5 /s | 0 | 1.15 | 1.0 | 0.6 | Sword and shield; greatsword |
| Archer | Focus | +8 /s | +12 /s | 100 | 1.0 | 1.0 | 0.35 | Longbow; hunting bow |
| Mage | Mana | +6 /s | +15 /s | 100 | 0.9 | 1.1 | 0.2 | Oak staff; wand and spellbook |
| Necromancer | Essence, built by harm done | +0 /s | −4 /s | 0 | 0.95 | 1.05 | 0.3 | Gravecaller's staff; dagger and spellbook |
| Rogue | Energy | +12 /s | +20 /s | 100 | 0.95 | 1.05 | 0.3 | Two daggers; sword and dagger |
| Healer | Mana | +6 /s | +15 /s | 100 | 1.0 | 0.95 | 0.4 | Sunward staff; mace and spellbook |

## Numbers

- **Levels:** health, power and armour grow 6% a level, compounding: level `L` has
  `1.06^(L−1)` times a level-1 fighter's. Level 1: 200 health, 30 power, 100 armour.
- **Damage:** `base × 1.06^(level−1) + coeff × power`, then the attacker's and target's auras,
  armour, and a 5% chance of a critical hit at 1.5 times.
- **Armour** stops a share of physical blows only: `armour / (armour + 200 × 1.06^(attacker
  level − 1))`, at most 75%.
- **From behind** (more than 110° off where the target faces): 1.1 times, and nothing guards
  against it. Backstabs land harder still from behind (twice, for the dagger's).
- **Executes** (the greatsword's Execution) hit 2.5 times as hard below a quarter health.

## Melee blows

- **A player's melee blow sweeps the cone in front of them** and lands on every enemy in it, up
  to the skill's limit (five unless it says otherwise), so positioning matters. Each weapon in
  the main hand sweeps its own width out to the blow's range:

| Weapon | Reach (basic attack) | Sweep |
|---|---|---|
| Dagger | 2.6 m | 80° |
| Sword, axe, shield bash | 3 m | 92° |
| Mace | 3 m | 86° |
| Greatsword (Execution) | 3.6 m | 103° |
| Spear | 4.5 m | 52° |
| Bare hands, bows, staves | as the skill | 80° |

- **Where it sweeps:** at your target while it's in reach (it needn't be in front: with a Tab
  lock the blow turns to it). Otherwise along your aim (the crosshair, or the pointer in Classic),
  turned by up to 12° (`AIM_PULL`) toward the enemy nearest that line, so a near miss still lands.
  With nothing in reach the skill isn't used ("Nothing in reach.").
- **Only the main victim is controlled:** the target, or else the nearest enemy, takes the whole
  blow. The others in the sweep take its harm (damage, bleeding and other conditions), but not its
  stuns, dazes, knockdowns, interrupts, taunts or defiance damage.
- **Wind-ups keep their aim:** a heavy blow at no target lands where it was aimed when it comes
  down, so a foe that steps out of it is missed ("Your blow misses."). One at a target follows it
  while it stays in reach.
- Charges, Shadowstep and Skewer's hook stay single-target, as do cones, lines and circles,
  which keep their own shapes and limits. **Monsters' blows** land on their target only; their
  big blows are warned of on the ground instead.

## Casts, wind-ups and channels

- **Casts** take time, shown as a cast bar. Some can be cast on the move; the rest stop if you
  move. Rolling breaks off any cast, and a channel unless it can be moved in.
- **Heavy melee blows wind up for 0.4 s** before they land (Rending Blade, Cleave, Crushing
  Blow, Tremor, Backstab, Sweep, Impale, Earthsplitter, Execution): long enough for the other
  side to see coming and parry or roll. Quick skills (bashes, lunges, sidesteps) are instant.
- **Channels** tick while you hold them, and stop if you're interrupted.
- **Interrupts** stop a cast and lock its caster out of skills for a few seconds.
- **Monsters' big blows** wind up with a warning on the ground (a circle, cone or line), and keep
  the aim they started with: get out, or roll through.

## Missiles

- **Homing** missiles follow their target; only a wall stops them.
- **Aimed** missiles fly straight and strike the first enemy in their path, so a target that
  moves can make them miss. Players aim instant shots with the point they send; cast bolts fly at
  where the target is when they're loosed. Monsters lead a moving target, so running in a straight
  line won't save you; changing direction will.
- **Piercing** missiles strike everything along their flight.
- **Shield Wall** turns missiles back on whoever loosed them.

## On the ground

- **Warned blows:** arrows and meteors fall on a spot a moment after they're loosed.
- **Fields** pulse on whoever stands in them. Combo fields (fire, ice, light, water, poison,
  lightning) change finishers that go through them:

| Field | Blast | Projectile | Whirl | Leap |
|---|---|---|---|---|
| Fire | Might for friends | Burning | Burning | Might |
| Ice | Chilled | Chilled | | |
| Light | Cleanse friends | | Cleanse friends | |
| Water | Heal friends | | | Heal |
| Poison | Weakness | Poisoned | | |
| Lightning | | Vulnerable | | |

- **Traps** wait for the first enemy to step in.

## Auras and crowd control

- **Boons:** might, fury, quickness, swiftness, protection, regeneration, riposte; **conditions:**
  bleeding, burning, poisoned, weakness, vulnerable, chilled, crippled; **shields** absorb harm.
- **Crowd control:** stun, knockdown, daze (no skills), silence, root, fear, sleep, slow.
- **Diminishing returns:** within 18 s, repeats in the same group (stuns and knockdowns; roots;
  fears and sleep; silences and dazes) last half as long, then a quarter, then don't land.
- **Defiance:** elites and bosses shrug off crowd control while their defiance bar holds. Control
  chips it (60 a second of control it would have held; 90 for a knockback), and so do skills that
  break defiance. Broken, they're stunned for 3 s and take 50% more for 6 s; the bar is back
  after 9 s.

## Guarding, parrying and rolling

- **Guard** (hold the guard key): blows from in front are blocked, at a cost in endurance (the
  same endurance rolls spend). While guarding you can only walk, your basic attack waits,
  endurance doesn't come back, and using a skill drops the guard.
  - **Shields** cover a wide arc (75° either side), stop missiles, and cost 15 a blow.
  - **Other melee weapons** cover 50° either side, can't stop missiles, and cost 25 a blow.
  - **A stronger foe's blows cost more** to stop: as much more as the attacker's level outweighs
    yours (1.8 times five levels up, 3.2 times ten up).
  - **Out of endurance,** the guard breaks and you're dazed.
- **Parry:** raise the guard within 0.25 s before a blow you saw winding up lands. It costs
  nothing; the attacker staggers (dazed for 1.2 s, taking 15% more), and your next blow within
  2 s is a riposte (half as hard again). Quick blows give no warning: block them or take them.
- **Unblockable blows** (the big monster slams) must be rolled through or outrun.
- **Rolling** costs 50 endurance, carries you 5 m, and nothing lands for 0.4 s. Endurance
  refills at 8 a second.

## Threat

- Monsters fight whoever they hate most: damage done to them (times stances' threat), and half
  of the healing done on whoever they're fighting.
- They change targets only when someone pulls 10% ahead (in melee) or 30% ahead (at range).
- **Taunts** put the taunter on top for 3 s.
- **Packs:** wolves and goblins call the members of their pack close by.
- **Leashing:** a monster gives up past 45 m from home, or after 12 s with nobody fighting it,
  and walks home untouchable, healing.

## Down, out and back

- **Downed:** a player whose health runs out is downed, not dead. The downed pool is half their
  health and bleeds away over 18 s (faster if they're hit). They can crawl and use the downed
  skills: a desperate strike, a shove that interrupts, and a bandage.
- **Getting up** (with a quarter of their health):
  - felling an enemy they hurt in the last 10 s;
  - a friend's help: 3 s of standing still beside them (up with half their health);
  - healing that fills the pool back up, from a friend or the bandage.
- **Defeated:** respawn at home whenever you choose, or be raised where you fell by a friend
  once nobody's fighting (6 s, back with 30% health and weakened for 30 s).
- **Monsters** come back where they started after a while; bosses after longer.

## Summons

A necromancer's skeletons (and a boss's dead) fight at their summoner's level, on their side.
They follow their summoner, fight what it fights, and crumble when their time is up or their
summoner falls.

## Balance

Levels: five levels up is about twice the fighter (each side's health and damage both grow 34%,
and armour shifts with level); ten levels up is about four times.

Checked by simulated duels between warriors (`crates/sim/tests/duels.rs`): a well-played fighter
(timed parries with human error, ripostes, blocks while endurance lasts, heals and breaks free
at the right moments) against a badly-played one (mashes whatever is ready, never guards or
rolls). Skill wins, out of 40 duels each (24 October 2026; unchanged when melee sweeps came in on
25 October, since a duel has one foe in the sweep):

| Level gap | 0 | 2 | 4 | 5 | 6 | 7 | 8 | 10 |
|---|---|---|---|---|---|---|---|---|
| Skill wins | 40 | 40 | 39 | 37 | 23 | 4 | 0 | 0 |

So skill reliably beats up to five levels, it's even at six, and seven or more levels win. The
tests hold the targets: skill wins at the same level, usually wins five levels down, and loses
ten down. `cargo test -p sim --release --test duels sweep -- --ignored --nocapture` prints the
table again after tuning.

## Checks

- `crates/sim/tests/combat.rs`: resources, casts, cooldowns, the queue, interrupts, melee sweeps
  (who's in them, aim pull, locked targets, reach and width, wind-ups missing),
  diminishing returns, defiance, warned blows, blocks, parries, guard breaks, blows from behind,
  backstabs, missiles (blocked, reflected, aimed, led, homing), downed and rallying, bleeding out,
  respawning, reviving, summons, combos, the combat view, determinism, every skill on every bar,
  and the step budget (in release).
- `crates/sim/tests/duels.rs`: the balance targets above.
- The Swift game's combat checks are covered by these.
