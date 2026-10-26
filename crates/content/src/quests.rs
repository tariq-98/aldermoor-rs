//! Quests and jobs (#70).
//!
//! **Quests** are the big, handmade stories: a handful per area, given by someone in town, told
//! in **steps** that follow one another. Each step has one or more **objectives** (talk to
//! someone, kill, gather, deliver, reach a place, use an item there), and the step is done when
//! they all are. Quests don't branch: there is one way through, and finishing one can open
//! others (`needs`). The reward is coins, quest XP for the player to give to any specialty, and
//! items.
//!
//! **Jobs** are the small ones between quests: notice boards offer a few at a time, rolled from
//! templates and changed every [`JOB_ROTATION`] seconds. A job asks for a number of kills, of
//! something gathered or of something brought, and pays for each one.
//!
//! The tables are `crates/content/quests/*.ron`, loaded and checked once ([`quests`]). Items
//! are the item tables' and monsters the combat tables', by key.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::items::economy;

/// The quest tables, in load order.
pub const FILES: [(&str, &str); 1] =
    [("brambleford.ron", include_str!("../quests/brambleford.ron"))];

/// How close a player stands to someone to talk to them, or to a board to read it.
pub const TALK_REACH: f32 = 4.0;
/// Jobs a player can have at once.
pub const MAX_JOBS: usize = 3;
/// Seconds before a board's offers change.
pub const JOB_ROTATION: f64 = 900.0;

/// Someone in town who gives quests or has a part in them. They stand in one place until the
/// townsfolk and their routines exist (#128).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonDef {
    pub key: &'static str,
    pub name: &'static str,
    pub town: &'static str,
    pub at: (f32, f32),
    /// What they say when there's nothing else to talk about.
    pub greeting: &'static str,
}

/// A notice board that offers jobs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardDef {
    pub key: &'static str,
    pub name: &'static str,
    pub town: &'static str,
    pub at: (f32, f32),
    /// Jobs it offers at a time.
    pub offers: u8,
}

