//! Gathering and crafting skills (#68, docs/CRAFTING.md).
//!
//! Every skill levels on its own through use, on the specialties' curve, to the same cap. A
//! player works one thing at a time: swings at a node (with a tool for the skill from their
//! pack) until it runs out, the pack fills or they move, or makes a recipe some number of times
//! at a station. Nodes don't tick: one that ran out notes when it grows back, and is full again
//! from then on. The tables are the content crate's (`content::crafts`).

use bevy_ecs::prelude::*;
use content::crafts::{crafts, NodeDef, RecipeDef, Skill, StationDef, ToolDef, MAX_SKILL_LEVEL, STATION_REACH, SWEET_SPOT};
use content::items::economy;
use glam::{Vec2, Vec3};
use protocol::{Event, ItemView, NodeView, RecipeView, SkillView, StationView, UnitId, WorkView};

use crate::combat::{emit, now, tell};
use crate::content::progression::{to_next, FULL_BELOW, GREY};
use crate::loot::Inventory;
use crate::{Brain, Fighter, Motion, Rng, Transform, Unit, Units, Vitals};

/// How far away a player sees gathering nodes.
pub const NODE_SEEN: f32 = 40.0;
/// Moving further than this from where you started stops the work.
const STAY_PUT: f32 = 0.3;

/// A player's level and XP in every gathering and crafting skill.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Skills {
    pub levels: [u8; 11],
    /// XP into the current level, toward the next.
    pub xp: [u32; 11],
}

impl Default for Skills {
    fn default() -> Self {
        Skills { levels: [1; 11], xp: [0; 11] }
    }
}

impl Skills {
    pub fn level(&self, s: Skill) -> u8 {
        self.levels[s.index()]
    }

    /// Adds XP to a skill. Returns the levels it rose through (none at the cap).
    pub fn add(&mut self, s: Skill, amount: u32) -> Vec<u8> {
        let i = s.index();
        let mut ups = Vec::new();
        let mut left = amount;
        while self.levels[i] < MAX_SKILL_LEVEL {
            let Some(need) = to_next(self.levels[i]) else { break };
            let room = need - self.xp[i];
            if left < room {
                self.xp[i] += left;
                return ups;
            }
            left -= room;
            self.xp[i] = 0;
            self.levels[i] += 1;
            ups.push(self.levels[i]);
        }
        ups
    }
}

/// What an action pays a skill at `level`: full up to `FULL_BELOW` levels over the task's,
/// nothing from `GREY` over, as for kills.
pub fn action_xp(base: u32, task_level: u8, level: u8) -> u32 {
    let over = level.saturating_sub(task_level);
    if over <= FULL_BELOW {
        base
    } else if over >= GREY {
        0
    } else {
        base * (GREY - over) as u32 / (GREY - FULL_BELOW) as u32
    }
}

/// A node in the world.
#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub def: &'static NodeDef,
    pub at: Vec3,
    left: u8,
    /// When one that ran out is full again.
    back_at: f64,
}

impl Node {
    /// Swings left at `t`.
    pub fn left(&self, t: f64) -> u8 {
        if self.left == 0 && t >= self.back_at {
            self.def.uses
        } else {
            self.left
        }
    }
}

/// Every gathering node, and the dice for what they give: their own sequence from the world's
/// seed, so gathering doesn't shift the fight's rolls.
#[derive(Resource)]
pub struct Nodes {
    pub list: Vec<Node>,
    rng: Rng,
}

impl Nodes {
    /// The craft tables' nodes, standing on `land`.
    pub fn new(seed: u64, land: &world::Valley) -> Nodes {
        let c = crafts();
        let list = c
            .spawns
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let def = c.node(s.node).expect("spawns are checked on load");
                let at = Vec3::new(s.at.0, land.ground(s.at.0, s.at.1), s.at.1);
                Node { id: i as u32 + 1, def, at, left: def.uses, back_at: 0.0 }
            })
            .collect();
        Nodes { list, rng: Rng::new(seed ^ 0x6761_7468) }
    }
}

/// Players at work, so a step with nobody working costs nothing.
#[derive(Resource, Default)]
pub(crate) struct Workers(Vec<Entity>);

/// The last time a carcass was skinned: its death then.
#[derive(Component, Clone, Copy, Debug)]
pub struct Skinned(f64);

