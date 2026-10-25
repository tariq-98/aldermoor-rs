//! Gathering and crafting skills (#68, docs/CRAFTING.md).
//!
//! Five gathering skills (mining, woodcutting, skinning, herbalism, fishing) take materials from
//! **nodes** that run out after a few uses and grow back. Six crafts (smithing, tailoring,
//! leatherworking, enchanting, cooking, alchemy) turn them into goods at **stations**. Every
//! skill levels on its own through use, to the same cap as specialties.
//!
//! Gathering needs a **tool** for the skill in your pack; better tools swing faster. Each swing
//! has an optional **sweet spot** ([`SWEET_SPOT`]): strike inside it for one more of the yield
//! and a chance at the node's rare find, or let the swing finish for the normal yield.
//!
//! The tables are `crates/content/crafts/*.ron`, loaded and checked once ([`crafts`]). Items
//! are the item tables' ([`crate::items`]), by key.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::items::{ItemKind, economy};

/// The craft tables, in load order.
pub const FILES: [(&str, &str); 2] = [
    ("gathering.ron", include_str!("../crafts/gathering.ron")),
    ("recipes.ron", include_str!("../crafts/recipes.ron")),
];

/// Where in a swing the sweet spot is, as shares of the swing: strike between these.
pub const SWEET_SPOT: (f32, f32) = (0.65, 0.85);
/// How close to a station a player stands to work at it.
pub const STATION_REACH: f32 = 4.0;
/// Skills stop here, like specialties.
pub const MAX_SKILL_LEVEL: u8 = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Skill {
    Mining,
    Woodcutting,
    Skinning,
    Herbalism,
    Fishing,
    Smithing,
    Tailoring,
    Leatherworking,
    Enchanting,
    Cooking,
    Alchemy,
}

impl Skill {
    pub const ALL: [Skill; 11] = [
        Skill::Mining,
        Skill::Woodcutting,
        Skill::Skinning,
        Skill::Herbalism,
        Skill::Fishing,
        Skill::Smithing,
        Skill::Tailoring,
        Skill::Leatherworking,
        Skill::Enchanting,
        Skill::Cooking,
        Skill::Alchemy,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Takes from nodes, rather than making at a station.
    pub fn gathering(self) -> bool {
        self.index() < 5
    }

    pub fn key(self) -> &'static str {
        match self {
            Skill::Mining => "mining",
            Skill::Woodcutting => "woodcutting",
            Skill::Skinning => "skinning",
            Skill::Herbalism => "herbalism",
            Skill::Fishing => "fishing",
            Skill::Smithing => "smithing",
            Skill::Tailoring => "tailoring",
            Skill::Leatherworking => "leatherworking",
            Skill::Enchanting => "enchanting",
            Skill::Cooking => "cooking",
            Skill::Alchemy => "alchemy",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Skill::Mining => "Mining",
            Skill::Woodcutting => "Woodcutting",
            Skill::Skinning => "Skinning",
            Skill::Herbalism => "Herbalism",
            Skill::Fishing => "Fishing",
            Skill::Smithing => "Smithing",
            Skill::Tailoring => "Tailoring",
            Skill::Leatherworking => "Leatherworking",
            Skill::Enchanting => "Enchanting",
            Skill::Cooking => "Cooking",
            Skill::Alchemy => "Alchemy",
        }
    }
}

/// Something to gather from: an ore vein, a tree, a herb patch, a fishing spot, a carcass.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDef {
    pub key: &'static str,
    pub name: &'static str,
    pub skill: Skill,
    /// The skill level it takes.
    #[serde(default = "one")]
    pub level: u8,
    /// What each swing gives, between the first and the second.
    pub item: &'static str,
    #[serde(default = "one_each")]
    pub gives: (u16, u16),
    /// Swings before it runs out, and seconds until it's back.
    pub uses: u8,
    pub regrow: f32,
    /// Seconds a swing takes with a tool of speed 1.
    pub swing: f32,
    /// XP each swing.
    pub xp: u32,
    /// A rare find a sweet-spot strike may turn up: the item and its chance.
    #[serde(default)]
    pub rare: Option<(&'static str, f32)>,
    /// How close a player stands to work it (fishing spots are out in the river).
    #[serde(default = "reach")]
    pub reach: f32,
}

fn one() -> u8 {
    1
}

fn one_each() -> (u16, u16) {
    (1, 1)
}

fn reach() -> f32 {
    3.0
}

/// A node standing in the world, (x, z) in metres.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnDef {
    pub node: &'static str,
    pub at: (f32, f32),
}

/// A monster whose carcass can be skinned once, as `node`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarcassDef {
    pub mob: &'static str,
    pub node: &'static str,
}

/// An item that gathers for a skill. The fastest one in the pack that the player has the level
/// for is the one used.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDef {
    pub item: &'static str,
    pub skill: Skill,
    #[serde(default = "one")]
    pub level: u8,
    /// Swings take the node's `swing` divided by this.
    pub speed: f32,
}

