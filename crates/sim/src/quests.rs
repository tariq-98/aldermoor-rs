//! Quests and jobs (#70): the content crate's tables (`content::quests`) played out.
//!
//! A player's [`Quests`] holds the quests under way, each at one step with a count for each of
//! the step's objectives, the quests finished, and their jobs. Progress comes from what already
//! happens: kills (with a fair share of the fight, as for XP), gathering, talking to someone,
//! using an item. Nothing is checked each step except places to reach, and those only for
//! players with one to reach, every [`REACH_EVERY`] steps.
//!
//! Notice boards roll their jobs from their templates with their own dice, and roll new ones
//! every `JOB_ROTATION` seconds. Every player sees the same offers and may take each once.

use bevy_ecs::prelude::*;
use content::items::economy;
use content::quests::{quests, BoardDef, JobDef, JobKind, Objective, QuestDef, JOB_ROTATION, MAX_JOBS, TALK_REACH};
use glam::{Vec2, Vec3};
use protocol::{BoardView, Event, JobOffer, ObjectiveView, PersonView, QuestOffer, QuestView};

use crate::combat::{emit, tell};
use crate::loot::{Inventory, Stack};
use crate::{Brain, Clock, Credit, Land, Progress, Rng, Transform, Unit, Units};

/// How often players with a place to reach are checked, in steps.
pub const REACH_EVERY: u64 = 10;
/// How far away a player sees the people who give quests.
pub const PEOPLE_SEEN: f32 = 40.0;

/// A quest under way.
#[derive(Clone, Debug, PartialEq)]
pub struct Active {
    pub quest: &'static QuestDef,
    pub step: usize,
    /// How far along each of the step's objectives is.
    pub counts: Vec<u16>,
}

/// A job taken from a board.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub offer: u32,
    pub def: &'static JobDef,
    pub need: u16,
    /// Kills or gathered so far (a job to bring something counts the pack instead).
    pub have: u16,
}

/// A player's quests and jobs.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct Quests {
    pub active: Vec<Active>,
    pub done: Vec<&'static str>,
    pub jobs: Vec<Job>,
    /// Offers taken from the boards, while the boards still show them.
    taken: Vec<u32>,
}

impl Quests {
    pub fn finished(&self, key: &str) -> bool {
        self.done.contains(&key)
    }

    fn under_way(&self, key: &str) -> bool {
        self.active.iter().any(|a| a.quest.key == key)
    }
}

/// A job on a board.
#[derive(Clone, Copy, Debug)]
pub struct Offer {
    pub id: u32,
    pub def: &'static JobDef,
    pub need: u16,
}

pub struct Board {
    pub def: &'static BoardDef,
    pub offers: Vec<Offer>,
    /// When it next rolls new offers.
    next_at: f64,
}

/// Every notice board, and the dice for their jobs: their own sequence from the world's seed.
#[derive(Resource)]
pub struct Boards {
    pub list: Vec<Board>,
    next: u32,
    rng: Rng,
}

impl Boards {
    pub fn new(seed: u64) -> Boards {
        let mut b = Boards { list: Vec::new(), next: 0, rng: Rng::new(seed ^ 0x7175_6573) };
        for def in &quests().boards {
            let offers = b.roll(def);
            b.list.push(Board { def, offers, next_at: JOB_ROTATION });
        }
        b
    }

    fn roll(&mut self, def: &'static BoardDef) -> Vec<Offer> {
        let pool: Vec<&'static JobDef> = quests().jobs_for(def.key).collect();
        let mut out = Vec::new();
        for _ in 0..def.offers {
            let job = pool[(self.rng.next_u32() as usize) % pool.len()];
            let span = (job.count.1 - job.count.0) as u32 + 1;
            let need = job.count.0 + (self.rng.next_u32() % span) as u16;
            self.next += 1;
            out.push(Offer { id: self.next, def: job, need });
        }
        out
    }

    fn offer(&self, id: u32) -> Option<(&Board, Offer)> {
        self.list.iter().find_map(|b| b.offers.iter().find(|o| o.id == id).map(|o| (b, *o)))
    }
}

