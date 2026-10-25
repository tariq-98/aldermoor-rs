//! Items, loot tables and shops (#71, docs/ECONOMY.md).
//!
//! An item always has the same stats: better gear comes from better material tiers and named
//! items, never random affixes. Gear can drop or be made as a **fine** version, a small fixed
//! bonus ([`FINE_BONUS`]) on the same item. Loot tables say what each monster drops; shops sell
//! at set prices and pay set prices, and each town pays more for the goods it wants.
//!
//! The tables are `crates/content/items/*.ron`, loaded and checked once ([`economy`]).

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// The item tables, in load order.
pub const FILES: [(&str, &str); 3] = [
    ("items.ron", include_str!("../items/items.ron")),
    ("loot.ron", include_str!("../items/loot.ron")),
    ("shops.ron", include_str!("../items/shops.ron")),
];

/// A fine version's stats and value over the plain one's: always this, never rolled.
pub const FINE_BONUS: f32 = 0.1;
/// A fine version sells for this many times the plain one.
pub const FINE_VALUE: u32 = 3;
/// Shops pay this share of an item's value (before the town's demand).
pub const SELL_SHARE: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    /// Held in a hand: `item` is also the gear catalogue's id and the weapon's key.
    Weapon,
    Armour,
    /// Crafting materials: ore, hides, herbs, wood. They stack.
    Material,
    /// Gathers for a skill while in the pack (`content::crafts`): a pickaxe, a rod, …
    Tool,
    /// Eaten or drunk. They stack.
    Consumable,
    /// Worth only what a shop pays.
    Junk,
}

/// What an item adds while equipped. Fixed for the item; a fine version adds [`FINE_BONUS`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemStats {
    pub power: f32,
    pub armor: f32,
    pub health: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    pub key: &'static str,
    pub name: &'static str,
    pub kind: ItemKind,
    /// The material tier, 1 upward (copper, iron, steel, …).
    #[serde(default = "one")]
    pub tier: u8,
    /// The price in coins a shop sells it for.
    pub value: u32,
    /// How many fit in one inventory slot: 1 for gear.
    #[serde(default = "one_u16")]
    pub stack: u16,
    #[serde(default)]
    pub stats: ItemStats,
    /// What it is, for shops' demand: "ore", "hide", "herb", "weapon", …
    #[serde(default)]
    pub tags: Vec<&'static str>,
}

fn one() -> u8 {
    1
}

fn one_u16() -> u16 {
    1
}

impl ItemDef {
    /// Only single gear (weapons and armour) comes in fine versions.
    pub fn can_be_fine(&self) -> bool {
        self.stack == 1 && matches!(self.kind, ItemKind::Weapon | ItemKind::Armour)
    }

    /// Its stats, plain or fine.
    pub fn stats(&self, fine: bool) -> ItemStats {
        let k = if fine { 1.0 + FINE_BONUS } else { 1.0 };
        ItemStats {
            power: self.stats.power * k,
            armor: self.stats.armor * k,
            health: self.stats.health * k,
        }
    }

    /// What a shop sells one for.
    pub fn price(&self, fine: bool) -> u32 {
        if fine {
            self.value * FINE_VALUE
        } else {
            self.value
        }
    }
}

/// One line of a loot table: `item` drops with `chance`, between `count.0` and `count.1` of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drop {
    pub item: &'static str,
    pub chance: f32,
    #[serde(default = "one_each")]
    pub count: (u16, u16),
}

fn one_each() -> (u16, u16) {
    (1, 1)
}

/// What a monster leaves when it dies. Each line rolls on its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LootTable {
    /// The monster's key in the combat tables.
    pub mob: &'static str,
    /// Coins, from the first to the second.
    #[serde(default)]
    pub coins: (u32, u32),
    #[serde(default)]
    pub drops: Vec<Drop>,
    /// The chance a piece of gear that drops is a fine version.
    #[serde(default = "fine_chance")]
    pub fine_chance: f32,
}

fn fine_chance() -> f32 {
    0.05
}

/// A town, and the goods its shops pay more for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TownDef {
    pub key: &'static str,
    pub name: &'static str,
    /// (tag, multiplier on what shops pay): a smithing town pays more for ore.
    #[serde(default)]
    pub demand: Vec<(&'static str, f32)>,
}

