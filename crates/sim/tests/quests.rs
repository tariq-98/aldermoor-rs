//! Quests and jobs (#70): one big quest and the jobs walked from start to finish, and what
//! stops them.

use content::items::economy;
use content::quests::{quests, JobKind, JOB_ROTATION, MAX_JOBS};
use glam::{Vec2, Vec3};
use protocol::wire::{Decoder, Encoder};
use protocol::{Command, Event, PlayerState};
use sim::combat::life;
use sim::quests::{Boards, Quests};
use sim::{Credit, Inventory, Nodes, Progress, Sim, Specialty, Transform, Unit};

fn at(key: &str) -> (f32, f32) {
    let q = quests();
    q.person(key).map(|p| p.at).or_else(|| q.board(key).map(|b| b.at)).unwrap()
}

/// Puts a player somewhere at once, a metre from `to`.
fn teleport(sim: &mut Sim, id: u32, to: (f32, f32)) {
    let (x, z) = (to.0 + 1.0, to.1);
    let y = sim.land().ground(x, z);
    sim.get_mut::<Transform>(id).unwrap().pos = Vec3::new(x, y, z);
}

fn me(sim: &mut Sim, id: u32) -> PlayerState {
    sim.snapshot(Some(id)).player.unwrap()
}

/// Steps once after a command, collecting the events.
fn send(sim: &mut Sim, id: u32, c: Command, events: &mut Vec<Event>) {
    sim.command(id, c);
    sim.step();
    events.extend(sim.take_events());
}

fn give(sim: &mut Sim, id: u32, item: &str, n: u16) {
    assert_eq!(sim.get_mut::<Inventory>(id).unwrap().add(economy().item(item).unwrap(), n, false), 0);
}

fn count(sim: &Sim, id: u32, item: &str) -> u32 {
    sim.get::<Inventory>(id).unwrap().count(item)
}

fn slot_of(sim: &Sim, id: u32, item: &str) -> u8 {
    sim.get::<Inventory>(id).unwrap().slots.iter().position(|s| s.as_ref().is_some_and(|s| s.item == item)).unwrap() as u8
}

/// Kills a monster with all the credit `hero`'s.
fn kill(sim: &mut Sim, hero: u32, mob: u32) {
    sim.get_mut::<Credit>(mob).unwrap().0 = vec![(hero, Specialty::Warrior, 50.0)];
    let e = sim.entity(mob).unwrap();
    life::die(&mut sim.world, e, None);
}

/// The living monsters of a kind, by ID.
fn mobs(sim: &Sim, mob: &str) -> Vec<u32> {
    let mut q = sim.world.try_query::<(&Unit, &sim::Vitals, &sim::Brain)>().unwrap();
    let mut ids: Vec<u32> = q.iter(&sim.world).filter(|(_, v, b)| b.mob == mob && !v.dead).map(|(u, ..)| u.id).collect();
    ids.sort();
    ids
}

fn step_of(sim: &Sim, id: u32, quest: &str) -> Option<usize> {
    sim.get::<Quests>(id).unwrap().active.iter().find(|a| a.quest.key == quest).map(|a| a.step)
}

fn said(events: &[Event], what: &str) -> bool {
    events.iter().any(|e| matches!(e, Event::Message { text } if text.contains(what)))
}

