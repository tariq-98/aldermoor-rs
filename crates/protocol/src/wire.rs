//! Views on the wire (#34): what a server sends a player over the network in place of a whole
//! [`Snapshot`] and [`CombatView`] each step, and how the player rebuilds them.
//!
//! - **Appearance once.** A unit's kind, body, name and look go in an [`Intro`] when it comes
//!   into view or changes, and never again while it stays.
//! - **Deltas.** Each view says only what changed since a *base*: the last view the player
//!   acknowledged (`Ack`), so the same scheme works over a transport that drops messages.
//! - **Quantised.** Positions to the centimetre, velocity to 5 cm/s, yaw to 1/65536 of a turn,
//!   health to a tenth of a point, endurance and resources to whole points, areas to 10 cm.
//! - **Far units less often** (#278): those within 30 m of the player go out in every view they
//!   change, then every second view out to 60 m, every fourth to 100 m, every eighth beyond.
//! - **Timers as ticks.** A cooldown, aura, cast or pose goes out as the tick it ends or began,
//!   so it doesn't change from view to view while it runs.
//!
//! An [`Encoder`] per connection on the server and a [`Decoder`] on the player's side keep the
//! same record of each view, so a delta always applies to exactly what the other side has.

use crate::*;
use std::collections::VecDeque;

/// Steps a second, to turn seconds into ticks.
const TICKS: f32 = 60.0;
/// Timers count in ticks from the start of the current minute, so they fit an `f32` exactly.
/// (Each minute every running timer is sent once more.)
const MINUTE: u64 = 3600;
/// Views kept waiting for an ack, about a second and a half at 20 Hz.
const KEPT: usize = 32;
/// Units within this many metres of the player go out in every view that they change...
const NEAR: f32 = 30.0;
/// ...within this many in every second view...
const MIDDLE: f32 = 60.0;
/// ...within this many in every fourth, and further out in every eighth (#278).
const FAR: f32 = 100.0;

/// How long both sides remember a unit's intro after it leaves view, so one wandering along the
/// edge of view doesn't send its look again each time it comes back. Ten seconds.
const REMEMBER: u64 = 600;

const CM: f32 = 100.0;
/// Velocity in 5 cm/s steps.
const VEL: f32 = 20.0;
const YAW: f32 = 65536.0 / std::f32::consts::TAU;
const HEALTH: f32 = 10.0;
/// Areas to the 10 cm.
const AREA: f32 = 10.0;

/// One view on the wire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WireView {
    pub tick: u64,
    pub time: f64,
    /// The view this one is a delta against, or `None` for one that starts from nothing.
    pub base: Option<u64>,
    /// Units new to view, or whose look, name or body changed.
    pub intros: Vec<Intro>,
    /// Units new or changed since the base, packed (see `write_units`).
    pub units: Vec<u8>,
    /// Units in the base no longer in view.
    pub gone: Vec<UnitId>,
    pub player: PlayerDelta,
    pub combat: CombatDelta,
    /// Everything that happened since the last view sent.
    pub events: Vec<Event>,
}

/// What doesn't change about a unit from step to step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Intro {
    pub id: UnitId,
    pub kind: UnitKind,
    pub body: String,
    pub name: String,
    pub look: Arc<Look>,
}

/// The player's own numbers, in parts that change at different rates. A part is `None` when
/// it's the same as in the base.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerDelta {
    /// No player state at all (`Snapshot::player` is `None`).
    pub none: bool,
    pub fast: Option<Fast>,
    pub bar: Option<Bar>,
    pub bags: Option<Bags>,
    pub craft: Option<Craft>,
    pub growth: Option<Growth>,
}

/// What changes most: the bars and flags.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Fast {
    pub unit: UnitId,
    pub endurance: u32,
    pub max_endurance: f32,
    pub resource_kind: ResourceKind,
    pub resource: u32,
    pub max_resource: f32,
    pub auto_attack: bool,
    pub in_combat: bool,
    pub can_respawn: bool,
    pub guarding: bool,
    pub downed: bool,
    pub swimming: bool,
    pub sprinting: bool,
    pub weapon_set: u8,
}