/// One thing a step asks for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Objective {
    /// Talk to someone.
    Talk { person: &'static str },
    /// Kill some of a monster, with at least a fair share of each fight (as for its XP).
    Kill { mob: &'static str, count: u16 },
    /// Gather some of an item from nodes or carcasses, while the step is under way.
    Gather { item: &'static str, count: u16 },
    /// Hand someone some of an item from the pack.
    Deliver {
        item: &'static str,
        count: u16,
        to: &'static str,
    },
    /// Go to a place, (x, z) in metres, named for the quest log.
    Reach {
        place: &'static str,
        at: (f32, f32),
        radius: f32,
    },
    /// Use an item from the pack at a place. It's used up.
    Use {
        item: &'static str,
        place: &'static str,
        at: (f32, f32),
        radius: f32,
    },
}

impl Objective {
    /// How many it takes.
    pub fn count(&self) -> u16 {
        match self {
            Objective::Kill { count, .. }
            | Objective::Gather { count, .. }
            | Objective::Deliver { count, .. } => *count,
            Objective::Talk { .. } | Objective::Reach { .. } | Objective::Use { .. } => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    /// The quest log's line for it.
    pub text: &'static str,
    /// What's said as it begins.
    #[serde(default)]
    pub say: &'static str,
    pub objectives: Vec<Objective>,
    /// Items handed over as it begins (for a later step to use or deliver).
    #[serde(default)]
    pub gives: Vec<(&'static str, u16)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, bound(deserialize = "'de: 'static"))]
pub struct Reward {
    pub coins: u32,
    /// Quest XP, for the player to give to any specialty (docs/PROGRESSION.md).
    pub xp: u32,
    pub items: Vec<(&'static str, u16)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestDef {
    pub key: &'static str,
    pub name: &'static str,
    /// Who offers it: one of the people.
    pub giver: &'static str,
    /// The level it's meant for. Anyone may start it.
    pub level: u8,
    /// Quests finished first.
    #[serde(default)]
    pub needs: Vec<&'static str>,
    /// What the giver asks, before it's started.
    pub offer: &'static str,
    pub steps: Vec<StepDef>,
    pub reward: Reward,
}

/// What a job asks for.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum JobKind {
    Kill {
        mob: &'static str,
    },
    /// Gathered after taking it.
    Gather {
        item: &'static str,
    },
    /// In the pack when handing it in, and handed over.
    Bring {
        item: &'static str,
    },
}

/// A template a board rolls jobs from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobDef {
    pub key: &'static str,
    pub board: &'static str,
    pub name: &'static str,
    /// The notice, with `{n}` for how many.
    pub text: &'static str,
    pub kind: JobKind,
    /// How many it may ask for, from the first to the second.
    pub count: (u16, u16),
    pub level: u8,
    /// Coins and quest XP for each one.
    pub pay: (u32, u32),
}

impl JobDef {
    /// The notice for `n`.
    pub fn notice(&self, n: u16) -> String {
        self.text.replace("{n}", &n.to_string())
    }
}

/// One table file. Every list is optional.
#[derive(Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, bound(deserialize = "'de: 'static"))]
pub struct Table {
    pub people: Vec<PersonDef>,
    pub boards: Vec<BoardDef>,
    pub quests: Vec<QuestDef>,
    pub jobs: Vec<JobDef>,
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
                "crates/content/quests/{file}:{}: {}",
                error.span.start, error.code
            ),
            LoadError::Problems(p) => {
                write!(f, "the quest tables don't add up:\n  {}", p.join("\n  "))
            }
        }
    }
}

impl std::error::Error for LoadError {}

pub struct Quests {
    pub people: Vec<PersonDef>,
    pub boards: Vec<BoardDef>,
    pub quests: Vec<QuestDef>,
    pub jobs: Vec<JobDef>,
    quest_ids: HashMap<&'static str, usize>,
}

impl Quests {
    pub fn load() -> Result<Quests, LoadError> {
        Quests::from_files(&FILES)
    }

    pub fn from_files(files: &[(&str, &'static str)]) -> Result<Quests, LoadError> {
        let mut all = Table::default();
        for (file, text) in files {
            let t: Table = ron::from_str(text).map_err(|error| LoadError::Syntax {
                file: file.to_string(),
                error: Box::new(error),
            })?;
            all.people.extend(t.people);
            all.boards.extend(t.boards);
            all.quests.extend(t.quests);
            all.jobs.extend(t.jobs);
        }
        let quest_ids = all
            .quests
            .iter()
            .enumerate()
            .map(|(i, q)| (q.key, i))
            .collect();
        let q = Quests {
            people: all.people,
            boards: all.boards,
            quests: all.quests,
            jobs: all.jobs,
            quest_ids,
        };
        let problems = q.problems();
        if problems.is_empty() {
            Ok(q)
        } else {
            Err(LoadError::Problems(problems))
        }
    }

    pub fn quest(&self, key: &str) -> Option<&QuestDef> {
        self.quest_ids.get(key).map(|&i| &self.quests[i])
    }

    pub fn person(&self, key: &str) -> Option<&PersonDef> {
        self.people.iter().find(|p| p.key == key)
    }

    pub fn board(&self, key: &str) -> Option<&BoardDef> {
        self.boards.iter().find(|b| b.key == key)
    }

    /// The job templates a board rolls from.
    pub fn jobs_for<'a>(&'a self, board: &'a str) -> impl Iterator<Item = &'a JobDef> + 'a {
        self.jobs.iter().filter(move |j| j.board == board)
    }

    /// Everything that doesn't add up: unknown keys, duplicates, impossible numbers, places
    /// outside the valley, and quests that need ones that come after them (so none can need
    /// itself, however far round).
    pub fn problems(&self) -> Vec<String> {
        let eco = economy();
        let cat = crate::combat::catalog();
        let mut out = Vec::new();
        let item = |out: &mut Vec<String>, what: &str, key: &str| {
            if eco.item(key).is_none() {
                out.push(format!("{what}: unknown item {key}"));
            }
        };
        let mob = |out: &mut Vec<String>, what: &str, key: &str| {
            if cat.mob(key).is_none() {
                out.push(format!("{what}: unknown monster {key}"));
            }
        };
        let person = |out: &mut Vec<String>, what: &str, key: &str| {
            if self.person(key).is_none() {
                out.push(format!("{what}: nobody called {key}"));
            }
        };
        let place = |out: &mut Vec<String>, what: &str, at: (f32, f32)| {
            if at.0.abs() >= 280.0 || at.1.abs() >= 280.0 {
                out.push(format!("{what}: {at:?} is outside the valley"));
            }
        };
        let town = |out: &mut Vec<String>, what: &str, key: &str| {
            if !eco.towns.iter().any(|t| t.key == key) {
                out.push(format!("{what}: no town {key}"));
            }
        };
        let mut seen = HashMap::new();
        for p in &self.people {
            let what = format!("person {}", p.key);
            if seen.insert(p.key, ()).is_some() {
                out.push(format!("{what} is defined twice"));
            }
            town(&mut out, &what, p.town);
            place(&mut out, &what, p.at);
        }
        for b in &self.boards {
            let what = format!("board {}", b.key);
            if seen.insert(b.key, ()).is_some() {
                out.push(format!("{what} is defined twice"));
            }
            town(&mut out, &what, b.town);
            place(&mut out, &what, b.at);
            if b.offers == 0 || self.jobs_for(b.key).next().is_none() {
                out.push(format!("{what} has nothing to offer"));
            }
        }
        for (i, q) in self.quests.iter().enumerate() {
            let what = format!("quest {}", q.key);
            if self.quests.iter().filter(|o| o.key == q.key).count() > 1 {
                out.push(format!("{what} is defined twice"));
            }
            person(&mut out, &what, q.giver);
            if q.level == 0 {
                out.push(format!("{what}: level 0"));
            }
            for n in &q.needs {
                if !self.quests[..i].iter().any(|o| o.key == *n) {
                    out.push(format!("{what}: needs {n}, which isn't a quest before it"));
                }
            }
            if q.steps.is_empty() {
                out.push(format!("{what} has no steps"));
            }
            for (s, step) in q.steps.iter().enumerate() {
                let what = format!("{what} step {}", s + 1);
                if step.objectives.is_empty() {
                    out.push(format!("{what} asks for nothing"));
                }
                for (it, n) in &step.gives {
                    item(&mut out, &what, it);
                    if *n == 0 {
                        out.push(format!("{what}: gives 0 {it}"));
                    }
                }
                for o in &step.objectives {
                    match o {
                        Objective::Talk { person: p } => person(&mut out, &what, p),
                        Objective::Kill { mob: m, .. } => mob(&mut out, &what, m),
                        Objective::Gather { item: it, .. } => item(&mut out, &what, it),
                        Objective::Deliver { item: it, to, .. } => {
                            item(&mut out, &what, it);
                            person(&mut out, &what, to);
                        }
                        Objective::Reach { at, radius, .. } => {
                            place(&mut out, &what, *at);
                            if *radius <= 0.0 {
                                out.push(format!("{what}: radius {radius}"));
                            }
                        }
                        Objective::Use {
                            item: it,
                            at,
                            radius,
                            ..
                        } => {
                            item(&mut out, &what, it);
                            place(&mut out, &what, *at);
                            if *radius <= 0.0 {
                                out.push(format!("{what}: radius {radius}"));
                            }
                        }
                    }
                    if o.count() == 0 {
                        out.push(format!("{what}: asks for 0"));
                    }
                }
            }
            for (it, n) in &q.reward.items {
                item(&mut out, &what, it);
                if *n == 0 {
                    out.push(format!("{what}: rewards 0 {it}"));
                }
            }
        }
        for j in &self.jobs {
            let what = format!("job {}", j.key);
            if self.jobs.iter().filter(|o| o.key == j.key).count() > 1 {
                out.push(format!("{what} is defined twice"));
            }
            if self.board(j.board).is_none() {
                out.push(format!("{what}: no board {}", j.board));
            }
            match j.kind {
                JobKind::Kill { mob: m } => mob(&mut out, &what, m),
                JobKind::Gather { item: it } | JobKind::Bring { item: it } => {
                    item(&mut out, &what, it)
                }
            }
            if j.count.0 == 0 || j.count.0 > j.count.1 {
                out.push(format!("{what}: count {:?}", j.count));
            }
            if !j.text.contains("{n}") {
                out.push(format!("{what}: the notice doesn't say how many"));
            }
            if j.level == 0 {
                out.push(format!("{what}: level 0"));
            }
        }
        out
    }
}

/// The built-in quest tables, loaded and checked on first use (panics on a bad table, which
/// this crate's tests catch).
pub fn quests() -> &'static Quests {
    static QUESTS: OnceLock<Quests> = OnceLock::new();
    QUESTS.get_or_init(|| Quests::load().unwrap_or_else(|e| panic!("{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_quest_tables_load_and_add_up() {
        let q = quests();
        assert!(!q.quests.is_empty() && !q.boards.is_empty());
        for quest in &q.quests {
            assert!(q.person(quest.giver).is_some());
        }
    }

    #[test]
    fn a_bad_table_says_what_is_wrong() {
        let bad: &'static str = r#"(
            people: [(key: "ann", name: "Ann", town: "brambleford", at: (0.0, 0.0), greeting: "Hello.")],
            quests: [
                (key: "a", name: "A", giver: "bob", level: 1, needs: ["b"], offer: "?", reward: (),
                    steps: [(text: "Go", objectives: [Kill(mob: "dragon", count: 0), Reach(place: "Far", at: (900.0, 0.0), radius: 1.0)])]),
                (key: "b", name: "B", giver: "ann", level: 1, offer: "?", reward: (items: [("nothing", 1)]), steps: []),
            ],
            jobs: [(key: "j", board: "none", name: "J", text: "Do it", kind: Kill(mob: "wolf"), count: (3, 1), level: 1, pay: (1, 1))],
        )"#;
        let Err(LoadError::Problems(p)) = Quests::from_files(&[("bad.ron", bad)]) else {
            panic!("a bad table loaded")
        };
        let all = p.join("\n");
        for want in [
            "nobody called bob",
            "needs b, which isn't a quest before it",
            "unknown monster dragon",
            "asks for 0",
            "outside the valley",
            "quest b has no steps",
            "unknown item nothing",
            "no board none",
            "count (3, 1)",
            "doesn't say how many",
        ] {
            assert!(all.contains(want), "missing {want:?} in:\n{all}");
        }
    }
}
