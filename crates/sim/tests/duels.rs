//! Simulated duels between a well-played and a badly-played fighter, to check the balance
//! target in docs/DESIGN.md ("Progression and player skill"): within about five levels the
//! better-played fighter should usually win; ten or more levels apart, the higher level should.
//!
//! The badly-played fighter mashes whatever skill is ready and never guards or rolls. The
//! well-played one does what a good player does with human timing: it raises its guard just
//! before blows land (sometimes too early, sometimes too late), ripostes after a parry, holds its
//! heavy blows when one is about to land on it, heals when hurt, and breaks free when dazed.
//!
//! Both fighters are players, so every duel here is fought with the PvP numbers (#200).

use std::f32::consts::PI;

use glam::Vec2;
use protocol::Command;
use sim::content::catalog;
use sim::{Auras, Faction, Fighter, Sim, Specialty, Stats, Transform, Unit, Vitals};

const ROAD: Vec2 = Vec2::new(5.0, 10.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Play {
    Skilled,
    Unskilled,
}

/// A little random sequence for a player's human timing.
struct Jitter(u64);

impl Jitter {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

struct Bot {
    id: u32,
    foe: u32,
    play: Play,
    jitter: Jitter,
    /// The blow it's bracing for: when to raise the guard, and when to drop it again.
    brace: Option<(f64, f64)>,
    /// The foe's wind-up or swing it has already planned for.
    planned: f64,
}

impl Bot {
    fn new(id: u32, foe: u32, play: Play, seed: u64) -> Bot {
        Bot { id, foe, play, jitter: Jitter(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1), brace: None, planned: -1.0 }
    }

    fn act(&mut self, sim: &mut Sim) {
        let t = sim.time();
        let cat = catalog();
        let me = sim.get::<Fighter>(self.id).unwrap().clone();
        let foe = sim.get::<Fighter>(self.foe).unwrap().clone();
        let (a, b) = (sim.get::<Transform>(self.id).unwrap().xz(), sim.get::<Transform>(self.foe).unwrap().xz());
        let to = b - a;
        let gap = to.length() - 0.8;
        let face = to.x.atan2(to.y);
        let direction = if gap > 1.0 { to.normalize() } else { Vec2::ZERO };
        sim.command(self.id, Command::Move { direction, walk: false, sprint: false, face_yaw: Some(face) });
        if me.target != Some(self.foe) {
            sim.command(self.id, Command::SetTarget(Some(self.foe)));
            sim.command(self.id, Command::SetAutoAttack(true));
        }
        let ready = |slot: usize| {
            let Some(id) = me.bar[slot] else { return false };
            let d = cat.ability(id);
            t >= me.ready_at(id) && (!d.gcd || t >= me.gcd_until) && me.resource >= d.cost && me.cast.is_none()
        };
        let health = sim.get::<Vitals>(self.id).unwrap().fraction();
        match self.play {
            Play::Unskilled => {
                // Whatever's ready, in bar order.
                if let Some(slot) = (1..10).find(|&s| ready(s)) {
                    sim.command(self.id, Command::UseSlot { slot: slot as u8, ground: None });
                }
            }
            Play::Skilled => self.skilled(sim, t, &me, &foe, gap, health, &ready),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn skilled(&mut self, sim: &mut Sim, t: f64, me: &Fighter, foe: &Fighter, gap: f32, health: f32, ready: &dyn Fn(usize) -> bool) {
        let cat = catalog();
        // The next blow coming: a heavy blow winding up (seen as it starts), or the foe's next
        // swing (from its rhythm).
        let coming = match foe.cast {
            Some(c) if catalog().ability(c.ability).is_swing() => Some((c.start, c.start + c.duration, true)),
            _ => foe.auto.filter(|_| gap < 3.0).map(|a| {
                let at = foe.ready_at(a).max(t);
                (at - cat.ability(a).cooldown, at, false)
            }),
        };
        let endurance = sim.get::<Vitals>(self.id).unwrap().endurance;
        if let Some((seen, lands, windup)) = coming && lands > self.planned + 0.05 && self.brace.is_none() {
            self.planned = lands;
            if windup {
                // A heavy blow winding up: try to parry it. A human reacts, then raises the
                // guard a little before it lands, give or take: some too early (a block),
                // some too late (it lands).
                let raise = (lands - self.jitter.range(-0.04, 0.38)).max(seen + self.jitter.range(0.15, 0.3));
                self.brace = Some((raise, lands + 0.12));
            } else if endurance >= 60.0 {
                // A quick blow: block it while there's endurance to spare (keeping enough to
                // roll or parry later).
                let raise = lands - self.jitter.range(0.05, 0.3);
                self.brace = Some((raise, lands + 0.12));
            }
        }
        let guarding = me.guard_since.is_some();
        if let Some((raise, lower)) = self.brace {
            if t >= lower {
                self.brace = None;
                if guarding {
                    sim.command(self.id, Command::Guard(false));
                }
            } else if t >= raise && !guarding && endurance >= 15.0 {
                sim.command(self.id, Command::Guard(true));
                return;
            }
        }
        if guarding {
            return;
        }
        let riposte = has(sim, self.id, "riposte");
        let foe_reeling = has(sim, self.foe, "staggered") || has(sim, self.foe, "dazed");
        let quiet = self.brace.is_none_or(|(raise, _)| raise - t > 0.55) || foe_reeling;
        // Break free first; heal and harden when hurt; then the heavy blows, kept for when
        // nothing's about to land (or for the riposte).
        let pick = if (has(sim, self.id, "dazed") || has(sim, self.id, "stunned")) && ready(9) {
            Some(9)
        } else if health < 0.5 && ready(5) {
            Some(5)
        } else if health < 0.45 && ready(7) {
            Some(7)
        } else if ready(8) {
            Some(8)
        } else if (riposte || quiet) && ready(1) {
            Some(1)
        } else if ready(3) && (foe.cast.is_some() || riposte) {
            // A bash to stop a wind-up, or to follow a parry.
            Some(3)
        } else if (riposte || quiet) && ready(2) {
            Some(2)
        } else {
            None
        };
        if let Some(slot) = pick {
            sim.command(self.id, Command::UseSlot { slot, ground: None });
        }
    }
}

fn has(sim: &Sim, id: u32, key: &str) -> bool {
    let aura = catalog().aura_id(key).unwrap();
    sim.get::<Auras>(id).unwrap().list.iter().any(|a| a.aura == aura)
}

/// Who wins: `Some(true)` if the first fighter does, `None` if neither goes down in time.
fn duel(seed: u64, first: (Play, u8), second: (Play, u8)) -> (Option<bool>, f64) {
    let mut sim = Sim::new(seed);
    let a = sim.spawn_hero("A", Specialty::Warrior, first.1, ROAD, 0.0);
    let b = sim.spawn_hero("B", Specialty::Warrior, second.1, ROAD + Vec2::new(0.0, 2.2), PI);
    sim.get_mut::<Unit>(b).unwrap().faction = Faction::Hostile;
    let mut bots = [Bot::new(a, b, first.0, seed * 2 + 1), Bot::new(b, a, second.0, seed * 2 + 2)];
    for _ in 0..(120 * 60) {
        for bot in &mut bots {
            bot.act(&mut sim);
        }
        sim.step();
        let (va, vb) = (*sim.get::<Vitals>(a).unwrap(), *sim.get::<Vitals>(b).unwrap());
        if !va.alive() || !vb.alive() {
            return (Some(vb.downed || vb.dead), sim.time());
        }
    }
    (None, sim.time())
}

/// Wins for the first fighter out of `n` duels, and the mean length of a fight.
fn series(n: u64, first: (Play, u8), second: (Play, u8)) -> (usize, f64) {
    let mut wins = 0;
    let mut time = 0.0;
    for seed in 0..n {
        let (won, t) = duel(seed + 1, first, second);
        wins += (won == Some(true)) as usize;
        time += t;
    }
    (wins, time / n as f64)
}

const N: u64 = 12;

#[test]
fn level_alone_decides_a_fight_between_equals_in_skill() {
    let (wins, _) = series(N, (Play::Unskilled, 6), (Play::Unskilled, 5));
    assert!(wins >= 9, "one level up won {wins} of {N}");
    let (wins, _) = series(N, (Play::Skilled, 10), (Play::Skilled, 5));
    assert!(wins >= 11, "five levels up, played as well, won {wins} of {N}");
}

#[test]
fn skill_wins_at_the_same_level() {
    let (wins, length) = series(N, (Play::Skilled, 5), (Play::Unskilled, 5));
    println!("same level: skill won {wins} of {N}, fights lasting {length:.1} s");
    assert!(wins >= 11, "skill won only {wins} of {N}");
    assert!((10.0..90.0).contains(&length), "fights lasted {length:.1} s");
}

#[test]
fn skill_usually_beats_five_levels() {
    let (wins, length) = series(N, (Play::Skilled, 5), (Play::Unskilled, 10));
    println!("five levels down: skill won {wins} of {N}, fights lasting {length:.1} s");
    assert!(wins >= 7, "skill won only {wins} of {N} against five levels");
}

#[test]
fn ten_levels_beat_skill() {
    let (wins, length) = series(N, (Play::Skilled, 5), (Play::Unskilled, 15));
    println!("ten levels down: skill won {wins} of {N}, fights lasting {length:.1} s");
    assert!(wins <= 1, "skill won {wins} of {N} against ten levels");
}

/// A player and an enemy at the same level, a step apart and facing each other. With
/// `player` false the enemy counts as a monster (same body, same armour).
fn squared_up(player: bool) -> (Sim, u32, u32) {
    let mut sim = Sim::new(3);
    let a = sim.spawn_hero("A", Specialty::Warrior, 10, ROAD, 0.0);
    let b = sim.spawn_hero("B", Specialty::Warrior, 10, ROAD + Vec2::new(0.0, 1.6), PI);
    sim.get_mut::<Unit>(b).unwrap().faction = Faction::Hostile;
    if !player {
        sim.get_mut::<Unit>(b).unwrap().kind = protocol::UnitKind::Monster;
    }
    sim.get_mut::<Stats>(a).unwrap().crit = 0.0;
    (sim, a, b)
}

/// The biggest blow `a` lands on `b` in a second of using `ability`.
fn hardest_blow(player: bool, ability: &str) -> (f32, Sim, u32) {
    use protocol::Event;
    let (mut sim, a, b) = squared_up(player);
    sim.get_mut::<Fighter>(a).unwrap().bar[1] = catalog().ability_id(ability);
    sim.command(a, Command::SetTarget(Some(b)));
    sim.command(a, Command::UseSlot { slot: 1, ground: None });
    let mut hardest = 0.0f32;
    for _ in 0..60 {
        sim.step();
        for e in std::mem::take(&mut sim.world.resource_mut::<sim::Outbox>().0) {
            if let Event::Damage { target, source, amount, .. } = e && target == b && source == a {
                hardest = hardest.max(amount);
            }
        }
    }
    (hardest, sim, b)
}

#[test]
fn pvp_numbers_apply_only_between_players() {
    let split = catalog().ability(catalog().ability_id("gs_execution").unwrap()).pvp.damage;
    assert!(split < 1.0, "Execution has a PvP split to test");
    let (on_monster, sim, b) = hardest_blow(false, "gs_execution");
    assert!(sim.get::<Fighter>(b).unwrap().pvp_until < sim.time(), "hitting a monster isn't a PvP fight");
    let (on_player, sim, b) = hardest_blow(true, "gs_execution");
    assert!(on_monster > 0.0 && on_player > 0.0, "both blows land: {on_monster} and {on_player}");
    let ratio = on_player / on_monster;
    assert!((ratio - split).abs() < 0.03, "a player takes {on_player} where a monster takes {on_monster} ({ratio:.2}, want {split})");
    // The player is now in a PvP fight: heals on them take PvP numbers for a few seconds.
    let until = sim.get::<Fighter>(b).unwrap().pvp_until;
    assert!(until > sim.time() && until <= sim.time() + sim::stats::PVP_COMBAT, "in a PvP fight until {until}");
}

#[test]
fn players_control_each_other_with_longer_diminishing_returns() {
    use sim::combat::{auras::apply_key, source};
    use sim::content::Cc;
    let window = |player: bool| {
        let (mut sim, a, b) = squared_up(player);
        let (ae, be) = (sim.entity(a).unwrap(), sim.entity(b).unwrap());
        let src = source(&sim.world, ae, None).unwrap();
        assert!(apply_key(&mut sim.world, "stunned", be, &src));
        sim.get::<Auras>(b).unwrap().dr[Cc::Stun.category().unwrap()].reset_at - sim.time()
    };
    assert!((window(false) - sim::stats::DR_WINDOW).abs() < 1e-6, "against monsters: {}", window(false));
    assert!((window(true) - sim::stats::PVP_DR_WINDOW).abs() < 1e-6, "between players: {}", window(true));
    assert!(sim::stats::PVP_DR_WINDOW > sim::stats::DR_WINDOW);
}

/// For tuning: what happens in skilled-against-unskilled duels at three level gaps (blows,
/// parries, blocks, skills used). `cargo test -p sim --release --test duels explain -- --ignored
/// --nocapture`
#[test]
#[ignore]
fn explain() {
    use protocol::Event;
    for (lvl_a, lvl_b) in [(5u8, 5u8), (5, 10), (5, 15)] {
        let mut tally = std::collections::BTreeMap::<String, f32>::new();
        for seed in 1..=6u64 {
            let mut sim = Sim::new(seed);
            let a = sim.spawn_hero("A", Specialty::Warrior, lvl_a, ROAD, 0.0);
            let b = sim.spawn_hero("B", Specialty::Warrior, lvl_b, ROAD + Vec2::new(0.0, 2.2), PI);
            sim.get_mut::<Unit>(b).unwrap().faction = Faction::Hostile;
            let mut bots = [Bot::new(a, b, Play::Skilled, seed * 2 + 1), Bot::new(b, a, Play::Unskilled, seed * 2 + 2)];
            let who = |id: u32| if id == a { "skilled" } else { "unskilled" };
            for _ in 0..(120 * 60) {
                for bot in &mut bots {
                    bot.act(&mut sim);
                }
                sim.step();
                for e in std::mem::take(&mut sim.world.resource_mut::<sim::Outbox>().0) {
                    let (k, v) = match e {
                        Event::Damage { target, amount, .. } => (format!("damage to {}", who(target)), amount),
                        Event::Avoided { target, how, .. } => (format!("{} {how:?}", who(target)), 1.0),
                        Event::AbilityUsed { unit, ability, .. } => (format!("{} used {ability}", who(unit)), 1.0),
                        _ => continue,
                    };
                    *tally.entry(k).or_default() += v;
                }
                let (va, vb) = (*sim.get::<Vitals>(a).unwrap(), *sim.get::<Vitals>(b).unwrap());
                if !va.alive() || !vb.alive() {
                    break;
                }
            }
        }
        println!("\n== skilled L{lvl_a} against unskilled L{lvl_b} (6 duels)");
        for (k, v) in &tally {
            println!("  {k}: {v:.0}");
        }
    }
}

/// For tuning: how often skill wins across level gaps, 40 duels each. `cargo test -p sim
/// --release --test duels sweep -- --ignored --nocapture`
#[test]
#[ignore]
fn sweep() {
    for gap in [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12] {
        let (mut wins, mut draws, mut time) = (0, 0, 0.0);
        for seed in 100..140u64 {
            let (won, t) = duel(seed, (Play::Skilled, 5), (Play::Unskilled, 5 + gap));
            wins += (won == Some(true)) as usize;
            draws += won.is_none() as usize;
            time += t;
        }
        println!("skilled L5 against unskilled L{:>2}: skill wins {wins:>2} of 40 ({draws} draws), {:.1} s a fight", 5 + gap, time / 40.0);
    }
}