/// The skill bar and what's running on the player. Timers are ticks from the anchor
/// (see `countdown` and `count_up`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bar {
    pub cooldowns: Vec<f32>,
    pub bar: Vec<Slot>,
    pub gcd: f32,
    pub swap_ready: f32,
    pub potion_ready: f32,
    pub cast: Option<CastBar>,
    pub auras: Vec<AuraView>,
}

/// Coins, bags, loot and the shop at hand.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Bags {
    pub coins: u32,
    pub inventory: Vec<ItemView>,
    pub loot: Vec<LootView>,
    pub shop: Option<ShopView>,
}

/// Gathering and crafting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Craft {
    pub skills: Vec<SkillView>,
    pub work: Option<WorkView>,
    pub nodes: Vec<NodeView>,
    pub station: Option<StationView>,
}

/// Specialty, level and XP.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Growth {
    pub specialty: String,
    pub level: u8,
    pub xp: u32,
    pub xp_next: u32,
    pub unassigned_xp: u32,
    pub specialties: Vec<SpecialtyLevel>,
}

/// Combat detail that changed since the base. Timers are ticks from the anchor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CombatDelta {
    pub units: Vec<UnitCombat>,
    pub gone: Vec<UnitId>,
    /// Every projectile in flight: few, and always moving.
    pub projectiles: Vec<ProjectileView>,
    pub areas: Vec<AreaView>,
    pub areas_gone: Vec<u32>,
}

#[derive(Debug)]
pub enum WireError {
    /// The view is a delta against one this side no longer has.
    UnknownBase(u64),
    /// A unit's delta names one with no intro.
    NoIntro(UnitId),
    Malformed(String),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            WireError::UnknownBase(t) => write!(f, "a delta against view {t}, which isn't kept"),
            WireError::NoIntro(id) => write!(f, "unit {id} without an intro"),
            WireError::Malformed(why) => write!(f, "a malformed view: {why}"),
        }
    }
}

impl std::error::Error for WireError {}

/// A unit as quantised for the wire.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Quantised {
    level: u8,
    pos: [i32; 3],
    vel: [i32; 3],
    yaw: u16,
    pose: Pose,
    /// When the pose began, in milliseconds of sim time.
    since: i64,
    traversal: Traversal,
    health: u32,
    max_health: f32,
    hostile: bool,
    target: Option<UnitId>,
}

/// Poses are sent as when they began, which drifts a little against `f32` pose times.
const SINCE_SLACK: i64 = 2;

/// A map kept as a vector in ID order: cheaper than a tree to build afresh every view.
#[derive(Clone, Debug)]
struct Sorted<V>(Vec<(u32, V)>);

impl<V> Default for Sorted<V> {
    fn default() -> Self {
        Sorted(Vec::new())
    }
}

impl State {
    /// The intro either side already has for `id`: in view in this state, or seen lately.
    fn intro(&self, id: UnitId) -> Option<&Arc<Intro>> {
        self.units.get(id).map(|(_, i)| i).or_else(|| self.left.get(id).map(|(i, _)| i))
    }

    /// What's remembered after this state, once `now` is in view at `tick`.
    fn remembered(&self, now: &Sorted<(Quantised, Arc<Intro>)>, tick: u64) -> Sorted<(Arc<Intro>, u64)> {
        let mut left: Vec<(u32, (Arc<Intro>, u64))> = self.left.0.iter().filter(|(id, (_, seen))| tick.saturating_sub(*seen) <= REMEMBER && !now.contains(*id)).cloned().collect();
        left.extend(self.units.0.iter().filter(|(id, _)| !now.contains(*id)).map(|(id, (_, i))| (*id, (i.clone(), self.tick))));
        Sorted::new(left)
    }
}

impl<V: Clone> Sorted<V> {
    fn new(mut entries: Vec<(u32, V)>) -> Sorted<V> {
        if !entries.is_sorted_by_key(|e| e.0) {
            entries.sort_by_key(|e| e.0);
        }
        Sorted(entries)
    }

    fn get(&self, id: u32) -> Option<&V> {
        self.0.binary_search_by_key(&id, |e| e.0).ok().map(|i| &self.0[i].1)
    }

    fn contains(&self, id: u32) -> bool {
        self.get(id).is_some()
    }

