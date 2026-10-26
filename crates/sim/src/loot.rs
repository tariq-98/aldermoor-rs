//! Inventories, loot and shops (#71, docs/ECONOMY.md).
//!
//! A monster that dies leaves each player who did a fair share of the fight (#201) a pile of
//! their own, rolled from its loot table, that only they can see and take. It's gone after
//! [`PILE_LASTS`]. Shops sell at set prices and pay what their town pays. Items are the content
//! crate's (`content::items`), referred to by key.

use bevy_ecs::prelude::*;
use content::items::{economy, ItemDef, SHOP_REACH};
use glam::{Vec2, Vec3};
use protocol::{ItemView, LootView, ShopView, UnitId};

use crate::combat::tell;
use crate::{Brain, Clock, Credit, Rng, Summoned, Transform, Unit};

/// Inventory slots. Materials and food stack in one; gear takes one each.
pub const SLOTS: usize = 28;
/// Seconds a pile lies there.
pub const PILE_LASTS: f64 = 180.0;
/// How close you stand to a pile to pick it up, and to see it in your state.
pub const PILE_REACH: f32 = 3.0;
pub const PILE_SEEN: f32 = 12.0;

/// Some of one item in a slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stack {
    pub item: &'static str,
    pub count: u16,
    pub fine: bool,
}

/// What a player carries.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Inventory {
    pub slots: Vec<Option<Stack>>,
    pub coins: u32,
}

impl Default for Inventory {
    fn default() -> Self {
        Inventory { slots: vec![None; SLOTS], coins: 0 }
    }
}

impl Inventory {
    /// Puts `count` of an item in, topping up stacks of it first, then empty slots. Returns
    /// how many didn't fit.
    pub fn add(&mut self, def: &ItemDef, mut count: u16, fine: bool) -> u16 {
        let fine = fine && def.can_be_fine();
        if def.stack > 1 {
            for s in self.slots.iter_mut().flatten().filter(|s| s.item == def.key && s.fine == fine) {
                let take = (def.stack - s.count.min(def.stack)).min(count);
                s.count += take;
                count -= take;
            }
        }
        for slot in self.slots.iter_mut().filter(|s| s.is_none()) {
            if count == 0 {
                break;
            }
            let take = count.min(def.stack);
            *slot = Some(Stack { item: def.key, count: take, fine });
            count -= take;
        }
        count
    }

    /// Whether all of `count` would fit.
    pub fn fits(&self, def: &ItemDef, count: u16, fine: bool) -> bool {
        self.clone().add(def, count, fine) == 0
    }

    /// Takes `count` of an item out, from the last stacks first. Returns false, taking
    /// nothing, if there aren't that many.
    pub fn remove(&mut self, item: &str, count: u16) -> bool {
        if self.count(item) < count as u32 {
            return false;
        }
        let mut left = count;
        for slot in self.slots.iter_mut().rev() {
            if left == 0 {
                break;
            }
            if let Some(s) = slot
                && s.item == item
            {
                let take = s.count.min(left);
                s.count -= take;
                left -= take;
                if s.count == 0 {
                    *slot = None;
                }
            }
        }
        true
    }

    /// How many of an item it holds, across slots.
    pub fn count(&self, item: &str) -> u32 {
        self.slots.iter().flatten().filter(|s| s.item == item).map(|s| s.count as u32).sum()
    }
}

/// What a dead monster left.
#[derive(Clone, Debug)]
pub struct Pile {
    pub id: u32,
    pub at: Vec3,
    pub owner: Option<UnitId>,
    pub items: Vec<Stack>,
    pub coins: u32,
    pub dropped: f64,
}

/// Every pile on the ground, and the dice for what drops: their own sequence from the same
/// seed, so loot doesn't shift every fight's rolls.
#[derive(Resource)]
pub struct Loot {
    pub piles: Vec<Pile>,
    next: u32,
    rng: Rng,
}

impl Loot {
    pub fn new(seed: u64) -> Loot {
        Loot { piles: Vec::new(), next: 0, rng: Rng::new(seed ^ 0x6c6f_6f74) }
    }
}