#[test]
fn smoke_over_the_meadow_from_start_to_finish() {
    let mut sim = Sim::brambleford(7);
    let hero = sim.spawn_player("Hero", Vec2::new(at("maren").0 + 1.0, at("maren").1), 0.0);
    let mut events = Vec::new();
    let p = me(&mut sim, hero);
    let maren = p.people.iter().find(|w| w.person == "maren").expect("Maren is in view");
    assert_eq!(maren.offers.iter().map(|o| o.quest.as_str()).collect::<Vec<_>>(), ["smoke_over_the_meadow"]);

    send(&mut sim, hero, Command::StartQuest { quest: "smoke_over_the_meadow".into() }, &mut events);
    assert!(events.contains(&Event::QuestStarted { unit: hero, quest: "smoke_over_the_meadow".into() }));
    let log = me(&mut sim, hero).quests;
    assert_eq!((log[0].step, log[0].steps, log[0].objectives[0].text.as_str()), (0, 5, "Go to the north meadow"));
    assert!(me(&mut sim, hero).people.iter().all(|w| w.offers.is_empty()), "offered a quest under way");

    // 1: the meadow.
    teleport(&mut sim, hero, (-38.0, 24.0));
    sim.run_for(0.5);
    events.extend(sim.take_events());
    assert_eq!(step_of(&sim, hero, "smoke_over_the_meadow"), Some(1));

    // 2: three wolves.
    let wolves = mobs(&sim, "wolf");
    assert!(wolves.len() >= 3);
    for &w in &wolves[..2] {
        kill(&mut sim, hero, w);
    }
    assert_eq!(me(&mut sim, hero).quests[0].objectives[0].have, 2);
    kill(&mut sim, hero, wolves[2]);
    assert_eq!(step_of(&sim, hero, "smoke_over_the_meadow"), Some(2));

    // 3: two pelts for Tobin, who wants them all at once.
    teleport(&mut sim, hero, at("tobin"));
    give(&mut sim, hero, "wolf_pelt", 1);
    send(&mut sim, hero, Command::Talk { person: "tobin".into() }, &mut events);
    assert!(said(&events, "wants 2 Wolf Pelt"));
    assert_eq!(step_of(&sim, hero, "smoke_over_the_meadow"), Some(2));
    give(&mut sim, hero, "wolf_pelt", 2);
    send(&mut sim, hero, Command::Talk { person: "tobin".into() }, &mut events);
    assert_eq!(count(&sim, hero, "wolf_pelt"), 1, "took more than asked");
    assert_eq!(step_of(&sim, hero, "smoke_over_the_meadow"), Some(3));
    assert_eq!(count(&sim, hero, "marigold_smoke"), 1, "Tobin didn't hand over the smoke");

    // 4: the smoke does nothing away from the cairn, and burns there.
    let smoke = slot_of(&sim, hero, "marigold_smoke");
    send(&mut sim, hero, Command::UseItem { slot: smoke }, &mut events);
    assert!(said(&events, "isn't the place"));
    assert_eq!(count(&sim, hero, "marigold_smoke"), 1);
    teleport(&mut sim, hero, (-46.0, 41.0));
    send(&mut sim, hero, Command::UseItem { slot: smoke }, &mut events);
    assert_eq!(count(&sim, hero, "marigold_smoke"), 0);
    assert_eq!(step_of(&sim, hero, "smoke_over_the_meadow"), Some(4));

    // 5: back to Maren, from too far away first.
    send(&mut sim, hero, Command::Talk { person: "maren".into() }, &mut events);
    assert!(said(&events, "too far"));
    teleport(&mut sim, hero, at("maren"));
    let (coins, xp) = (sim.get::<Inventory>(hero).unwrap().coins, sim.get::<Progress>(hero).unwrap().unassigned);
    send(&mut sim, hero, Command::Talk { person: "maren".into() }, &mut events);
    assert!(events.contains(&Event::QuestDone { unit: hero, quest: "smoke_over_the_meadow".into() }));
    let steps: Vec<u8> = events.iter().filter_map(|e| if let Event::QuestStep { step, .. } = e { Some(*step) } else { None }).collect();
    assert_eq!(steps, [1, 2, 3, 4]);
    assert_eq!(sim.get::<Inventory>(hero).unwrap().coins, coins + 120);
    assert_eq!(sim.get::<Progress>(hero).unwrap().unassigned, xp + 1200);
    assert_eq!(count(&sim, hero, "leather_jerkin"), 1);
    let p = me(&mut sim, hero);
    assert!(p.quests.is_empty());
    assert_eq!(p.quests_done, ["smoke_over_the_meadow"]);
    // Done for good: Maren offers it no more, and starting it again does nothing.
    send(&mut sim, hero, Command::StartQuest { quest: "smoke_over_the_meadow".into() }, &mut events);
    assert!(sim.get::<Quests>(hero).unwrap().active.is_empty());
}