    /// The IDs here but not in `now`.
    fn gone(&self, now: &Sorted<V>) -> Vec<u32> {
        self.0.iter().map(|e| e.0).filter(|&id| !now.contains(id)).collect()
    }

    /// This without `gone`, and with `changes` (in ID order) put in.
    fn patched(&self, gone: &[u32], changes: Vec<(u32, V)>) -> Sorted<V> {
        let mut out = Vec::with_capacity(self.0.len() + changes.len());
        let mut changes = changes.into_iter().peekable();
        for (id, v) in &self.0 {
            while let Some(c) = changes.next_if(|c| c.0 < *id) {
                out.push(c);
            }
            match changes.next_if(|c| c.0 == *id) {
                Some(c) => out.push(c),
                None if gone.contains(id) => {}
                None => out.push((*id, v.clone())),
            }
        }
        out.extend(changes);
        Sorted(out)
    }
}

/// One view as both sides keep it.
#[derive(Clone, Debug, Default)]
struct State {
    tick: u64,
    units: Sorted<(Quantised, Arc<Intro>)>,
    /// Intros of units that left view lately, and the tick each was last seen.
    left: Sorted<(Arc<Intro>, u64)>,
    player: Option<(Fast, Bar, Bags, Craft, Growth)>,
    combat: Sorted<Arc<UnitCombat>>,
    areas: Sorted<AreaView>,
}

/// A connection's side: turns each view into a delta against the last one acknowledged.
#[derive(Default)]
pub struct Encoder {
    sent: VecDeque<State>,
    acked: Option<u64>,
    /// Views encoded, to take turns at sending far units.
    views: u64,
}

impl Encoder {
    pub fn new() -> Encoder {
        Encoder::default()
    }

    /// The player has view `tick`: later views are deltas against it.
    pub fn ack(&mut self, tick: u64) {
        if self.sent.iter().any(|s| s.tick == tick) && self.acked.is_none_or(|a| tick > a) {
            self.acked = Some(tick);
            self.sent.retain(|s| s.tick >= tick);
        }
    }