/// Rolls a dying monster's loot table once for each player who earned a share of it, into a
/// pile of their own where it fell. Summons leave nothing.
pub(crate) fn drop_loot(world: &mut World, dead: Entity) {
    if world.get::<Summoned>(dead).is_some() {
        return;
    }
    let Some(table) = world.get::<Brain>(dead).and_then(|b| economy().loot_for(b.mob)) else { return };
    let Some(at) = world.get::<Transform>(dead).map(|t| t.pos) else { return };
    let earners = world.get::<Credit>(dead).map(|c| c.earners()).unwrap_or_default();
    let t = world.resource::<Clock>().time;
    let mut loot = world.resource_mut::<Loot>();
    for owner in earners {
        let pile = roll(&mut loot.rng, table, at, Some(owner), t);
        if pile.items.is_empty() && pile.coins == 0 {
            continue;
        }
        loot.next += 1;
        let id = loot.next;
        loot.piles.push(Pile { id, ..pile });
    }
}

/// Leaves `items` in a pile at `at` that only `owner` may take: what didn't fit in their pack.
pub(crate) fn drop_for(world: &mut World, owner: UnitId, at: Vec3, items: Vec<Stack>) {
    let t = world.resource::<Clock>().time;
    let mut loot = world.resource_mut::<Loot>();
    loot.next += 1;
    let id = loot.next;
    loot.piles.push(Pile { id, at, owner: Some(owner), items, coins: 0, dropped: t });
}

/// One roll of a loot table: each line on its own, gear sometimes fine.
pub fn roll(rng: &mut Rng, table: &content::items::LootTable, at: Vec3, owner: Option<UnitId>, now: f64) -> Pile {
    let eco = economy();
    let coins = if table.coins.1 > table.coins.0 {
        table.coins.0 + rng.next_u32() % (table.coins.1 - table.coins.0 + 1)
    } else {
        table.coins.0
    };
    let mut items = Vec::new();
    for d in &table.drops {
        if !rng.chance(d.chance) {
            continue;
        }
        let count = d.count.0 + (rng.next_u32() % (d.count.1 - d.count.0 + 1) as u32) as u16;
        let def = eco.item(d.item).expect("loot tables are checked on load");
        let fine = def.can_be_fine() && rng.chance(table.fine_chance);
        items.push(Stack { item: def.key, count, fine });
    }
    Pile { id: 0, at, owner, items, coins, dropped: now }
}

/// Takes what fits from a pile within reach that the player may loot.
pub(crate) fn take(world: &mut World, e: Entity, pile: u32) {
    let (Some(id), Some(p)) = (world.get::<Unit>(e).map(|u| u.id), world.get::<Transform>(e).map(|t| t.pos)) else { return };
    let Some(i) = world.resource::<Loot>().piles.iter().position(|q| q.id == pile) else { return };
    let q = world.resource::<Loot>().piles[i].clone();
    if flat(q.at - p) > PILE_REACH || !may_loot(&q, id) {
        return;
    }
    let Some(mut inv) = world.get_mut::<Inventory>(e) else { return };
    inv.coins += q.coins;
    let mut left = Vec::new();
    for s in &q.items {
        let def = economy().item(s.item).expect("piles hold known items");
        let over = inv.add(def, s.count, s.fine);
        if over > 0 {
            left.push(Stack { count: over, ..s.clone() });
        }
    }
    let full = !left.is_empty();
    let mut loot = world.resource_mut::<Loot>();
    if left.is_empty() {
        loot.piles.remove(i);
    } else {
        loot.piles[i].items = left;
        loot.piles[i].coins = 0;
    }
    if full {
        tell(world, e, "Your pack is full.");
    }
}

/// A pile with an owner is theirs alone.
fn may_loot(p: &Pile, who: UnitId) -> bool {
    p.owner.is_none_or(|o| o == who)
}

fn flat(d: Vec3) -> f32 {
    Vec2::new(d.x, d.z).length()
}

/// Piles go when their time is up.
pub(crate) fn expire(world: &mut World) {
    let t = world.resource::<Clock>().time;
    world.resource_mut::<Loot>().piles.retain(|p| t - p.dropped < PILE_LASTS);
}