#[test]
fn the_smiths_ore_waits_for_the_meadow_and_counts_what_you_mine() {
    let mut sim = Sim::brambleford(7);
    let hero = sim.spawn_player("Hero", Vec2::new(at("hilde").0 + 1.0, at("hilde").1), 0.0);
    let mut events = Vec::new();
    let hilde = |p: &PlayerState| p.people.iter().find(|w| w.person == "hilde").unwrap().offers.len();
    assert_eq!(hilde(&me(&mut sim, hero)), 0, "offered before the meadow was done");
    send(&mut sim, hero, Command::StartQuest { quest: "the_smiths_ore".into() }, &mut events);
    assert!(sim.get::<Quests>(hero).unwrap().active.is_empty());

    sim.get_mut::<Quests>(hero).unwrap().done.push("smoke_over_the_meadow");
    assert_eq!(hilde(&me(&mut sim, hero)), 1);
    send(&mut sim, hero, Command::StartQuest { quest: "the_smiths_ore".into() }, &mut events);
    assert_eq!(step_of(&sim, hero, "the_smiths_ore"), Some(0));

    // Both goblins and the archer, in any order; a wolf counts for nothing.
    let wolf = mobs(&sim, "wolf")[0];
    kill(&mut sim, hero, wolf);
    for g in mobs(&sim, "goblin") {
        kill(&mut sim, hero, g);
    }
    assert_eq!(step_of(&sim, hero, "the_smiths_ore"), Some(0));
    let archer = mobs(&sim, "goblin_archer")[0];
    kill(&mut sim, hero, archer);
    assert_eq!(step_of(&sim, hero, "the_smiths_ore"), Some(1));

    // Ore in the pack from before doesn't count: only what's mined.
    give(&mut sim, hero, "copper_ore", 10);
    give(&mut sim, hero, "pickaxe.copper", 1);
    let veins: Vec<(u32, Vec3)> = sim.world.resource::<Nodes>().list.iter().filter(|n| n.def.key == "copper_vein").map(|n| (n.id, n.at)).collect();
    for (vein, at) in veins {
        teleport(&mut sim, hero, (at.x + 0.5, at.z));
        sim.command(hero, Command::Gather { node: vein });
        for _ in 0..60 * 60 {
            sim.step();
            if sim.get::<sim::Work>(hero).is_none() {
                break;
            }
        }
        if step_of(&sim, hero, "the_smiths_ore") != Some(1) {
            break;
        }
    }
    assert_eq!(step_of(&sim, hero, "the_smiths_ore"), Some(2), "mining didn't count");

    teleport(&mut sim, hero, at("hilde"));
    let ore = count(&sim, hero, "copper_ore");
    send(&mut sim, hero, Command::Talk { person: "hilde".into() }, &mut events);
    assert_eq!(count(&sim, hero, "copper_ore"), ore - 4);
    assert!(sim.get::<Quests>(hero).unwrap().finished("the_smiths_ore"));
    assert_eq!(count(&sim, hero, "shield.kite"), 1);
}

#[test]
fn a_full_pack_leaves_the_reward_on_the_ground() {
    let mut sim = Sim::brambleford(7);
    let hero = sim.spawn_player("Hero", Vec2::new(at("hilde").0 + 1.0, at("hilde").1), 0.0);
    let mut events = Vec::new();
    sim.get_mut::<Quests>(hero).unwrap().done.push("smoke_over_the_meadow");
    send(&mut sim, hero, Command::StartQuest { quest: "the_smiths_ore".into() }, &mut events);
    // At the last step, with five ore (so a stack stays after handing over four) and the
    // rest of the pack full of gear that doesn't stack.
    {
        let mut log = sim.get_mut::<Quests>(hero).unwrap();
        log.active[0].step = 2;
        log.active[0].counts = vec![0];
    }
    give(&mut sim, hero, "copper_ore", 5);
    let free = sim.get::<Inventory>(hero).unwrap().slots.iter().filter(|s| s.is_none()).count();
    for _ in 0..free {
        give(&mut sim, hero, "padded_coat", 1);
    }
    send(&mut sim, hero, Command::Talk { person: "hilde".into() }, &mut events);
    assert!(sim.get::<Quests>(hero).unwrap().finished("the_smiths_ore"));
    assert_eq!(count(&sim, hero, "shield.kite"), 0);
    let p = me(&mut sim, hero);
    assert!(p.loot.iter().any(|l| l.items.iter().any(|i| i.item == "shield.kite")), "the shield isn't on the ground");
}