    /// The view to send for `snapshot` and `combat`, with `snapshot.events` as its events.
    pub fn encode(&mut self, snapshot: &Snapshot, combat: &CombatView) -> WireView {
        let base = self.acked.and_then(|a| self.sent.iter().find(|s| s.tick == a));
        let empty = State::default();
        let was = base.unwrap_or(&empty);
        let (tick, time) = (snapshot.tick, snapshot.time);
        let now_ms = (time * 1000.0).round() as i64;
        let mut intros = Vec::new();
        let mut changed = Vec::new();
        let mut units = Vec::with_capacity(snapshot.units.len());
        let me = snapshot.player.as_ref().and_then(|p| snapshot.units.iter().find(|u| u.id == p.unit)).map(|u| u.position);
        let latest = if base.is_some() { self.sent.back() } else { None };
        let turn = self.views;
        self.views += 1;
        for u in &snapshot.units {
            let before = was.units.get(u.id);
            let mut q = quantise(u, now_ms);
            if let Some((b, _)) = before {
                if (q.since - b.since).abs() <= SINCE_SLACK {
                    q.since = b.since;
                }
                // A far unit that changed waits its turn, as it was in the base.
                let every = match me.map(|m| m.distance(u.position)) {
                    Some(d) if d >= FAR => 8,
                    Some(d) if d >= MIDDLE => 4,
                    Some(d) if d >= NEAR => 2,
                    _ => 1,
                };
                // Off its turn, it stays as the last view had it: what the base has, or a newer
                // turn's value the player hasn't acked yet (resent, so it doesn't jump back).
                if (turn + u.id as u64) % every != 0 {
                    q = latest.and_then(|l| l.units.get(u.id)).map_or(*b, |(l, _)| *l);
                }
            }
            let intro = match before.map(|(_, i)| i).or_else(|| was.left.get(u.id).map(|(i, _)| i)) {
                Some(i) if i.kind == u.kind && i.body == u.body && i.name == u.name && (Arc::ptr_eq(&i.look, &u.look) || i.look == u.look) => i.clone(),
                _ => {
                    let i = Arc::new(Intro { id: u.id, kind: u.kind, body: u.body.clone(), name: u.name.clone(), look: u.look.clone() });
                    intros.push((*i).clone());
                    i
                }
            };
            if before.is_none_or(|(b, _)| *b != q) {
                changed.push((u.id, q, before.map(|(b, _)| *b)));
            }
            units.push((u.id, (q, intro)));
        }
        let mut state = State { tick, units: Sorted::new(units), ..Default::default() };
        state.left = was.remembered(&state.units, tick);
        let gone = was.units.gone(&state.units);
        changed.sort_unstable_by_key(|c| c.0);
        intros.sort_unstable_by_key(|i| i.id);
        let mut packed = Vec::new();
        write_units(&mut packed, &changed, now_ms);

        let anchor = tick % MINUTE;
        let mut player = PlayerDelta::default();
        match &snapshot.player {
            None => player.none = true,
            Some(p) => {
                let parts = split(p, anchor);
                let old = was.player.as_ref();
                player.fast = old.is_none_or(|o| o.0 != parts.0).then(|| parts.0.clone());
                player.bar = old.is_none_or(|o| o.1 != parts.1).then(|| parts.1.clone());
                player.bags = old.is_none_or(|o| o.2 != parts.2).then(|| parts.2.clone());
                player.craft = old.is_none_or(|o| o.3 != parts.3).then(|| parts.3.clone());
                player.growth = old.is_none_or(|o| o.4 != parts.4).then(|| parts.4.clone());
                state.player = Some(parts);
            }
        }

        let mut delta = CombatDelta { projectiles: combat.projectiles.clone(), ..Default::default() };
        let mut list = Vec::with_capacity(combat.units.len());
        for c in &combat.units {
            let before = was.combat.get(c.id);
            debug_assert!(before.is_none_or(|b| combat_matches(c, b, anchor) == (combat_out(c, anchor) == **b)));
            match before {
                // Compared as it would go out, without building it.
                Some(b) if combat_matches(c, b, anchor) => list.push((c.id, b.clone())),
                _ => {
                    let c = Arc::new(combat_out(c, anchor));
                    delta.units.push((*c).clone());
                    list.push((c.id, c));
                }
            }
        }
        state.combat = Sorted::new(list);
        delta.gone = was.combat.gone(&state.combat);
        let mut list = Vec::with_capacity(combat.areas.len());
        for a in &combat.areas {
            // A warning follows its caster by centimetres a step: to the 10 cm, it mostly stays put.
            let a = AreaView { elapsed: count_up(a.elapsed, anchor), center: (a.center * AREA).round() / AREA, ..a.clone() };
            if was.areas.get(a.id) != Some(&a) {
                delta.areas.push(a.clone());
            }
            list.push((a.id, a));
        }
        state.areas = Sorted::new(list);
        delta.areas_gone = was.areas.gone(&state.areas);
        delta.units.sort_unstable_by_key(|u| u.id);
        delta.areas.sort_unstable_by_key(|a| a.id);

        let view = WireView { tick, time, base: base.map(|b| b.tick), intros, units: packed, gone, player, combat: delta, events: snapshot.events.clone() };
        self.sent.push_back(state);
        while self.sent.len() > KEPT {
            self.sent.pop_front();
        }
        view
    }
}

/// The player's side: rebuilds each view from its base. Send `Ack(view.tick)` once decoded.
/// Units and combat detail come back in ID order.
#[derive(Default)]
pub struct Decoder {
    kept: VecDeque<State>,
}

impl Decoder {
    pub fn new() -> Decoder {
        Decoder::default()
    }