#[derive(Clone, Copy, Debug)]
enum Job {
    Node(u32),
    Carcass(UnitId),
    Recipe(&'static RecipeDef),
}

/// What a player is working on.
#[derive(Component, Clone, Copy, Debug)]
pub struct Work {
    job: Job,
    skill: Skill,
    /// When this swing or item began, and how long it takes.
    start: f64,
    duration: f64,
    /// Whether this swing's strike landed in the sweet spot (`None` before any strike).
    struck: Option<bool>,
    /// Where they stood, and when they began.
    from: Vec3,
    started: f64,
    /// More to make after this one.
    left: u16,
}

fn flat(d: Vec3) -> f32 {
    Vec2::new(d.x, d.z).length()
}

/// The fastest tool for `skill` in the pack that the player has the level for.
fn best_tool(inv: &Inventory, skill: Skill, level: u8) -> Option<&'static ToolDef> {
    crafts()
        .tools
        .iter()
        .filter(|t| t.skill == skill && t.level <= level && inv.count(t.item) > 0)
        .max_by(|a, b| a.speed.total_cmp(&b.speed))
}

/// The tool a beginner uses, to say what's missing.
fn first_tool(skill: Skill) -> &'static str {
    let t = crafts().tools.iter().find(|t| t.skill == skill && t.level == 1).expect("every gathering skill has a beginner's tool");
    economy().item(t.item).map_or(t.item, |d| d.name)
}

fn level_of(world: &World, e: Entity, s: Skill) -> u8 {
    world.get::<Skills>(e).map_or(1, |k| k.level(s))
}

/// Whether the player is free to work: alive, up, dry, not casting, and out of a fight (or,
/// for skinning what they just killed, at least not in one since `since`).
fn free(world: &World, e: Entity, t: f64, since: Option<f64>) -> Result<(), &'static str> {
    let v = world.get::<Vitals>(e).ok_or("")?;
    if v.dead || v.downed {
        return Err("");
    }
    let fighting = match since {
        Some(start) => v.last_combat > start,
        None => t - v.last_combat < crate::COMBAT_TIMEOUT,
    };
    if fighting {
        return Err("You can't do that in a fight.");
    }
    if world.get::<Motion>(e).is_some_and(|m| m.swimming) {
        return Err("You can't do that while swimming.");
    }
    if world.get::<Fighter>(e).is_some_and(|f| f.cast.is_some()) {
        return Err("");
    }
    Ok(())
}

/// Checks a gathering node or carcass can be worked: the level, a tool, room in the pack. The
/// swing's length with the tool, or what to tell the player.
fn ready_to_gather(world: &World, e: Entity, def: &NodeDef) -> Result<f64, String> {
    let level = level_of(world, e, def.skill);
    if level < def.level {
        return Err(format!("You need {} {}.", def.skill.name(), def.level));
    }
    let inv = world.get::<Inventory>(e).ok_or_else(String::new)?;
    let Some(tool) = best_tool(inv, def.skill, level) else {
        return Err(format!("You need a {} or better.", first_tool(def.skill)));
    };
    let item = economy().item(def.item).expect("node items are checked on load");
    if !inv.fits(item, 1, false) {
        return Err("Your pack is full.".to_string());
    }
    Ok((def.swing / tool.speed) as f64)
}

fn start(world: &mut World, e: Entity, job: Job, skill: Skill, duration: f64, left: u16) {
    let t = now(world);
    let Some(from) = world.get::<Transform>(e).map(|tr| tr.pos) else { return };
    world.entity_mut(e).insert(Work { job, skill, start: t, duration, struck: None, from, started: t, left });
    let mut workers = world.resource_mut::<Workers>();
    if !workers.0.contains(&e) {
        workers.0.push(e);
    }
}

fn refuse(world: &mut World, e: Entity, why: &str) {
    if !why.is_empty() {
        tell(world, e, why);
    }
}

/// `Command::Gather`: starts swinging at a node within reach.
pub(crate) fn gather(world: &mut World, e: Entity, node: u32) {
    let t = now(world);
    if let Err(why) = free(world, e, t, None) {
        return refuse(world, e, why);
    }
    let Some(p) = world.get::<Transform>(e).map(|tr| tr.pos) else { return };
    let Some(n) = world.resource::<Nodes>().list.iter().find(|n| n.id == node).cloned() else { return };
    if flat(n.at - p) > n.def.reach {
        return refuse(world, e, "That's too far away.");
    }
    if n.left(t) == 0 {
        return refuse(world, e, &format!("The {} has run out for now.", n.def.name.to_lowercase()));
    }
    match ready_to_gather(world, e, n.def) {
        Ok(swing) => start(world, e, Job::Node(node), n.def.skill, swing, 0),
        Err(why) => refuse(world, e, &why),
    }
}