/// A seed whose board offers a job of the kind `want` picks out, and that offer.
fn board_with(want: impl Fn(&JobKind) -> bool) -> (Sim, u32, u16, &'static str) {
    for seed in 0..64 {
        let sim = Sim::brambleford(seed);
        let found = sim.world.resource::<Boards>().list[0].offers.iter().find(|o| want(&o.def.kind)).map(|o| (o.id, o.need, o.def.key));
        if let Some((id, need, key)) = found {
            return (sim, id, need, key);
        }
    }
    panic!("no seed offers that job");
}

#[test]
fn a_kill_job_from_the_board_pays_for_each_kill() {
    let (mut sim, offer, need, key) = board_with(|k| matches!(k, JobKind::Kill { mob: "wolf" }));
    let board = at("brambleford_board");
    let hero = sim.spawn_player("Hero", Vec2::new(board.0 + 1.0, board.1), 0.0);
    let mut events = Vec::new();
    let b = me(&mut sim, hero).board.expect("standing at the board");
    let o = b.jobs.iter().find(|j| j.offer == offer).unwrap().clone();
    send(&mut sim, hero, Command::TakeJob { offer }, &mut events);
    assert!(me(&mut sim, hero).board.unwrap().jobs.iter().find(|j| j.offer == offer).unwrap().taken);
    send(&mut sim, hero, Command::HandIn { offer }, &mut events);
    assert!(said(&events, "isn't done"));

    // Spawn wolves enough and kill them.
    for i in 0..need {
        let w = sim.spawn_mob("wolf", Vec2::new(-30.0 + i as f32 * 3.0, 20.0)).unwrap();
        kill(&mut sim, hero, w);
    }
    let log = me(&mut sim, hero).quests;
    assert!(log[0].ready && log[0].job == Some(offer));
    let coins = sim.get::<Inventory>(hero).unwrap().coins;
    send(&mut sim, hero, Command::HandIn { offer }, &mut events);
    assert!(events.contains(&Event::JobDone { unit: hero, job: key.into() }));
    assert_eq!(sim.get::<Inventory>(hero).unwrap().coins, coins + o.coins);
    assert!(me(&mut sim, hero).quests.is_empty());
    // Once a rotation: the same offer can't be taken twice.
    send(&mut sim, hero, Command::TakeJob { offer }, &mut events);
    assert!(said(&events, "already"));
}

#[test]
fn a_bring_job_takes_what_it_asks_for_at_the_board() {
    let (mut sim, offer, need, _) = board_with(|k| matches!(k, JobKind::Bring { .. }));
    let item = match sim.world.resource::<Boards>().list[0].offers.iter().find(|o| o.id == offer).unwrap().def.kind {
        JobKind::Bring { item } => item,
        _ => unreachable!(),
    };
    let board = at("brambleford_board");
    let hero = sim.spawn_player("Hero", Vec2::new(board.0 + 1.0, board.1), 0.0);
    let mut events = Vec::new();
    send(&mut sim, hero, Command::TakeJob { offer }, &mut events);
    give(&mut sim, hero, item, need - 1);
    assert!(!me(&mut sim, hero).quests[0].ready);
    send(&mut sim, hero, Command::HandIn { offer }, &mut events);
    assert_eq!(count(&sim, hero, item), (need - 1) as u32, "took some of too few");
    give(&mut sim, hero, item, 2);
    assert!(me(&mut sim, hero).quests[0].ready);
    teleport(&mut sim, hero, at("maren"));
    send(&mut sim, hero, Command::HandIn { offer }, &mut events);
    assert!(said(&events, "at the board"));
    teleport(&mut sim, hero, board);
    send(&mut sim, hero, Command::HandIn { offer }, &mut events);
    assert_eq!(count(&sim, hero, item), 1);
    assert!(sim.get::<Quests>(hero).unwrap().jobs.is_empty());
}