    pub fn decode(&mut self, view: WireView) -> Result<(Snapshot, CombatView), WireError> {
        let empty = State::default();
        let was = match view.base {
            None => &empty,
            Some(b) => self.kept.iter().find(|s| s.tick == b).ok_or(WireError::UnknownBase(b))?,
        };
        let (tick, time) = (view.tick, view.time);
        let now_ms = (time * 1000.0).round() as i64;
        let intros = Sorted::new(view.intros.into_iter().map(|i| (i.id, Arc::new(i))).collect());
        let mut changes = Vec::new();
        for (id, q) in read_units(&view.units, &was.units, now_ms)? {
            let intro = intros.get(id).or_else(|| was.intro(id)).ok_or(WireError::NoIntro(id))?;
            changes.push((id, (q, intro.clone())));
        }
        let mut state = State { tick, units: was.units.patched(&view.gone, changes), ..Default::default() };
        state.left = was.remembered(&state.units, tick);
        for (id, i) in &intros.0 {
            // A new look for a unit that didn't otherwise change.
            let at = state.units.0.binary_search_by_key(id, |e| e.0).map_err(|_| WireError::NoIntro(*id))?;
            state.units.0[at].1.1 = i.clone();
        }
        let units = state.units.0.iter().map(|(id, (q, i))| unit(*id, q, i, now_ms)).collect();

        let anchor = tick % MINUTE;
        let p = view.player;
        let player = if p.none {
            None
        } else {
            let old = was.player.as_ref();
            let missing = || WireError::Malformed("a player part with no base".into());
            let parts = (
                p.fast.or_else(|| old.map(|o| o.0.clone())).ok_or_else(missing)?,
                p.bar.or_else(|| old.map(|o| o.1.clone())).ok_or_else(missing)?,
                p.bags.or_else(|| old.map(|o| o.2.clone())).ok_or_else(missing)?,
                p.craft.or_else(|| old.map(|o| o.3.clone())).ok_or_else(missing)?,
                p.growth.or_else(|| old.map(|o| o.4.clone())).ok_or_else(missing)?,
            );
            let player = join(&parts, anchor);
            state.player = Some(parts);
            Some(player)
        };

        let c = view.combat;
        state.combat = was.combat.patched(&c.gone, c.units.into_iter().map(|u| (u.id, Arc::new(u))).collect());
        state.areas = was.areas.patched(&c.areas_gone, c.areas.into_iter().map(|a| (a.id, a)).collect());
        let combat = CombatView {
            tick,
            units: state.combat.0.iter().map(|(_, u)| combat_in(u, anchor)).collect(),
            projectiles: c.projectiles,
            areas: state.areas.0.iter().map(|(_, a)| AreaView { elapsed: from_count_up(a.elapsed, anchor), ..a.clone() }).collect(),
        };

        // Later views are deltas against this one or newer.
        if let Some(b) = view.base {
            self.kept.retain(|s| s.tick >= b);
        }
        self.kept.push_back(state);
        while self.kept.len() > KEPT {
            self.kept.pop_front();
        }
        Ok((Snapshot { tick, time, units, player, events: view.events }, combat))
    }
}

fn quantise(u: &UnitState, now_ms: i64) -> Quantised {
    let q = |v: f32, step: f32| (v * step).round() as i32;
    Quantised {
        level: u.level,
        pos: [q(u.position.x, CM), q(u.position.y, CM), q(u.position.z, CM)],
        vel: [q(u.velocity.x, VEL), q(u.velocity.y, VEL), q(u.velocity.z, VEL)],
        yaw: (u.yaw.rem_euclid(std::f32::consts::TAU) * YAW).round() as i64 as u16,
        pose: u.pose,
        since: now_ms - (u.pose_time as f64 * 1000.0).round() as i64,
        traversal: u.traversal,
        health: (u.health.max(0.0) * HEALTH).round() as u32,
        max_health: u.max_health,
        hostile: u.hostile,
        target: u.target,
    }
}

fn unit(id: UnitId, q: &Quantised, i: &Intro, now_ms: i64) -> UnitState {
    let v = |a: [i32; 3], step: f32| Vec3::new(a[0] as f32 / step, a[1] as f32 / step, a[2] as f32 / step);
    UnitState {
        id,
        kind: i.kind,
        body: i.body.clone(),
        name: i.name.clone(),
        level: q.level,
        position: v(q.pos, CM),
        velocity: v(q.vel, VEL),
        yaw: q.yaw as f32 / YAW,
        pose: q.pose,
        pose_time: ((now_ms - q.since) as f64 / 1000.0).max(0.0) as f32,
        traversal: q.traversal,
        health: q.health as f32 / HEALTH,
        max_health: q.max_health,
        hostile: q.hostile,
        target: q.target,
        look: i.look.clone(),
    }
}