/// The shop whose counter the player stands at, if any.
fn shop_at(p: Vec3) -> Option<&'static content::items::ShopDef> {
    economy().shops.iter().find(|s| Vec2::new(p.x - s.at.0, p.z - s.at.1).length() <= SHOP_REACH)
}

/// Buys `count` of an item from a shop the player stands at, if they can pay and carry it.
pub(crate) fn buy(world: &mut World, e: Entity, shop: &str, item: &str, count: u16) {
    let eco = economy();
    let Some(p) = world.get::<Transform>(e).map(|t| t.pos) else { return };
    let (Some(s), Some(def)) = (shop_at(p).filter(|s| s.key == shop), eco.item(item)) else { return };
    if count == 0 || !s.sells.contains(&def.key) {
        return;
    }
    let cost = def.price(false) * count as u32;
    let Some(mut inv) = world.get_mut::<Inventory>(e) else { return };
    let message = if inv.coins < cost {
        Some("You can't afford that.")
    } else if !inv.fits(def, count, false) {
        Some("Your pack is full.")
    } else {
        inv.coins -= cost;
        inv.add(def, count, false);
        None
    };
    if let Some(m) = message {
        tell(world, e, m);
    }
}

/// Sells `count` from an inventory slot to the shop the player stands at, if it buys that.
pub(crate) fn sell(world: &mut World, e: Entity, shop: &str, slot: u8, count: u16) {
    let eco = economy();
    let Some(p) = world.get::<Transform>(e).map(|t| t.pos) else { return };
    let Some(s) = shop_at(p).filter(|s| s.key == shop) else { return };
    let Some(mut inv) = world.get_mut::<Inventory>(e) else { return };
    let Some(Some(stack)) = inv.slots.get(slot as usize).cloned() else { return };
    let Some(def) = eco.item(stack.item) else { return };
    let Some(each) = eco.buy_price(s, def, stack.fine) else {
        tell(world, e, "They don't buy that here.");
        return;
    };
    let n = count.min(stack.count);
    if n == 0 {
        return;
    }
    inv.coins += each * n as u32;
    let left = stack.count - n;
    inv.slots[slot as usize] = (left > 0).then_some(Stack { count: left, ..stack });
}

fn view(s: &Stack, price: u32) -> ItemView {
    let name = economy().item(s.item).map_or(s.item, |d| d.name);
    ItemView { item: s.item.to_string(), name: name.to_string(), count: s.count, fine: s.fine, price }
}

/// The player's pack, the piles near them they may take, and the shop they stand at: (coins,
/// slots, piles, shop). Inventory prices are what that shop pays, 0 where there's none.
pub(crate) fn player_view(world: &World, e: Entity) -> (u32, Vec<ItemView>, Vec<LootView>, Option<ShopView>) {
    let (Some(inv), Some(p), Some(id)) = (world.get::<Inventory>(e), world.get::<Transform>(e).map(|t| t.pos), world.get::<Unit>(e).map(|u| u.id)) else {
        return (0, Vec::new(), Vec::new(), None);
    };
    let eco = economy();
    let shop = shop_at(p);
    let slots = inv
        .slots
        .iter()
        .map(|s| match s {
            Some(s) => {
                let pays = shop.zip(eco.item(s.item)).and_then(|(sh, d)| eco.buy_price(sh, d, s.fine)).unwrap_or(0);
                view(s, pays)
            }
            None => ItemView::default(),
        })
        .collect();
    let piles = world
        .resource::<Loot>()
        .piles
        .iter()
        .filter(|q| flat(q.at - p) <= PILE_SEEN && may_loot(q, id))
        .map(|q| LootView { id: q.id, position: q.at, coins: q.coins, items: q.items.iter().map(|s| view(s, 0)).collect() })
        .collect();
    let shop = shop.map(|s| ShopView {
        shop: s.key.to_string(),
        name: s.name.to_string(),
        sells: s
            .sells
            .iter()
            .filter_map(|k| eco.item(k))
            .map(|d| view(&Stack { item: d.key, count: 1, fine: false }, d.price(false)))
            .collect(),
    });
    (inv.coins, slots, piles, shop)
}
