//! Gathering and crafting skills (#68).

use content::crafts::{crafts, Skill, SWEET_SPOT};
use content::items::economy;
use glam::{Vec2, Vec3};
use protocol::{Command, Event, PlayerState};
use sim::{action_xp, Brain, Inventory, Nodes, Sim, Skills, Transform, Unit, Vitals, Work};

/// The first placed node of a kind, as (id, where).
fn node(sim: &Sim, key: &str) -> (u32, Vec3) {
    let n = sim.world.resource::<Nodes>().list.iter().find(|n| n.def.key == key).unwrap();
    (n.id, n.at)
}

/// A player standing a metre and a half from `at`, carrying `items`.
fn worker(sim: &mut Sim, at: Vec3, items: &[(&str, u16)]) -> u32 {
    let id = sim.spawn_player("Worker", Vec2::new(at.x + 1.5, at.z), 0.0);
    give(sim, id, items);
    id
}

fn give(sim: &mut Sim, id: u32, items: &[(&str, u16)]) {
    let mut inv = sim.get_mut::<Inventory>(id).unwrap();
    for (key, n) in items {
        assert_eq!(inv.add(economy().item(key).unwrap(), *n, false), 0);
    }
}

fn count(sim: &Sim, id: u32, item: &str) -> u32 {
    sim.get::<Inventory>(id).unwrap().count(item)
}

fn xp(sim: &Sim, id: u32, s: Skill) -> u32 {
    sim.get::<Skills>(id).unwrap().xp[s.index()]
}

fn working(sim: &Sim, id: u32) -> bool {
    sim.get::<Work>(id).is_some()
}

/// Puts a player somewhere at once (for walking between stations).
fn teleport(sim: &mut Sim, id: u32, x: f32, z: f32) {
    let y = sim.land().ground(x, z);
    sim.get_mut::<Transform>(id).unwrap().pos = Vec3::new(x, y, z);
}

fn station(key: &str) -> (f32, f32) {
    crafts().stations.iter().find(|s| s.key == key).unwrap().at
}

fn me(sim: &mut Sim, id: u32) -> PlayerState {
    sim.snapshot(Some(id)).player.unwrap()
}

/// Steps until the player stops working, up to `seconds`.
fn work_until_done(sim: &mut Sim, id: u32, seconds: f32) {
    for _ in 0..(seconds * 60.0) as u32 {
        sim.step();
        if !working(sim, id) {
            return;
        }
    }
    panic!("still working after {seconds} s");
}

/// Gathers a node until it runs out, striking every swing at `strike` of the way through it
/// (or never).
fn gather_all(sim: &mut Sim, id: u32, node: u32, strike: Option<f32>) {
    sim.command(id, Command::Gather { node });
    sim.step();
    assert!(working(sim, id), "didn't start");
    let mut struck_this_swing = false;
    let mut last_start = -1.0;
    for _ in 0..(120 * 60) {
        let p = me(sim, id);
        let Some(w) = p.work else { return };
        if w.elapsed < 0.02 || w.elapsed < last_start {
            struck_this_swing = false;
        }
        last_start = w.elapsed;
        if let Some(at) = strike
            && !struck_this_swing
            && w.elapsed >= w.duration * at
        {
            sim.command(id, Command::Strike);
            struck_this_swing = true;
        }
        sim.step();
    }
    panic!("the node never ran out");
}

#[test]
fn mine_ore_smelt_it_and_smith_a_dagger() {
    let mut sim = Sim::new(1);
    let (vein, at) = node(&sim, "copper_vein");
    let hero = worker(&mut sim, at, &[("pickaxe.copper", 1), ("oak_log", 1)]);
    gather_all(&mut sim, hero, vein, None);
    let ore = count(&sim, hero, "copper_ore");
    assert!((4..=8).contains(&ore), "four swings of one or two: {ore}");
    assert_eq!(xp(&sim, hero, Skill::Mining), 4 * 20);
    let n = me(&mut sim, hero).nodes.into_iter().find(|n| n.id == vein).unwrap();
    assert!(!n.ready, "the vein ran out");

    // Smelt two ingots at the forge, then make a dagger at the anvil.
    let forge = station("brambleford_forge");
    teleport(&mut sim, hero, forge.0 + 1.0, forge.1);
    sim.command(hero, Command::Craft { recipe: "copper_ingot".into(), count: 2 });
    work_until_done(&mut sim, hero, 10.0);
    assert_eq!(count(&sim, hero, "copper_ingot"), 2);
    assert_eq!(count(&sim, hero, "copper_ore"), ore - 4);
    let anvil = station("brambleford_anvil");
    teleport(&mut sim, hero, anvil.0 + 1.0, anvil.1);
    let p = me(&mut sim, hero);
    let dagger = p.station.unwrap().recipes.into_iter().find(|r| r.recipe == "dagger.hunting").unwrap();
    assert_eq!(dagger.can_make, 1);
    sim.command(hero, Command::Craft { recipe: "dagger.hunting".into(), count: 1 });
    work_until_done(&mut sim, hero, 10.0);
    assert_eq!(count(&sim, hero, "dagger.hunting"), 1);
    assert_eq!((count(&sim, hero, "copper_ingot"), count(&sim, hero, "oak_log")), (0, 0));
    assert_eq!(xp(&sim, hero, Skill::Smithing), 2 * 20 + 35);
}