// Which of a unit's parts a delta carries.
const POS: u8 = 1;
const VELOCITY: u8 = 2;
const FACING: u8 = 4;
const POSE: u8 = 8;
const TRAVERSAL: u8 = 16;
const HEALTH_BIT: u8 = 32;
const REST: u8 = 64;
/// No base for this unit: positions are absolute.
const NEW: u8 = 128;

fn put<T: Serialize>(out: &mut Vec<u8>, value: &T) {
    let mut bytes = postcard::to_stdvec(value).expect("wire parts always serialise");
    out.append(&mut bytes);
}

fn take<'a, T: for<'de> Deserialize<'de>>(input: &mut &'a [u8]) -> Result<T, WireError> {
    let (value, rest) = postcard::take_from_bytes(input).map_err(|e| WireError::Malformed(e.to_string()))?;
    *input = rest;
    Ok(value)
}

/// Each changed unit, in ID order: the gap from the last ID, a byte saying which parts follow,
/// then those parts (postcard varints, positions as centimetres from the base).
fn write_units(out: &mut Vec<u8>, changed: &[(UnitId, Quantised, Option<Quantised>)], now_ms: i64) {
    let mut last = 0;
    for (id, q, base) in changed {
        put(out, &(id - last));
        last = *id;
        let mask = match base {
            None => NEW | POS | VELOCITY | FACING | POSE | TRAVERSAL | HEALTH_BIT | REST,
            Some(b) => {
                (POS * (q.pos != b.pos) as u8)
                    | (VELOCITY * (q.vel != b.vel) as u8)
                    | (FACING * (q.yaw != b.yaw) as u8)
                    | (POSE * (q.pose != b.pose || q.since != b.since) as u8)
                    | (TRAVERSAL * (q.traversal != b.traversal) as u8)
                    | (HEALTH_BIT * (q.health != b.health) as u8)
                    | (REST * (q.level != b.level || q.max_health != b.max_health || q.hostile != b.hostile || q.target != b.target) as u8)
            }
        };
        out.push(mask);
        let from = base.map_or([0; 3], |b| b.pos);
        if mask & POS != 0 {
            for a in 0..3 {
                put(out, &(q.pos[a] - from[a]));
            }
        }
        if mask & VELOCITY != 0 {
            put(out, &q.vel);
        }
        if mask & FACING != 0 {
            let turn = q.yaw.wrapping_sub(base.map_or(0, |b| b.yaw)) as i16;
            put(out, &turn);
        }
        if mask & POSE != 0 {
            put(out, &(q.pose, now_ms - q.since));
        }
        if mask & TRAVERSAL != 0 {
            put(out, &q.traversal);
        }
        if mask & HEALTH_BIT != 0 {
            put(out, &q.health);
        }
        if mask & REST != 0 {
            put(out, &(q.level, q.max_health, q.hostile, q.target));
        }
    }
}

fn read_units(mut input: &[u8], base: &Sorted<(Quantised, Arc<Intro>)>, now_ms: i64) -> Result<Vec<(UnitId, Quantised)>, WireError> {
    let mut out = Vec::new();
    let mut last: UnitId = 0;
    while !input.is_empty() {
        let gap: UnitId = take(&mut input)?;
        let id = last.checked_add(gap).ok_or_else(|| WireError::Malformed("unit IDs out of order".into()))?;
        last = id;
        let mask: u8 = take(&mut input)?;
        let mut q = if mask & NEW != 0 {
            Quantised {
                level: 0,
                pos: [0; 3],
                vel: [0; 3],
                yaw: 0,
                pose: Pose::Idle,
                since: now_ms,
                traversal: Traversal::Ground,
                health: 0,
                max_health: 0.0,
                hostile: false,
                target: None,
            }
        } else {
            base.get(id).map(|(b, _)| *b).ok_or_else(|| WireError::Malformed(format!("a delta for unit {id}, which the base hasn't")))?
        };
        if mask & POS != 0 {
            for a in 0..3 {
                let d: i32 = take(&mut input)?;
                q.pos[a] += d;
            }
        }
        if mask & VELOCITY != 0 {
            q.vel = take(&mut input)?;
        }
        if mask & FACING != 0 {
            let turn: i16 = take(&mut input)?;
            q.yaw = q.yaw.wrapping_add(turn as u16);
        }
        if mask & POSE != 0 {
            let (pose, ago): (Pose, i64) = take(&mut input)?;
            q.pose = pose;
            q.since = now_ms - ago;
        }
        if mask & TRAVERSAL != 0 {
            q.traversal = take(&mut input)?;
        }
        if mask & HEALTH_BIT != 0 {
            q.health = take(&mut input)?;
        }
        if mask & REST != 0 {
            (q.level, q.max_health, q.hostile, q.target) = take(&mut input)?;
        }
        out.push((id, q));
    }
    Ok(out)
}