/// A shopkeeper: what they sell, where, and in which town.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopDef {
    pub key: &'static str,
    pub name: &'static str,
    pub town: &'static str,
    /// Where the counter is, (x, z) in metres: players trade within [`SHOP_REACH`] of it.
    pub at: (f32, f32),
    /// What they sell, always in stock.
    pub sells: Vec<&'static str>,
    /// Tags they buy; empty buys anything.
    #[serde(default)]
    pub buys: Vec<&'static str>,
}

/// How close a player stands to a shop's counter to trade.
pub const SHOP_REACH: f32 = 6.0;

/// One table file. Every list is optional.
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, bound(deserialize = "'de: 'static"))]
pub struct Table {
    pub items: Vec<ItemDef>,
    pub loot: Vec<LootTable>,
    pub towns: Vec<TownDef>,
    pub shops: Vec<ShopDef>,
}

#[derive(Debug)]
pub enum LoadError {
    Syntax {
        file: String,
        error: Box<ron::error::SpannedError>,
    },
    Problems(Vec<String>),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Syntax { file, error } => write!(
                f,
                "crates/content/items/{file}:{}: {}",
                error.span.start, error.code
            ),
            LoadError::Problems(p) => {
                write!(f, "the item tables don't add up:\n  {}", p.join("\n  "))
            }
        }
    }
}

impl std::error::Error for LoadError {}

pub struct Economy {
    pub items: Vec<ItemDef>,
    pub loot: Vec<LootTable>,
    pub towns: Vec<TownDef>,
    pub shops: Vec<ShopDef>,
    item_ids: HashMap<&'static str, usize>,
}

impl Economy {
    pub fn load() -> Result<Economy, LoadError> {
        Economy::from_files(&FILES)
    }