/// The node a dead beast can be skinned as, if it's dead and not yet skinned.
fn carcass(world: &World, target: Entity) -> Option<&'static NodeDef> {
    let v = world.get::<Vitals>(target)?;
    if !v.dead || world.get::<Skinned>(target).is_some_and(|s| s.0 == v.died_at) {
        return None;
    }
    crafts().carcass(world.get::<Brain>(target)?.mob)
}

/// `Command::Skin`: starts skinning a dead beast within reach.
pub(crate) fn skin(world: &mut World, e: Entity, target: UnitId) {
    let t = now(world);
    // The wolf you just killed can be skinned at once.
    if let Err(why) = free(world, e, t, Some(t)) {
        return refuse(world, e, why);
    }
    let Some(te) = world.resource::<Units>().entity(target) else { return };
    let (Some(p), Some(q)) = (world.get::<Transform>(e).map(|tr| tr.pos), world.get::<Transform>(te).map(|tr| tr.pos)) else { return };
    let Some(def) = carcass(world, te) else {
        return refuse(world, e, "There's nothing to skin.");
    };
    if flat(q - p) > def.reach {
        return refuse(world, e, "That's too far away.");
    }
    match ready_to_gather(world, e, def) {
        Ok(swing) => start(world, e, Job::Carcass(target), def.skill, swing, 0),
        Err(why) => refuse(world, e, &why),
    }
}

/// The station of `kind` the player stands at.
fn station_at(p: Vec3, kind: Option<&str>) -> Option<&'static StationDef> {
    crafts()
        .stations
        .iter()
        .find(|s| kind.is_none_or(|k| s.kind == k) && Vec2::new(p.x - s.at.0, p.z - s.at.1).length() <= STATION_REACH)
}

/// How many of a recipe the pack holds the inputs for.
fn makeable(inv: &Inventory, r: &RecipeDef) -> u16 {
    r.inputs.iter().map(|(item, n)| inv.count(item) / *n as u32).min().unwrap_or(0).min(u16::MAX as u32) as u16
}

/// `Command::Craft`: starts making `count` of a recipe at a station within reach.
pub(crate) fn craft(world: &mut World, e: Entity, recipe: &str, count: u16) {
    let t = now(world);
    if let Err(why) = free(world, e, t, None) {
        return refuse(world, e, why);
    }
    let Some(r) = crafts().recipe(recipe) else { return };
    let Some(p) = world.get::<Transform>(e).map(|tr| tr.pos) else { return };
    if count == 0 {
        return;
    }
    if station_at(p, Some(r.station)).is_none() {
        return refuse(world, e, "You need to be at the right station.");
    }
    if level_of(world, e, r.skill) < r.level {
        return refuse(world, e, &format!("You need {} {}.", r.skill.name(), r.level));
    }
    if world.get::<Inventory>(e).is_none_or(|inv| makeable(inv, r) == 0) {
        return refuse(world, e, "You don't have the materials.");
    }
    start(world, e, Job::Recipe(r), r.skill, r.seconds as f64, count - 1);
}

/// `Command::Strike`: the first strike each swing lands in the sweet spot or misses it.
pub(crate) fn strike(world: &mut World, e: Entity) {
    let t = now(world);
    let Some(mut w) = world.get_mut::<Work>(e) else { return };
    if matches!(w.job, Job::Recipe(_)) || w.struck.is_some() {
        return;
    }
    let at = ((t - w.start) / w.duration) as f32;
    w.struck = Some((SWEET_SPOT.0..=SWEET_SPOT.1).contains(&at));
}

/// `Command::StopWork`, and anything else that ends the work.
pub(crate) fn stop(world: &mut World, e: Entity) {
    if world.entity_mut(e).take::<Work>().is_some() {
        world.resource_mut::<Workers>().0.retain(|&w| w != e);
    }
}

/// Gives XP to a skill, with the level-up event and message.
fn give(world: &mut World, e: Entity, skill: Skill, amount: u32) {
    if amount == 0 {
        return;
    }
    let Some(ups) = world.get_mut::<Skills>(e).map(|mut s| s.add(skill, amount)) else { return };
    let Some(&level) = ups.last() else { return };
    let unit = world.get::<Unit>(e).map_or(0, |u| u.id);
    for &l in &ups {
        emit(world, Event::SkillUp { unit, skill: skill.key().to_string(), level: l });
    }
    tell(world, e, &format!("Your {} reached level {level}.", skill.name()));
}