/// Seconds left, as the tick (from the anchor) it runs out; 0 when it already has.
fn countdown(left: f32, anchor: u64) -> f32 {
    let ticks = (left * TICKS).round();
    if ticks <= 0.0 { 0.0 } else { anchor as f32 + ticks }
}

fn from_countdown(at: f32, anchor: u64) -> f32 {
    if at == 0.0 { 0.0 } else { ((at - anchor as f32) / TICKS).max(0.0) }
}

/// Seconds since, as the tick (from the anchor) it began.
fn count_up(elapsed: f32, anchor: u64) -> f32 {
    anchor as f32 - (elapsed * TICKS).round()
}

fn from_count_up(at: f32, anchor: u64) -> f32 {
    ((anchor as f32 - at) / TICKS).max(0.0)
}

fn auras_out(auras: &[AuraView], anchor: u64) -> Vec<AuraView> {
    auras.iter().map(|a| AuraView { remaining: countdown(a.remaining, anchor), ..a.clone() }).collect()
}

fn auras_in(auras: &[AuraView], anchor: u64) -> Vec<AuraView> {
    auras.iter().map(|a| AuraView { remaining: from_countdown(a.remaining, anchor), ..a.clone() }).collect()
}

fn cast_out(c: &Option<CastBar>, anchor: u64) -> Option<CastBar> {
    c.as_ref().map(|c| CastBar { elapsed: count_up(c.elapsed, anchor), ..c.clone() })
}

fn cast_in(c: &Option<CastBar>, anchor: u64) -> Option<CastBar> {
    c.as_ref().map(|c| CastBar { elapsed: from_count_up(c.elapsed, anchor), ..c.clone() })
}

fn combat_out(c: &UnitCombat, anchor: u64) -> UnitCombat {
    UnitCombat {
        action_time: count_up(c.action_time, anchor),
        cast: cast_out(&c.cast, anchor),
        auras: auras_out(&c.auras, anchor),
        resource: c.resource.round(),
        ..c.clone()
    }
}

/// Whether `c` would go out as `n`, which already went.
fn combat_matches(c: &UnitCombat, n: &UnitCombat, anchor: u64) -> bool {
    let cast = match (&c.cast, &n.cast) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            count_up(a.elapsed, anchor) == b.elapsed
                && (a.ability.as_str(), a.name.as_str(), a.duration, a.channel, a.interruptible) == (b.ability.as_str(), b.name.as_str(), b.duration, b.channel, b.interruptible)
        }
        _ => false,
    };
    let aura = |a: &AuraView, b: &AuraView| {
        countdown(a.remaining, anchor) == b.remaining && (a.key.as_str(), a.name.as_str(), a.stacks, a.duration, a.debuff) == (b.key.as_str(), b.name.as_str(), b.stacks, b.duration, b.debuff)
    };
    c.id == n.id
        && c.action == n.action
        && count_up(c.action_time, anchor) == n.action_time
        && cast
        && c.auras.len() == n.auras.len()
        && c.auras.iter().zip(&n.auras).all(|(a, b)| aura(a, b))
        && c.resource_kind == n.resource_kind
        && c.resource.round() == n.resource
        && c.max_resource == n.max_resource
        && c.guarding == n.guarding
        && c.downed == n.downed
        && c.defiance == n.defiance
}

fn combat_in(c: &UnitCombat, anchor: u64) -> UnitCombat {
    UnitCombat { action_time: from_count_up(c.action_time, anchor), cast: cast_in(&c.cast, anchor), auras: auras_in(&c.auras, anchor), ..c.clone() }
}