#[test]
fn pick_herbs_and_brew_a_potion() {
    let mut sim = Sim::new(2);
    let (patch, at) = node(&sim, "marigold_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    gather_all(&mut sim, hero, patch, None);
    let herbs = count(&sim, hero, "marigold");
    assert!((3..=9).contains(&herbs));
    let table = station("brambleford_alchemy");
    teleport(&mut sim, hero, table.0, table.1 + 1.0);
    sim.command(hero, Command::Craft { recipe: "healing_draught".into(), count: 10 });
    work_until_done(&mut sim, hero, 40.0);
    assert_eq!(count(&sim, hero, "healing_draught"), herbs / 3, "as many as the herbs make");
    assert_eq!(count(&sim, hero, "marigold"), herbs % 3);
    assert!(xp(&sim, hero, Skill::Alchemy) > 0);
}

#[test]
fn nodes_grow_back() {
    let mut sim = Sim::new(3);
    let (patch, at) = node(&sim, "flax_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    gather_all(&mut sim, hero, patch, None);
    sim.command(hero, Command::Gather { node: patch });
    sim.step();
    assert!(!working(&sim, hero), "nothing to pick yet");
    let regrow = crafts().node("flax_patch").unwrap().regrow;
    sim.run_for(regrow);
    assert!(me(&mut sim, hero).nodes.iter().find(|n| n.id == patch).unwrap().ready);
    sim.command(hero, Command::Gather { node: patch });
    sim.step();
    assert!(working(&sim, hero), "it grew back");
}

/// Total marigolds from one patch, striking each swing at `strike` of the way through.
fn marigolds(seed: u64, strike: Option<f32>) -> u32 {
    let mut sim = Sim::new(seed);
    let (patch, at) = node(&sim, "marigold_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    gather_all(&mut sim, hero, patch, strike);
    count(&sim, hero, "marigold")
}

#[test]
fn a_sweet_spot_strike_gives_one_more_and_a_mistimed_one_nothing() {
    let uses = crafts().node("marigold_patch").unwrap().uses as u32;
    let middle = (SWEET_SPOT.0 + SWEET_SPOT.1) / 2.0;
    for seed in [4, 5, 6] {
        let plain = marigolds(seed, None);
        assert_eq!(marigolds(seed, Some(middle)), plain + uses, "seed {seed}");
        assert_eq!(marigolds(seed, Some(0.2)), plain, "too early: seed {seed}");
        assert_eq!(marigolds(seed, Some(0.95)), plain, "too late: seed {seed}");
    }
}

#[test]
fn only_the_first_strike_each_swing_counts() {
    let mut sim = Sim::new(7);
    let (patch, at) = node(&sim, "marigold_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    sim.command(hero, Command::Gather { node: patch });
    sim.step();
    sim.command(hero, Command::Strike);
    sim.step();
    let p = me(&mut sim, hero).work.unwrap();
    assert_eq!(p.struck, Some(false));
    // Mashing it in the sweet spot afterwards doesn't help.
    while me(&mut sim, hero).work.unwrap().elapsed < p.sweet_from + 0.05 {
        sim.step();
    }
    sim.command(hero, Command::Strike);
    sim.step();
    assert_eq!(me(&mut sim, hero).work.unwrap().struck, Some(false));
}

#[test]
fn gathering_takes_a_tool_and_the_level() {
    let mut sim = Sim::new(8);
    let (vein, at) = node(&sim, "copper_vein");
    let hero = worker(&mut sim, at, &[]);
    sim.command(hero, Command::Gather { node: vein });
    sim.step();
    assert!(!working(&sim, hero), "no pickaxe");
    give(&mut sim, hero, &[("pickaxe.copper", 1)]);
    sim.command(hero, Command::Gather { node: vein });
    sim.step();
    assert!(working(&sim, hero));

    let (iron, at) = node(&sim, "iron_vein");
    let miner = worker(&mut sim, at, &[("pickaxe.copper", 1), ("pickaxe.iron", 1)]);
    sim.command(miner, Command::Gather { node: iron });
    sim.step();
    assert!(!working(&sim, miner), "iron takes Mining 10");
    let messages: Vec<String> = sim.snapshot(None).events.into_iter().filter_map(|e| match e {
        Event::Message { text } => Some(text),
        _ => None,
    }).collect();
    assert!(messages.iter().any(|m| m == "You need Mining 10."), "{messages:?}");
}

#[test]
fn better_tools_swing_faster() {
    let swing = |tools: &[(&str, u16)], level: u8| {
        let mut sim = Sim::new(9);
        let (vein, at) = node(&sim, "copper_vein");
        let hero = worker(&mut sim, at, tools);
        sim.get_mut::<Skills>(hero).unwrap().levels[Skill::Mining.index()] = level;
        sim.command(hero, Command::Gather { node: vein });
        sim.step();
        me(&mut sim, hero).work.unwrap().duration
    };
    let both = [("pickaxe.copper", 1), ("pickaxe.iron", 1)];
    assert!((swing(&both, 1) - 2.4).abs() < 1e-4, "an iron pickaxe needs Mining 10");
    assert!((swing(&both, 10) - 2.4 / 1.4).abs() < 1e-4);
}

#[test]
fn moving_stops_the_work() {
    let mut sim = Sim::new(10);
    let (patch, at) = node(&sim, "marigold_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    sim.command(hero, Command::Gather { node: patch });
    sim.step();
    assert!(working(&sim, hero));
    sim.command(hero, Command::Move { direction: Vec2::X, walk: false, sprint: false, face_yaw: None });
    sim.run_for(0.5);
    assert!(!working(&sim, hero));
    assert_eq!(count(&sim, hero, "marigold"), 0, "the swing never finished");
}

#[test]
fn a_full_pack_stops_gathering() {
    let mut sim = Sim::new(11);
    let (patch, at) = node(&sim, "marigold_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    // Fill every other slot with a different tool.
    for _ in 1..sim::SLOTS {
        give(&mut sim, hero, &[("rod.fishing", 1)]);
    }
    sim.command(hero, Command::Gather { node: patch });
    sim.step();
    assert!(!working(&sim, hero));
}

#[test]
fn a_dead_wolf_can_be_skinned_once() {
    let mut sim = Sim::new(12);
    let w = sim.spawn_mob("wolf", Vec2::new(-20.0, 0.0)).unwrap();
    let hero = worker(&mut sim, Vec3::new(-20.0, 0.0, 0.0), &[("knife.skinning", 1)]);
    sim.command(hero, Command::Skin { target: w });
    sim.step();
    assert!(!working(&sim, hero), "it's alive");
    let we = sim.entity(w).unwrap();
    sim::combat::life::die(&mut sim.world, we, None);
    sim.step();
    let near = sim.get::<Transform>(w).unwrap().pos;
    teleport(&mut sim, hero, near.x + 1.0, near.z);
    sim.command(hero, Command::Skin { target: w });
    work_until_done(&mut sim, hero, 5.0);
    assert!(count(&sim, hero, "wolf_pelt") >= 1);
    assert_eq!(xp(&sim, hero, Skill::Skinning), 20);
    sim.command(hero, Command::Skin { target: w });
    sim.step();
    assert!(!working(&sim, hero), "already skinned");
    assert!(sim.get::<Brain>(w).is_some() && sim.get::<Unit>(w).is_some());
}

#[test]
fn a_wolf_can_be_skinned_straight_after_the_fight() {
    let mut sim = Sim::new(18);
    let w = sim.spawn_mob("wolf", Vec2::new(-20.0, 0.0)).unwrap();
    let hero = worker(&mut sim, Vec3::new(-20.0, 0.0, 0.0), &[("knife.skinning", 1)]);
    let we = sim.entity(w).unwrap();
    sim::combat::life::die(&mut sim.world, we, Some(hero));
    let t = sim.time();
    sim.get_mut::<Vitals>(hero).unwrap().last_combat = t;
    sim.step();
    let near = sim.get::<Transform>(w).unwrap().pos;
    teleport(&mut sim, hero, near.x + 1.0, near.z);
    sim.command(hero, Command::Skin { target: w });
    work_until_done(&mut sim, hero, 5.0);
    assert!(count(&sim, hero, "wolf_pelt") >= 1);
}

#[test]
fn being_hit_stops_the_work() {
    let mut sim = Sim::new(19);
    let (patch, at) = node(&sim, "marigold_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    sim.command(hero, Command::Gather { node: patch });
    sim.step();
    assert!(working(&sim, hero));
    let t = sim.time();
    sim.get_mut::<Vitals>(hero).unwrap().last_combat = t + 0.001;
    sim.step();
    assert!(!working(&sim, hero));
}

#[test]
fn no_gathering_in_a_fight() {
    let mut sim = Sim::new(13);
    let (patch, at) = node(&sim, "marigold_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    let t = sim.time();
    sim.get_mut::<Vitals>(hero).unwrap().last_combat = t;
    sim.command(hero, Command::Gather { node: patch });
    sim.step();
    assert!(!working(&sim, hero));
}

#[test]
fn recipes_take_the_level_and_the_station() {
    let mut sim = Sim::new(14);
    let anvil = station("brambleford_anvil");
    let hero = sim.spawn_player("Smith", Vec2::new(anvil.0 + 1.0, anvil.1), 0.0);
    give(&mut sim, hero, &[("copper_ingot", 4), ("oak_log", 1)]);
    sim.command(hero, Command::Craft { recipe: "sword.arming".into(), count: 1 });
    sim.step();
    assert!(!working(&sim, hero), "a sword takes Smithing 5");
    sim.command(hero, Command::Craft { recipe: "copper_ingot".into(), count: 1 });
    sim.step();
    assert!(!working(&sim, hero), "ingots are smelted at the forge");
    sim.get_mut::<Skills>(hero).unwrap().levels[Skill::Smithing.index()] = 5;
    sim.command(hero, Command::Craft { recipe: "sword.arming".into(), count: 1 });
    work_until_done(&mut sim, hero, 10.0);
    assert_eq!(count(&sim, hero, "sword.arming"), 1);
}

#[test]
fn skills_level_on_the_specialty_curve_to_fifty() {
    let mut s = Skills::default();
    assert_eq!(s.add(Skill::Fishing, 250), vec![2]);
    assert_eq!(s.level(Skill::Fishing), 2);
    assert_eq!(s.level(Skill::Mining), 1, "each skill levels on its own");
    s.add(Skill::Fishing, u32::MAX / 2);
    assert_eq!(s.level(Skill::Fishing), 50);
    assert!(s.add(Skill::Fishing, 1000).is_empty(), "nothing past the cap");
    // Easy work pays less, then nothing.
    assert_eq!(action_xp(20, 1, 6), 20);
    assert_eq!(action_xp(20, 1, 9), 8);
    assert_eq!(action_xp(20, 1, 11), 0);
}

#[test]
fn a_level_up_is_an_event() {
    let mut sim = Sim::new(15);
    let (patch, at) = node(&sim, "marigold_patch");
    let hero = worker(&mut sim, at, &[("sickle.copper", 1)]);
    sim.get_mut::<Skills>(hero).unwrap().xp[Skill::Herbalism.index()] = 249;
    sim.command(hero, Command::Gather { node: patch });
    let mut ups = Vec::new();
    for _ in 0..(3 * 60) {
        sim.step();
        ups.extend(sim.take_events().into_iter().filter(|e| matches!(e, Event::SkillUp { .. })));
    }
    assert_eq!(ups, vec![Event::SkillUp { unit: hero, skill: "herbalism".into(), level: 2 }]);
    assert_eq!(me(&mut sim, hero).skills[Skill::Herbalism.index()].level, 2);
}

#[test]
fn the_same_seed_and_commands_gather_the_same() {
    let run = |seed| {
        let mut sim = Sim::new(seed);
        let (vein, at) = node(&sim, "copper_vein");
        let hero = worker(&mut sim, at, &[("pickaxe.copper", 1)]);
        gather_all(&mut sim, hero, vein, Some(0.75));
        sim.get::<Inventory>(hero).unwrap().clone()
    };
    assert_eq!(run(16), run(16));
}

#[test]
fn every_node_and_station_can_be_reached() {
    let sim = Sim::new(17);
    let land = sim.land();
    let c = crafts();
    let spots = c
        .spawns
        .iter()
        .map(|s| (s.node, s.at, c.node(s.node).unwrap().reach))
        .chain(c.stations.iter().map(|s| (s.key, s.at, content::crafts::STATION_REACH)));
    for (what, (x, z), reach) in spots {
        let ok = (0..48).any(|i| {
            (1..=8).any(|k| {
                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                let p = Vec2::new(x, z) + Vec2::from_angle(a) * reach * k as f32 / 8.0 * 0.95;
                !land.blocked(p, 0.4) && land.water_depth(p.x, p.y) <= 0.0
            })
        });
        assert!(ok, "nowhere to stand within reach of {what} at ({x}, {z})");
    }
}

#[test]
fn skills_cross_a_region_border() {
    let mut west = Sim::new(20);
    let mut east = Sim::new(20);
    let hero = west.spawn_player("Wanderer", Vec2::new(0.0, -20.0), 0.0);
    west.get_mut::<Skills>(hero).unwrap().add(Skill::Mining, 600);
    let before = west.get::<Skills>(hero).unwrap().clone();
    let record = west.export(hero).unwrap();
    east.import(vec![record]);
    assert_eq!(east.get::<Skills>(hero), Some(&before));
    assert!(before.level(Skill::Mining) > 1);
}