#[test]
fn boards_offer_new_jobs_each_rotation_and_jobs_are_capped() {
    let mut sim = Sim::brambleford(5);
    let board = at("brambleford_board");
    let hero = sim.spawn_player("Hero", Vec2::new(board.0 + 1.0, board.1), 0.0);
    let mut events = Vec::new();
    let first: Vec<u32> = me(&mut sim, hero).board.unwrap().jobs.iter().map(|j| j.offer).collect();
    assert_eq!(first.len(), quests().boards[0].offers as usize);
    for &o in &first {
        send(&mut sim, hero, Command::TakeJob { offer: o }, &mut events);
    }
    assert_eq!(sim.get::<Quests>(hero).unwrap().jobs.len(), MAX_JOBS.min(first.len()));

    sim.run_for(JOB_ROTATION as f32 + 1.0);
    let second: Vec<u32> = me(&mut sim, hero).board.unwrap().jobs.iter().map(|j| j.offer).collect();
    assert!(second.iter().all(|o| !first.contains(o)), "the board didn't change");
    send(&mut sim, hero, Command::TakeJob { offer: second[0] }, &mut events);
    assert!(said(&events, "as many jobs"));
    send(&mut sim, hero, Command::DropJob { offer: first[0] }, &mut events);
    send(&mut sim, hero, Command::TakeJob { offer: second[0] }, &mut events);
    assert!(sim.get::<Quests>(hero).unwrap().jobs.iter().any(|j| j.offer == second[0]));
}

#[test]
fn the_same_seed_rolls_the_same_jobs() {
    let offers = |seed| -> Vec<(&'static str, u16)> { Sim::brambleford(seed).world.resource::<Boards>().list[0].offers.iter().map(|o| (o.def.key, o.need)).collect() };
    assert_eq!(offers(11), offers(11));
}

#[test]
fn people_and_boards_stand_on_open_dry_ground() {
    let sim = Sim::brambleford(1);
    let q = quests();
    for (what, (x, z)) in q.people.iter().map(|p| (p.key, p.at)).chain(q.boards.iter().map(|b| (b.key, b.at))) {
        assert!(!sim.land().blocked(Vec2::new(x, z), 0.5), "{what} stands in a wall");
        assert_eq!(sim.land().water_depth(x, z), 0.0, "{what} stands in the river");
    }
    // And the places quests send you to.
    for quest in &q.quests {
        for step in &quest.steps {
            for o in &step.objectives {
                if let content::quests::Objective::Reach { at, .. } | content::quests::Objective::Use { at, .. } = o {
                    assert_eq!(sim.land().water_depth(at.0, at.1), 0.0, "{} sends you into the river", quest.key);
                }
            }
        }
    }
}

#[test]
fn the_quest_log_goes_over_the_wire() {
    let mut sim = Sim::brambleford(7);
    let hero = sim.spawn_player("Hero", Vec2::new(at("maren").0 + 1.0, at("maren").1), 0.0);
    sim.command(hero, Command::StartQuest { quest: "smoke_over_the_meadow".into() });
    sim.step();
    let (mut encoder, mut decoder) = (Encoder::new(), Decoder::new());
    let snapshot = sim.snapshot(Some(hero));
    let view = encoder.encode(&snapshot, &sim.combat_view(Some(hero)));
    let got = decoder.decode(view).unwrap();
    let (a, b) = (snapshot.player.unwrap(), got.0.player.unwrap());
    assert_eq!((a.quests, a.quests_done, a.people, a.board), (b.quests, b.quests_done, b.people, b.board));
}