fn split(p: &PlayerState, anchor: u64) -> (Fast, Bar, Bags, Craft, Growth) {
    let PlayerState {
        unit,
        endurance,
        max_endurance,
        cooldowns,
        auto_attack,
        in_combat,
        can_respawn,
        resource_kind,
        resource,
        max_resource,
        bar,
        gcd,
        cast,
        auras,
        guarding,
        downed,
        weapon_set,
        swap_ready,
        swimming,
        sprinting,
        specialty,
        level,
        xp,
        xp_next,
        unassigned_xp,
        specialties,
        coins,
        inventory,
        loot,
        shop,
        skills,
        work,
        nodes,
        station,
        potion_ready,
    } = p;
    let fast = Fast {
        unit: *unit,
        endurance: endurance.max(0.0).round() as u32,
        max_endurance: *max_endurance,
        resource_kind: *resource_kind,
        resource: resource.max(0.0).round() as u32,
        max_resource: *max_resource,
        auto_attack: *auto_attack,
        in_combat: *in_combat,
        can_respawn: *can_respawn,
        guarding: *guarding,
        downed: *downed,
        swimming: *swimming,
        sprinting: *sprinting,
        weapon_set: *weapon_set,
    };
    let bar = Bar {
        cooldowns: cooldowns.iter().map(|&c| countdown(c, anchor)).collect(),
        bar: bar.iter().map(|s| Slot { cooldown: countdown(s.cooldown, anchor), ..s.clone() }).collect(),
        gcd: countdown(*gcd, anchor),
        swap_ready: countdown(*swap_ready, anchor),
        potion_ready: countdown(*potion_ready, anchor),
        cast: cast_out(cast, anchor),
        auras: auras_out(auras, anchor),
    };
    let bags = Bags { coins: *coins, inventory: inventory.clone(), loot: loot.clone(), shop: shop.clone() };
    let work = work.as_ref().map(|w| WorkView { elapsed: count_up(w.elapsed, anchor), ..w.clone() });
    let craft = Craft { skills: skills.clone(), work, nodes: nodes.clone(), station: station.clone() };
    let growth = Growth {
        specialty: specialty.clone(),
        level: *level,
        xp: *xp,
        xp_next: *xp_next,
        unassigned_xp: *unassigned_xp,
        specialties: specialties.clone(),
    };
    (fast, bar, bags, craft, growth)
}

fn join((f, b, bags, c, g): &(Fast, Bar, Bags, Craft, Growth), anchor: u64) -> PlayerState {
    PlayerState {
        unit: f.unit,
        endurance: f.endurance as f32,
        max_endurance: f.max_endurance,
        cooldowns: b.cooldowns.iter().map(|&c| from_countdown(c, anchor)).collect(),
        auto_attack: f.auto_attack,
        in_combat: f.in_combat,
        can_respawn: f.can_respawn,
        resource_kind: f.resource_kind,
        resource: f.resource as f32,
        max_resource: f.max_resource,
        bar: b.bar.iter().map(|s| Slot { cooldown: from_countdown(s.cooldown, anchor), ..s.clone() }).collect(),
        gcd: from_countdown(b.gcd, anchor),
        cast: cast_in(&b.cast, anchor),
        auras: auras_in(&b.auras, anchor),
        guarding: f.guarding,
        downed: f.downed,
        weapon_set: f.weapon_set,
        swap_ready: from_countdown(b.swap_ready, anchor),
        swimming: f.swimming,
        sprinting: f.sprinting,
        specialty: g.specialty.clone(),
        level: g.level,
        xp: g.xp,
        xp_next: g.xp_next,
        unassigned_xp: g.unassigned_xp,
        specialties: g.specialties.clone(),
        coins: bags.coins,
        inventory: bags.inventory.clone(),
        loot: bags.loot.clone(),
        shop: bags.shop.clone(),
        skills: c.skills.clone(),
        work: c.work.as_ref().map(|w| WorkView { elapsed: from_count_up(w.elapsed, anchor), ..w.clone() }),
        nodes: c.nodes.clone(),
        station: c.station.clone(),
        potion_ready: from_countdown(b.potion_ready, anchor),
    }
}