/// Moves the work on: ends it if the player moved, fell into a fight or started casting, and
/// finishes each swing or item whose time is up.
pub(crate) fn update(world: &mut World) {
    if world.resource::<Workers>().0.is_empty() {
        return;
    }
    let t = now(world);
    let workers = world.resource::<Workers>().0.clone();
    for e in workers {
        let Some(w) = world.get::<Work>(e).copied() else {
            world.resource_mut::<Workers>().0.retain(|&x| x != e);
            continue;
        };
        let moved = world.get::<Transform>(e).is_none_or(|tr| flat(tr.pos - w.from) > STAY_PUT);
        // Anything that hits them (or that they hit) after they started ends it.
        if moved || free(world, e, t, Some(w.started)).is_err() {
            stop(world, e);
            continue;
        }
        if t >= w.start + w.duration {
            finish(world, e, w, t);
        }
    }
}

fn finish(world: &mut World, e: Entity, w: Work, t: f64) {
    match w.job {
        Job::Node(id) => {
            let Some(n) = world.resource::<Nodes>().list.iter().find(|n| n.id == id).cloned() else { return stop(world, e) };
            if n.left(t) == 0 {
                tell(world, e, &format!("The {} has run out for now.", n.def.name.to_lowercase()));
                return stop(world, e);
            }
            let level = level_of(world, e, n.def.skill);
            if !yield_from(world, e, n.def, w.struck == Some(true), level) {
                return stop(world, e);
            }
            let mut nodes = world.resource_mut::<Nodes>();
            let node = nodes.list.iter_mut().find(|n| n.id == id).expect("found above");
            node.left = node.left(t) - 1;
            let ran_out = node.left == 0;
            if ran_out {
                node.back_at = t + node.def.regrow as f64;
                tell(world, e, &format!("The {} has run out for now.", n.def.name.to_lowercase()));
                return stop(world, e);
            }
            next_swing(world, e, n.def, t);
        }
        Job::Carcass(target) => {
            let Some(te) = world.resource::<Units>().entity(target) else { return stop(world, e) };
            if let Some(def) = carcass(world, te) {
                let level = level_of(world, e, def.skill);
                if yield_from(world, e, def, w.struck == Some(true), level) {
                    let died = world.get::<Vitals>(te).map_or(0.0, |v| v.died_at);
                    world.entity_mut(te).insert(Skinned(died));
                }
            }
            stop(world, e);
        }
        Job::Recipe(r) => {
            let made = make(world, e, r);
            let at_station = world.get::<Transform>(e).is_some_and(|tr| station_at(tr.pos, Some(r.station)).is_some());
            let more = made && w.left > 0 && at_station && world.get::<Inventory>(e).is_some_and(|inv| makeable(inv, r) > 0);
            match world.get_mut::<Work>(e) {
                Some(mut w) if more => {
                    w.left -= 1;
                    w.start = t;
                }
                _ => stop(world, e),
            }
        }
    }
}

/// Starts the next swing at a node, if the player still has a tool and room.
fn next_swing(world: &mut World, e: Entity, def: &NodeDef, t: f64) {
    match ready_to_gather(world, e, def) {
        Ok(swing) => {
            if let Some(mut w) = world.get_mut::<Work>(e) {
                w.start = t;
                w.duration = swing;
                w.struck = None;
            }
        }
        Err(why) => {
            refuse(world, e, &why);
            stop(world, e);
        }
    }
}

/// One swing's take: the yield, one more for a sweet-spot strike and a chance at the rare
/// find. Returns false if none of it fit.
fn yield_from(world: &mut World, e: Entity, def: &NodeDef, sweet: bool, level: u8) -> bool {
    let eco = economy();
    let (count, rare) = {
        let mut nodes = world.resource_mut::<Nodes>();
        let span = (def.gives.1 - def.gives.0) as u32 + 1;
        let mut count = def.gives.0 + (nodes.rng.next_u32() % span) as u16;
        let mut rare = None;
        if sweet {
            count += 1;
            if let Some((item, chance)) = def.rare
                && nodes.rng.chance(chance)
            {
                rare = Some(item);
            }
        }
        (count, rare)
    };
    let item = eco.item(def.item).expect("node items are checked on load");
    let Some(mut inv) = world.get_mut::<Inventory>(e) else { return false };
    let over = inv.add(item, count, false);
    if let Some(r) = rare.and_then(|r| eco.item(r)) {
        inv.add(r, 1, false);
    }
    if over == count {
        tell(world, e, "Your pack is full.");
        return false;
    }
    if let Some(r) = rare.and_then(|r| eco.item(r)) {
        tell(world, e, &format!("You found a {}!", r.name));
    }
    give(world, e, def.skill, action_xp(def.xp, def.level, level));
    true
}