    pub fn from_files(files: &[(&str, &'static str)]) -> Result<Economy, LoadError> {
        let mut all = Table::default();
        for (file, text) in files {
            let t: Table = ron::from_str(text).map_err(|error| LoadError::Syntax {
                file: file.to_string(),
                error: Box::new(error),
            })?;
            all.items.extend(t.items);
            all.loot.extend(t.loot);
            all.towns.extend(t.towns);
            all.shops.extend(t.shops);
        }
        let item_ids = all
            .items
            .iter()
            .enumerate()
            .map(|(i, d)| (d.key, i))
            .collect();
        let e = Economy {
            items: all.items,
            loot: all.loot,
            towns: all.towns,
            shops: all.shops,
            item_ids,
        };
        let problems = e.problems();
        if problems.is_empty() {
            Ok(e)
        } else {
            Err(LoadError::Problems(problems))
        }
    }

    pub fn item(&self, key: &str) -> Option<&ItemDef> {
        self.item_ids.get(key).map(|&i| &self.items[i])
    }

    pub fn loot_for(&self, mob: &str) -> Option<&LootTable> {
        self.loot.iter().find(|l| l.mob == mob)
    }

    pub fn town(&self, key: &str) -> Option<&TownDef> {
        self.towns.iter().find(|t| t.key == key)
    }

    pub fn shop(&self, key: &str) -> Option<&ShopDef> {
        self.shops.iter().find(|s| s.key == key)
    }

    /// What `shop` pays for one `item` (fine or not), or `None` if it doesn't buy it.
    pub fn buy_price(&self, shop: &ShopDef, item: &ItemDef, fine: bool) -> Option<u32> {
        let wanted = shop.buys.is_empty() || item.tags.iter().any(|t| shop.buys.contains(t));
        if !wanted {
            return None;
        }
        let demand = self
            .town(shop.town)
            .map(|t| {
                t.demand
                    .iter()
                    .filter(|(tag, _)| item.tags.contains(tag))
                    .map(|(_, m)| *m)
                    .fold(1.0, f32::max)
            })
            .unwrap_or(1.0);
        Some(((item.price(fine) as f32 * SELL_SHARE * demand).floor() as u32).max(1))
    }

    /// Everything that doesn't add up: unknown keys, duplicates, impossible numbers. Monster
    /// keys are checked against the combat tables.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashMap::new();
        for d in &self.items {
            if seen.insert(d.key, ()).is_some() {
                out.push(format!("item {} is defined twice", d.key));
            }
            if d.stack == 0 {
                out.push(format!("item {} stacks to 0", d.key));
            }
            if d.value == 0 {
                out.push(format!("item {} is worth nothing", d.key));
            }
            if matches!(d.kind, ItemKind::Weapon | ItemKind::Armour) && d.stack != 1 {
                out.push(format!("item {}: gear doesn't stack", d.key));
            }
            if d.kind == ItemKind::Weapon && crate::combat::catalog().weapon(d.key).is_none() {
                out.push(format!(
                    "item {}: no weapon of that key in the combat tables",
                    d.key
                ));
            }
        }
        let mut mobs = HashMap::new();
        for l in &self.loot {
            if mobs.insert(l.mob, ()).is_some() {
                out.push(format!("loot for {} is defined twice", l.mob));
            }
            if crate::combat::catalog().mob(l.mob).is_none() {
                out.push(format!("loot for {}: no such monster", l.mob));
            }
            if l.coins.0 > l.coins.1 {
                out.push(format!("loot for {}: coins run backwards", l.mob));
            }
            if !(0.0..=1.0).contains(&l.fine_chance) {
                out.push(format!("loot for {}: fine chance {}", l.mob, l.fine_chance));
            }
            for d in &l.drops {
                if self.item(d.item).is_none() {
                    out.push(format!("loot for {}: unknown item {}", l.mob, d.item));
                }
                if !(0.0..=1.0).contains(&d.chance) || d.chance == 0.0 {
                    out.push(format!(
                        "loot for {}: {} drops with chance {}",
                        l.mob, d.item, d.chance
                    ));
                }
                if d.count.0 == 0 || d.count.0 > d.count.1 {
                    out.push(format!(
                        "loot for {}: {} count {:?}",
                        l.mob, d.item, d.count
                    ));
                }
            }
        }
        for t in &self.towns {
            for (tag, m) in &t.demand {
                if *m <= 0.0 {
                    out.push(format!("town {}: demand for {tag} is {m}", t.key));
                }
            }
        }
        for s in &self.shops {
            if self.town(s.town).is_none() {
                out.push(format!("shop {}: unknown town {}", s.key, s.town));
            }
            for i in &s.sells {
                if self.item(i).is_none() {
                    out.push(format!("shop {}: sells unknown item {i}", s.key));
                }
            }
        }
        out
    }
}

/// The built-in item tables, loaded and checked on first use (panics on a bad table, which this
/// crate's tests catch).
pub fn economy() -> &'static Economy {
    static ECONOMY: OnceLock<Economy> = OnceLock::new();
    ECONOMY.get_or_init(|| Economy::load().unwrap_or_else(|e| panic!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_item_tables_load_and_add_up() {
        let e = Economy::load().unwrap_or_else(|e| panic!("{e}"));
        assert!(e.items.len() >= 20);
        assert!(e.loot_for("wolf").is_some());
        assert!(!e.shops.is_empty());
    }

    #[test]
    fn a_fine_version_adds_the_same_fixed_bonus_every_time() {
        let sword = economy().item("sword.arming").unwrap();
        let (plain, fine) = (sword.stats(false), sword.stats(true));
        assert!(plain.power > 0.0);
        assert!((fine.power - plain.power * 1.1).abs() < 1e-4);
        assert_eq!(sword.stats(true), fine, "fine is fixed, not rolled");
        assert_eq!(sword.price(true), sword.value * FINE_VALUE);
        assert!(!economy().item("copper_ore").unwrap().can_be_fine());
    }

    #[test]
    fn a_town_pays_more_for_the_goods_it_wants() {
        let e = economy();
        let (ore, pelt) = (e.item("copper_ore").unwrap(), e.item("wolf_pelt").unwrap());
        let store = e.shop("brambleford_store").unwrap();
        let share = |v: u32, m: f32| (v as f32 * SELL_SHARE * m).floor() as u32;
        assert_eq!(e.buy_price(store, ore, false), Some(share(ore.value, 1.0)));
        assert_eq!(
            e.buy_price(store, pelt, false),
            Some(share(pelt.value, 1.5))
        );
        // The smith only buys metal.
        let smith = e.shop("brambleford_smith").unwrap();
        assert_eq!(e.buy_price(smith, pelt, false), None);
        assert!(e.buy_price(smith, ore, false).is_some());
    }

    #[test]
    fn a_bad_table_says_what_is_wrong() {
        let bad: &'static str =
            r#"(items: [(key: "x", name: "X", kind: Weapon, value: 0, stack: 5)])"#;
        let Err(LoadError::Problems(p)) = Economy::from_files(&[("bad.ron", bad)]) else {
            panic!("a bad table loaded")
        };
        assert!(p.iter().any(|p| p.contains("worth nothing")));
        assert!(p.iter().any(|p| p.contains("gear doesn't stack")));
    }
}