/// Players with a place to reach in the step under way.
#[derive(Resource, Default)]
pub(crate) struct Seekers(Vec<Entity>);

fn ground(world: &World, at: (f32, f32)) -> Vec3 {
    Vec3::new(at.0, world.resource::<Land>().0.ground(at.0, at.1), at.1)
}

fn near(p: Vec3, at: (f32, f32), reach: f32) -> bool {
    Vec2::new(p.x - at.0, p.z - at.1).length() <= reach
}

fn pos(world: &World, e: Entity) -> Option<Vec3> {
    world.get::<Transform>(e).map(|t| t.pos)
}

fn unit(world: &World, e: Entity) -> protocol::UnitId {
    world.get::<Unit>(e).map_or(0, |u| u.id)
}

fn item_name(key: &str) -> &str {
    economy().item(key).map_or(key, |d| d.name)
}

/// Gives items, dropping what doesn't fit in a pile at the player's feet that only they can take.
fn hand_over(world: &mut World, e: Entity, items: &[(&'static str, u16)]) {
    let eco = economy();
    let Some(mut inv) = world.get_mut::<Inventory>(e) else { return };
    let mut over = Vec::new();
    for &(key, n) in items {
        let def = eco.item(key).expect("quest items are checked on load");
        let left = inv.add(def, n, false);
        if left > 0 {
            over.push(Stack { item: def.key, count: left, fine: false });
        }
    }
    if over.is_empty() {
        return;
    }
    let (Some(at), id) = (pos(world, e), unit(world, e)) else { return };
    crate::loot::drop_for(world, id, at, over);
    tell(world, e, "Your pack is full: the rest is on the ground.");
}

/// `Command::StartQuest`: starts a quest from its giver, within reach, once what it needs is done.
pub(crate) fn start(world: &mut World, e: Entity, key: &str) {
    let q = quests();
    let Some(def) = q.quest(key) else { return };
    let giver = q.person(def.giver).expect("givers are checked on load");
    let Some(p) = pos(world, e) else { return };
    if !near(p, giver.at, TALK_REACH) {
        return tell(world, e, "That's too far away.");
    }
    let Some(log) = world.get::<Quests>(e) else { return };
    if log.under_way(key) || log.finished(key) || !def.needs.iter().all(|n| log.finished(n)) {
        return;
    }
    world.get_mut::<Quests>(e).unwrap().active.push(Active { quest: def, step: 0, counts: Vec::new() });
    emit(world, Event::QuestStarted { unit: unit(world, e), quest: def.key.to_string() });
    tell(world, e, &format!("Quest started: {}.", def.name));
    begin_step(world, e, def.key);
}

/// Starts the step a quest is at: its line, what's handed over, and the counts.
fn begin_step(world: &mut World, e: Entity, key: &str) {
    let Some(mut log) = world.get_mut::<Quests>(e) else { return };
    let Some(a) = log.active.iter_mut().find(|a| a.quest.key == key) else { return };
    let quest: &'static QuestDef = a.quest;
    let step = &quest.steps[a.step];
    a.counts = vec![0; step.objectives.len()];
    if !step.say.is_empty() {
        tell(world, e, step.say);
    }
    tell(world, e, step.text);
    hand_over(world, e, &step.gives);
    if step.objectives.iter().any(|o| matches!(o, Objective::Reach { .. })) {
        let mut seekers = world.resource_mut::<Seekers>();
        if !seekers.0.contains(&e) {
            seekers.0.push(e);
        }
    }
}

/// Moves a quest on if its step is done: to the next step, or to the reward.
fn check(world: &mut World, e: Entity, key: &'static str) {
    let Some(log) = world.get::<Quests>(e) else { return };
    let Some(a) = log.active.iter().find(|a| a.quest.key == key) else { return };
    let step = &a.quest.steps[a.step];
    if step.objectives.iter().zip(&a.counts).any(|(o, &n)| n < o.count()) {
        return;
    }
    let def = a.quest;
    let next = a.step + 1;
    let id = unit(world, e);
    if next < def.steps.len() {
        world.get_mut::<Quests>(e).unwrap().active.iter_mut().find(|a| a.quest.key == key).unwrap().step = next;
        emit(world, Event::QuestStep { unit: id, quest: key.to_string(), step: next as u8 });
        return begin_step(world, e, key);
    }
    let mut log = world.get_mut::<Quests>(e).unwrap();
    log.active.retain(|a| a.quest.key != key);
    log.done.push(key);
    let r = &def.reward;
    if let Some(mut inv) = world.get_mut::<Inventory>(e) {
        inv.coins += r.coins;
    }
    if let Some(mut p) = world.get_mut::<Progress>(e) {
        p.unassigned += r.xp;
    }
    hand_over(world, e, &r.items);
    emit(world, Event::QuestDone { unit: id, quest: key.to_string() });
    tell(world, e, &format!("Quest complete: {}! {} coins and {} quest XP.", def.name, r.coins, r.xp));
}

/// Counts progress on every quest step and job of a player that `hits` picks out, by how much,
/// then moves the quests on.
fn count(world: &mut World, e: Entity, hits: impl Fn(&Objective) -> Option<u16>, job: impl Fn(&JobKind) -> Option<u16>) {
    let Some(mut log) = world.get_mut::<Quests>(e) else { return };
    let mut moved = Vec::new();
    for a in &mut log.active {
        let step = &a.quest.steps[a.step];
        for (o, n) in step.objectives.iter().zip(a.counts.iter_mut()) {
            if let Some(by) = hits(o)
                && *n < o.count()
            {
                *n = (*n + by).min(o.count());
                moved.push(a.quest.key);
            }
        }
    }
    for j in &mut log.jobs {
        if let Some(by) = job(&j.def.kind)
            && j.have < j.need
        {
            j.have = (j.have + by).min(j.need);
            if j.have == j.need {
                moved.push("");
            }
        }
    }
    let jobs_ready = moved.contains(&"");
    moved.dedup();
    for key in moved.into_iter().filter(|k| !k.is_empty()) {
        check(world, e, key);
    }
    if jobs_ready {
        tell(world, e, "Job done: hand it in at the board.");
    }
}

/// A monster died: counts it for every player who earned a share of it. Before its credit is
/// paid out.
pub(crate) fn killed(world: &mut World, dead: Entity) {
    let Some(mob) = world.get::<Brain>(dead).map(|b| b.mob) else { return };
    let Some(earners) = world.get::<Credit>(dead).map(|c| c.earners()) else { return };
    for id in earners {
        let Some(p) = world.resource::<Units>().entity(id) else { continue };
        if world.get::<Quests>(p).is_none_or(|q| q.active.is_empty() && q.jobs.is_empty()) {
            continue;
        }
        count(
            world,
            p,
            |o| matches!(o, Objective::Kill { mob: m, .. } if *m == mob).then_some(1),
            |k| matches!(k, JobKind::Kill { mob: m } if *m == mob).then_some(1),
        );
    }
}

/// A player gathered `n` of an item.
pub(crate) fn gathered(world: &mut World, e: Entity, item: &str, n: u16) {
    if n == 0 || world.get::<Quests>(e).is_none_or(|q| q.active.is_empty() && q.jobs.is_empty()) {
        return;
    }
    count(
        world,
        e,
        |o| matches!(o, Objective::Gather { item: i, .. } if *i == item).then_some(n),
        |k| matches!(k, JobKind::Gather { item: i } if *i == item).then_some(n),
    );
}

/// `Command::Talk`: hands over what quests ask to be delivered to them, and counts the talk.
pub(crate) fn talk(world: &mut World, e: Entity, person: &str) {
    let q = quests();
    let Some(who) = q.person(person) else { return };
    let Some(p) = pos(world, e) else { return };
    if !near(p, who.at, TALK_REACH) {
        return tell(world, e, "That's too far away.");
    }
    let Some(log) = world.get::<Quests>(e).cloned() else { return };
    let mut moved = Vec::new();
    let mut short = None;
    for a in &log.active {
        for (i, o) in a.quest.steps[a.step].objectives.iter().enumerate() {
            if a.counts[i] >= o.count() {
                continue;
            }
            let done = match o {
                Objective::Talk { person: to } => *to == person,
                Objective::Deliver { item, count, to } if *to == person => {
                    let mut inv = world.get_mut::<Inventory>(e).unwrap();
                    let handed = inv.remove(item, *count);
                    if !handed {
                        short = Some(format!("{} wants {count} {}.", who.name, item_name(item)));
                    }
                    handed
                }
                _ => false,
            };
            if done {
                let mut log = world.get_mut::<Quests>(e).unwrap();
                let a = log.active.iter_mut().find(|x| x.quest.key == a.quest.key).unwrap();
                a.counts[i] = o.count();
                moved.push(a.quest.key);
            }
        }
    }
    if let Some(why) = &short {
        tell(world, e, why);
    }
    if moved.is_empty() && short.is_none() {
        tell(world, e, &format!("{}: \"{}\"", who.name, who.greeting));
    }
    moved.dedup();
    for key in moved {
        check(world, e, key);
    }
}

/// `Command::UseItem` on an item a quest asks to be used somewhere. Returns false if no quest
/// wants it, for it to be used as usual.
pub(crate) fn use_item(world: &mut World, e: Entity, slot: u8) -> bool {
    let Some(log) = world.get::<Quests>(e) else { return false };
    if log.active.is_empty() {
        return false;
    }
    let Some(Some(stack)) = world.get::<Inventory>(e).and_then(|i| i.slots.get(slot as usize).cloned()) else { return false };
    let Some(p) = pos(world, e) else { return false };
    let mut wanted = false;
    let mut found = None;
    for a in &log.active {
        for (i, o) in a.quest.steps[a.step].objectives.iter().enumerate() {
            if let Objective::Use { item, at, radius, .. } = o
                && *item == stack.item
                && a.counts[i] == 0
            {
                wanted = true;
                if near(p, *at, *radius) {
                    found = Some((a.quest.key, i));
                }
            }
        }
    }
    let Some((key, i)) = found else {
        if wanted {
            tell(world, e, "This isn't the place for that.");
        }
        return wanted;
    };
    world.get_mut::<Inventory>(e).unwrap().remove(stack.item, 1);
    let mut log = world.get_mut::<Quests>(e).unwrap();
    log.active.iter_mut().find(|a| a.quest.key == key).unwrap().counts[i] = 1;
    check(world, e, key);
    true
}

/// `Command::TakeJob`: takes an offer from the board the player stands at.
pub(crate) fn take_job(world: &mut World, e: Entity, offer: u32) {
    let Some(p) = pos(world, e) else { return };
    let boards = world.resource::<Boards>();
    let Some((board, o)) = boards.offer(offer) else { return };
    if !near(p, board.def.at, TALK_REACH) {
        return tell(world, e, "That's too far away.");
    }
    let showing: Vec<u32> = boards.list.iter().flat_map(|b| b.offers.iter().map(|o| o.id)).collect();
    let Some(mut log) = world.get_mut::<Quests>(e) else { return };
    log.taken.retain(|id| showing.contains(id));
    let why = if log.taken.contains(&offer) {
        "You've taken that job already."
    } else if log.jobs.len() >= MAX_JOBS {
        "You have as many jobs as you can take on."
    } else {
        log.taken.push(offer);
        log.jobs.push(Job { offer, def: o.def, need: o.need, have: 0 });
        ""
    };
    if !why.is_empty() {
        return tell(world, e, why);
    }
    tell(world, e, &format!("Job taken: {}.", o.def.name));
}

/// `Command::HandIn`: hands in a finished job at its board, with what it asks to be brought.
pub(crate) fn hand_in(world: &mut World, e: Entity, offer: u32) {
    let Some(p) = pos(world, e) else { return };
    let Some(job) = world.get::<Quests>(e).and_then(|q| q.jobs.iter().find(|j| j.offer == offer).cloned()) else { return };
    let Some(board) = quests().board(job.def.board) else { return };
    if !near(p, board.at, TALK_REACH) {
        return tell(world, e, "Hand it in at the board.");
    }
    let done = match job.def.kind {
        JobKind::Bring { item } => world.get_mut::<Inventory>(e).is_some_and(|mut inv| inv.remove(item, job.need)),
        JobKind::Kill { .. } | JobKind::Gather { .. } => job.have >= job.need,
    };
    if !done {
        return tell(world, e, "That job isn't done yet.");
    }
    let (coins, xp) = (job.def.pay.0 * job.need as u32, job.def.pay.1 * job.need as u32);
    world.get_mut::<Quests>(e).unwrap().jobs.retain(|j| j.offer != offer);
    if let Some(mut inv) = world.get_mut::<Inventory>(e) {
        inv.coins += coins;
    }
    if let Some(mut pr) = world.get_mut::<Progress>(e) {
        pr.unassigned += xp;
    }
    emit(world, Event::JobDone { unit: unit(world, e), job: job.def.key.to_string() });
    tell(world, e, &format!("Job done: {}. {coins} coins and {xp} quest XP.", job.def.name));
}

/// `Command::DropJob`.
pub(crate) fn drop_job(world: &mut World, e: Entity, offer: u32) {
    if let Some(mut log) = world.get_mut::<Quests>(e) {
        log.jobs.retain(|j| j.offer != offer);
    }
}

/// Whether a player has a place to reach in a step under way.
fn seeking(world: &World, e: Entity) -> bool {
    world.get::<Quests>(e).is_some_and(|q| {
        q.active.iter().any(|a| a.quest.steps[a.step].objectives.iter().zip(&a.counts).any(|(o, &n)| matches!(o, Objective::Reach { .. }) && n == 0))
    })
}

/// A player who crossed in from another region keeps looking for their places here.
pub(crate) fn arrived(world: &mut World, e: Entity) {
    if seeking(world, e) {
        let mut seekers = world.resource_mut::<Seekers>();
        if !seekers.0.contains(&e) {
            seekers.0.push(e);
        }
    }
}

/// Rolls boards' new offers when it's time, and checks the places players are looking for.
pub(crate) fn update(world: &mut World) {
    let Clock { tick, time } = *world.resource::<Clock>();
    let mut boards = world.resource_mut::<Boards>();
    for i in 0..boards.list.len() {
        if time >= boards.list[i].next_at {
            let def = boards.list[i].def;
            let offers = boards.roll(def);
            let b = &mut boards.list[i];
            b.offers = offers;
            b.next_at = time + JOB_ROTATION;
        }
    }
    if world.resource::<Seekers>().0.is_empty() || !tick.is_multiple_of(REACH_EVERY) {
        return;
    }
    let seekers = std::mem::take(&mut world.resource_mut::<Seekers>().0);
    let mut still = Vec::with_capacity(seekers.len());
    for e in seekers {
        let Some(p) = pos(world, e) else { continue };
        let reached = |o: &Objective| matches!(o, Objective::Reach { at, radius, .. } if near(p, *at, *radius)).then_some(1);
        count(world, e, reached, |_| None);
        if seeking(world, e) && !still.contains(&e) {
            still.push(e);
        }
    }
    // Steps that began while checking added their seekers already.
    let mut seekers = world.resource_mut::<Seekers>();
    for e in still {
        if !seekers.0.contains(&e) {
            seekers.0.push(e);
        }
    }
}

fn objective_view(world: &World, o: &Objective, have: u16) -> ObjectiveView {
    let q = quests();
    let person = |key: &str| q.person(key).expect("people are checked on load");
    let (text, at) = match o {
        Objective::Talk { person: p } => (format!("Talk to {}", person(p).name), Some(person(p).at)),
        Objective::Kill { mob, .. } => (format!("Kill {}", crate::content::catalog().mob(mob).map_or(*mob, |m| m.name)), None),
        Objective::Gather { item, .. } => (format!("Gather {}", item_name(item)), None),
        Objective::Deliver { item, to, .. } => (format!("Bring {} to {}", item_name(item), person(to).name), Some(person(to).at)),
        Objective::Reach { place, at, .. } => (format!("Go to {place}"), Some(*at)),
        Objective::Use { item, place, at, .. } => (format!("Use the {} at {place}", item_name(item)), Some(*at)),
    };
    ObjectiveView { text, have, need: o.count(), at: at.map(|a| ground(world, a)) }
}

/// The player's quest log, the people near them and the board they stand at: (quests, done,
/// people, board).
pub(crate) fn player_view(world: &World, e: Entity) -> (Vec<QuestView>, Vec<String>, Vec<PersonView>, Option<BoardView>) {
    let q = quests();
    let empty = Quests::default();
    let log = world.get::<Quests>(e).unwrap_or(&empty);
    let inv = world.get::<Inventory>(e);
    let mut list: Vec<QuestView> = log
        .active
        .iter()
        .map(|a| {
            let step = &a.quest.steps[a.step];
            QuestView {
                quest: a.quest.key.to_string(),
                name: a.quest.name.to_string(),
                job: None,
                step: a.step as u8,
                steps: a.quest.steps.len() as u8,
                text: step.text.to_string(),
                objectives: step.objectives.iter().zip(&a.counts).map(|(o, &n)| objective_view(world, o, n)).collect(),
                ready: false,
            }
        })
        .collect();
    for j in &log.jobs {
        let board = q.board(j.def.board).map(|b| ground(world, b.at));
        let (text, have) = match j.def.kind {
            JobKind::Kill { mob } => (format!("Kill {}", crate::content::catalog().mob(mob).map_or(mob, |m| m.name)), j.have),
            JobKind::Gather { item } => (format!("Gather {}", item_name(item)), j.have),
            JobKind::Bring { item } => (format!("Bring {} to the board", item_name(item)), inv.map_or(0, |i| i.count(item).min(j.need as u32) as u16)),
        };
        let at = matches!(j.def.kind, JobKind::Bring { .. }).then_some(board).flatten();
        list.push(QuestView {
            quest: j.def.key.to_string(),
            name: j.def.name.to_string(),
            job: Some(j.offer),
            step: 0,
            steps: 1,
            text: j.def.notice(j.need),
            objectives: vec![ObjectiveView { text, have, need: j.need, at }],
            ready: have >= j.need,
        });
    }
    let done = log.done.iter().map(|k| k.to_string()).collect();
    let Some(p) = pos(world, e) else { return (list, done, Vec::new(), None) };
    let wanted = |key: &str| {
        log.active.iter().any(|a| {
            a.quest.steps[a.step].objectives.iter().zip(&a.counts).any(|(o, &n)| {
                n < o.count() && matches!(o, Objective::Talk { person } | Objective::Deliver { to: person, .. } if *person == key)
            })
        })
    };
    let people = q
        .people
        .iter()
        .filter(|w| near(p, w.at, PEOPLE_SEEN))
        .map(|w| PersonView {
            person: w.key.to_string(),
            name: w.name.to_string(),
            position: ground(world, w.at),
            offers: q
                .quests
                .iter()
                .filter(|d| d.giver == w.key && !log.under_way(d.key) && !log.finished(d.key) && d.needs.iter().all(|n| log.finished(n)))
                .map(|d| QuestOffer { quest: d.key.to_string(), name: d.name.to_string(), text: d.offer.to_string(), level: d.level })
                .collect(),
            wanted: wanted(w.key),
        })
        .collect();
    let board = world.resource::<Boards>().list.iter().find(|b| near(p, b.def.at, TALK_REACH)).map(|b| BoardView {
        board: b.def.key.to_string(),
        name: b.def.name.to_string(),
        jobs: b
            .offers
            .iter()
            .map(|o| JobOffer {
                offer: o.id,
                name: o.def.name.to_string(),
                text: o.def.notice(o.need),
                level: o.def.level,
                coins: o.def.pay.0 * o.need as u32,
                xp: o.def.pay.1 * o.need as u32,
                taken: log.taken.contains(&o.id),
            })
            .collect(),
    });
    (list, done, people, board)
}