/// A place to craft: a forge, an anvil, a loom, …
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StationDef {
    pub key: &'static str,
    pub name: &'static str,
    /// What recipes ask for: "forge", "anvil", "loom", "tanning_rack", "altar", "fire",
    /// "alchemy_table".
    pub kind: &'static str,
    pub at: (f32, f32),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeDef {
    pub key: &'static str,
    pub skill: Skill,
    #[serde(default = "one")]
    pub level: u8,
    /// The kind of station it's made at.
    pub station: &'static str,
    pub inputs: Vec<(&'static str, u16)>,
    pub output: (&'static str, u16),
    /// Seconds to make one.
    pub seconds: f32,
    pub xp: u32,
}

/// One table file. Every list is optional.
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, bound(deserialize = "'de: 'static"))]
pub struct Table {
    pub nodes: Vec<NodeDef>,
    pub spawns: Vec<SpawnDef>,
    pub carcasses: Vec<CarcassDef>,
    pub tools: Vec<ToolDef>,
    pub stations: Vec<StationDef>,
    pub recipes: Vec<RecipeDef>,
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
                "crates/content/crafts/{file}:{}: {}",
                error.span.start, error.code
            ),
            LoadError::Problems(p) => {
                write!(f, "the craft tables don't add up:\n  {}", p.join("\n  "))
            }
        }
    }
}

impl std::error::Error for LoadError {}

pub struct Crafts {
    pub nodes: Vec<NodeDef>,
    pub spawns: Vec<SpawnDef>,
    pub carcasses: Vec<CarcassDef>,
    pub tools: Vec<ToolDef>,
    pub stations: Vec<StationDef>,
    pub recipes: Vec<RecipeDef>,
    node_ids: HashMap<&'static str, usize>,
}

impl Crafts {
    pub fn load() -> Result<Crafts, LoadError> {
        Crafts::from_files(&FILES)
    }