/// Makes one of a recipe from the pack. Returns false if the inputs aren't there or the
/// output wouldn't fit.
fn make(world: &mut World, e: Entity, r: &RecipeDef) -> bool {
    let eco = economy();
    let out = eco.item(r.output.0).expect("recipe outputs are checked on load");
    let level = level_of(world, e, r.skill);
    let Some(mut inv) = world.get_mut::<Inventory>(e) else { return false };
    if makeable(&inv, r) == 0 {
        tell(world, e, "You don't have the materials.");
        return false;
    }
    let mut after = inv.clone();
    for (item, n) in &r.inputs {
        after.remove(item, *n);
    }
    if after.add(out, r.output.1, false) > 0 {
        tell(world, e, "Your pack is full.");
        return false;
    }
    *inv = after;
    give(world, e, r.skill, action_xp(r.xp, r.level, level));
    true
}

fn item_view(key: &str, count: u16) -> ItemView {
    let name = economy().item(key).map_or(key, |d| d.name);
    ItemView { item: key.to_string(), name: name.to_string(), count, fine: false, price: 0 }
}

/// The player's skills, their work, the nodes nearby and the station they stand at.
pub(crate) fn player_view(world: &World, e: Entity) -> (Vec<SkillView>, Option<WorkView>, Vec<NodeView>, Option<StationView>) {
    let t = now(world);
    let skills = world.get::<Skills>(e).cloned().unwrap_or_default();
    let skill_views = Skill::ALL
        .iter()
        .map(|s| SkillView {
            level: skills.level(*s),
            xp: skills.xp[s.index()],
            xp_next: if skills.level(*s) < MAX_SKILL_LEVEL { to_next(skills.level(*s)).unwrap_or(0) } else { 0 },
        })
        .collect();
    let Some(p) = world.get::<Transform>(e).map(|tr| tr.pos) else { return (skill_views, None, Vec::new(), None) };
    let nodes = world.resource::<Nodes>();
    let work = world.get::<Work>(e).map(|w| {
        let (name, left) = match w.job {
            Job::Node(id) => (nodes.list.iter().find(|n| n.id == id).map_or("", |n| n.def.name), 0),
            Job::Carcass(_) => ("Carcass", 0),
            Job::Recipe(r) => (economy().item(r.output.0).map_or(r.key, |d| d.name), w.left),
        };
        let sweet = !matches!(w.job, Job::Recipe(_));
        WorkView {
            skill: w.skill.key().to_string(),
            name: name.to_string(),
            elapsed: (t - w.start).max(0.0) as f32,
            duration: w.duration as f32,
            sweet_from: if sweet { w.duration as f32 * SWEET_SPOT.0 } else { 0.0 },
            sweet_to: if sweet { w.duration as f32 * SWEET_SPOT.1 } else { 0.0 },
            struck: w.struck,
            left,
        }
    });
    let near = nodes
        .list
        .iter()
        .filter(|n| flat(n.at - p) <= NODE_SEEN)
        .map(|n| NodeView { id: n.id, node: n.def.key.to_string(), name: n.def.name.to_string(), position: n.at, level: n.def.level, ready: n.left(t) > 0, reach: n.def.reach })
        .collect();
    let station = station_at(p, None).map(|s| {
        let inv = world.get::<Inventory>(e);
        StationView {
            station: s.key.to_string(),
            name: s.name.to_string(),
            recipes: crafts()
                .recipes
                .iter()
                .filter(|r| r.station == s.kind)
                .map(|r| RecipeView {
                    recipe: r.key.to_string(),
                    skill: r.skill.key().to_string(),
                    level: r.level,
                    inputs: r.inputs.iter().map(|(i, n)| item_view(i, *n)).collect(),
                    output: item_view(r.output.0, r.output.1),
                    can_make: if skills.level(r.skill) >= r.level { inv.map_or(0, |i| makeable(i, r)) } else { 0 },
                })
                .collect(),
        }
    });
    (skill_views, work, near, station)
}