    pub fn from_files(files: &[(&str, &'static str)]) -> Result<Crafts, LoadError> {
        let mut all = Table::default();
        for (file, text) in files {
            let t: Table = ron::from_str(text).map_err(|error| LoadError::Syntax {
                file: file.to_string(),
                error: Box::new(error),
            })?;
            all.nodes.extend(t.nodes);
            all.spawns.extend(t.spawns);
            all.carcasses.extend(t.carcasses);
            all.tools.extend(t.tools);
            all.stations.extend(t.stations);
            all.recipes.extend(t.recipes);
        }
        let node_ids = all
            .nodes
            .iter()
            .enumerate()
            .map(|(i, d)| (d.key, i))
            .collect();
        let c = Crafts {
            nodes: all.nodes,
            spawns: all.spawns,
            carcasses: all.carcasses,
            tools: all.tools,
            stations: all.stations,
            recipes: all.recipes,
            node_ids,
        };
        let problems = c.problems();
        if problems.is_empty() {
            Ok(c)
        } else {
            Err(LoadError::Problems(problems))
        }
    }

    pub fn node(&self, key: &str) -> Option<&NodeDef> {
        self.node_ids.get(key).map(|&i| &self.nodes[i])
    }

    pub fn recipe(&self, key: &str) -> Option<&RecipeDef> {
        self.recipes.iter().find(|r| r.key == key)
    }

    /// The node a dead `mob` can be skinned as, if any.
    pub fn carcass(&self, mob: &str) -> Option<&NodeDef> {
        self.carcasses
            .iter()
            .find(|c| c.mob == mob)
            .and_then(|c| self.node(c.node))
    }

    pub fn tool(&self, item: &str) -> Option<&ToolDef> {
        self.tools.iter().find(|t| t.item == item)
    }

    /// Everything that doesn't add up: unknown keys, duplicates, impossible numbers. Items are
    /// checked against the item tables and monsters against the combat tables.
    pub fn problems(&self) -> Vec<String> {
        let eco = economy();
        let mut out = Vec::new();
        let item = |out: &mut Vec<String>, what: &str, key: &str| {
            if eco.item(key).is_none() {
                out.push(format!("{what}: unknown item {key}"));
            }
        };
        let level = |out: &mut Vec<String>, what: &str, l: u8| {
            if !(1..=MAX_SKILL_LEVEL).contains(&l) {
                out.push(format!("{what}: level {l}"));
            }
        };
        let mut seen = HashMap::new();
        for n in &self.nodes {
            let what = format!("node {}", n.key);
            if seen.insert(n.key, ()).is_some() {
                out.push(format!("{what} is defined twice"));
            }
            if !n.skill.gathering() {
                out.push(format!("{what}: {} isn't gathered", n.skill.key()));
            }
            level(&mut out, &what, n.level);
            item(&mut out, &what, n.item);
            if n.gives.0 == 0 || n.gives.0 > n.gives.1 {
                out.push(format!("{what}: gives {:?}", n.gives));
            }
            if n.uses == 0 || n.regrow <= 0.0 || n.swing <= 0.0 || n.reach <= 0.0 {
                out.push(format!(
                    "{what}: uses, regrow, swing and reach must be positive"
                ));
            }
            if let Some((rare, chance)) = n.rare {
                item(&mut out, &what, rare);
                if !(0.0..=1.0).contains(&chance) || chance == 0.0 {
                    out.push(format!("{what}: rare chance {chance}"));
                }
            }
        }
        for s in &self.spawns {
            match self.node(s.node) {
                None => out.push(format!("spawn at {:?}: unknown node {}", s.at, s.node)),
                Some(n) if n.skill == Skill::Skinning => {
                    out.push(format!("spawn at {:?}: carcasses aren't placed", s.at))
                }
                Some(_) => {}
            }
        }
        for c in &self.carcasses {
            if crate::combat::catalog().mob(c.mob).is_none() {
                out.push(format!("carcass {}: no such monster", c.mob));
            }
            if !self
                .node(c.node)
                .is_some_and(|n| n.skill == Skill::Skinning)
            {
                out.push(format!(
                    "carcass {}: {} isn't a skinning node",
                    c.mob, c.node
                ));
            }
        }
        for t in &self.tools {
            let what = format!("tool {}", t.item);
            item(&mut out, &what, t.item);
            if eco.item(t.item).is_some_and(|d| d.kind != ItemKind::Tool) {
                out.push(format!("{what}: the item isn't a tool"));
            }
            if !t.skill.gathering() {
                out.push(format!("{what}: {} isn't gathered", t.skill.key()));
            }
            level(&mut out, &what, t.level);
            if t.speed <= 0.0 {
                out.push(format!("{what}: speed {}", t.speed));
            }
        }
        for s in Skill::ALL.iter().filter(|s| s.gathering()) {
            if !self.tools.iter().any(|t| t.skill == *s && t.level == 1) {
                out.push(format!("{}: no tool for a beginner", s.key()));
            }
        }
        for r in &self.recipes {
            let what = format!("recipe {}", r.key);
            if self.recipes.iter().filter(|q| q.key == r.key).count() > 1 {
                out.push(format!("{what} is defined twice"));
            }
            if r.skill.gathering() {
                out.push(format!("{what}: {} isn't a craft", r.skill.key()));
            }
            level(&mut out, &what, r.level);
            if !self.stations.iter().any(|s| s.kind == r.station) {
                out.push(format!("{what}: no {} anywhere", r.station));
            }
            if r.inputs.is_empty() || r.inputs.iter().any(|i| i.1 == 0) || r.output.1 == 0 {
                out.push(format!("{what}: counts must be positive"));
            }
            for (i, _) in &r.inputs {
                item(&mut out, &what, i);
            }
            item(&mut out, &what, r.output.0);
            if r.seconds <= 0.0 {
                out.push(format!("{what}: takes {} seconds", r.seconds));
            }
        }
        out
    }
}

/// The built-in craft tables, loaded and checked on first use (panics on a bad table, which this
/// crate's tests catch).
pub fn crafts() -> &'static Crafts {
    static CRAFTS: OnceLock<Crafts> = OnceLock::new();
    CRAFTS.get_or_init(|| Crafts::load().unwrap_or_else(|e| panic!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_craft_tables_load_and_add_up() {
        let c = Crafts::load().unwrap_or_else(|e| panic!("{e}"));
        for s in Skill::ALL {
            let used = if s.gathering() {
                c.nodes.iter().any(|n| n.skill == s)
            } else {
                c.recipes.iter().any(|r| r.skill == s)
            };
            assert!(used, "nothing uses {}", s.key());
        }
        assert!(c.carcass("wolf").is_some());
    }

    #[test]
    fn every_placed_node_and_station_is_in_the_valley() {
        let c = crafts();
        for (what, at) in c
            .spawns
            .iter()
            .map(|s| (s.node, s.at))
            .chain(c.stations.iter().map(|s| (s.key, s.at)))
        {
            assert!(
                at.0.abs() < 280.0 && at.1.abs() < 280.0,
                "{what} at {at:?} is outside the valley"
            );
        }
    }

    #[test]
    fn a_bad_table_says_what_is_wrong() {
        let bad: &'static str = r#"(
            nodes: [(key: "x", name: "X", skill: Smithing, item: "nothing", uses: 0, regrow: 1.0, swing: 1.0, xp: 1)],
            recipes: [(key: "y", skill: Mining, station: "nowhere", inputs: [], output: ("bread", 1), seconds: 1.0, xp: 1)],
        )"#;
        let Err(LoadError::Problems(p)) = Crafts::from_files(&[("bad.ron", bad)]) else {
            panic!("a bad table loaded")
        };
        for want in [
            "smithing isn't gathered",
            "unknown item nothing",
            "must be positive",
            "mining isn't a craft",
            "no nowhere anywhere",
            "no tool for a beginner",
        ] {
            assert!(
                p.iter().any(|p| p.contains(want)),
                "missing {want:?} in {p:?}"
            );
        }
    }
}
