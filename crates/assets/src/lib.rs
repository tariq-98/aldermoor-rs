//! The art catalogue: what the build scripts in `art/blender/` produce, as the game reads it.
//!
//! [`Catalog::load`] reads `assets/catalog/*.json`. [`Catalog::check`] verifies every file,
//! bone, socket, region and clip a catalogue entry names, so a broken export fails the tests
//! instead of the game. See `docs/ASSETS.md` for the conventions.

use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

mod gltf;

pub use gltf::Gltf;

/// The 65 bones every humanoid skeleton has (the Universal Animation Library's rig).
pub const HUMANOID_BONES: [&str; 65] = [
    "root", "pelvis", "spine_01", "spine_02", "spine_03", "neck_01", "Head",
    "clavicle_l", "upperarm_l", "lowerarm_l", "hand_l",
    "index_01_l", "index_02_l", "index_03_l", "index_04_leaf_l", "middle_01_l", "middle_02_l", "middle_03_l", "middle_04_leaf_l",
    "pinky_01_l", "pinky_02_l", "pinky_03_l", "pinky_04_leaf_l", "ring_01_l", "ring_02_l", "ring_03_l", "ring_04_leaf_l",
    "thumb_01_l", "thumb_02_l", "thumb_03_l", "thumb_04_leaf_l",
    "clavicle_r", "upperarm_r", "lowerarm_r", "hand_r",
    "index_01_r", "index_02_r", "index_03_r", "index_04_leaf_r", "middle_01_r", "middle_02_r", "middle_03_r", "middle_04_leaf_r",
    "pinky_01_r", "pinky_02_r", "pinky_03_r", "pinky_04_leaf_r", "ring_01_r", "ring_02_r", "ring_03_r", "ring_04_leaf_r",
    "thumb_01_r", "thumb_02_r", "thumb_03_r", "thumb_04_leaf_r",
    "thigh_l", "calf_l", "foot_l", "ball_l", "ball_leaf_l",
    "thigh_r", "calf_r", "foot_r", "ball_r", "ball_leaf_r",
];

/// Words that must never appear in the catalogue: Tariq has arachnophobia (docs/DESIGN.md).
pub const FORBIDDEN: [&str; 6] = ["spider", "arachn", "scorpion", "tarantula", "cobweb", "webbing"];

#[derive(Debug, Clone, Deserialize)]
pub struct Tone {
    pub id: String,
    pub linear: [f32; 3],
    #[serde(default)]
    pub races: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Collider {
    pub bone: String,
    pub radius: f32,
    pub a: [f32; 3],
    pub b: [f32; 3],
}

#[derive(Debug, Clone, Deserialize)]
pub struct BodyType {
    pub label: String,
    #[serde(default)]
    pub race: Option<String>,
    #[serde(default)]
    pub frame: Option<String>,
    pub file: String,
    pub pelvis: [f32; 3],
    pub height: f32,
    #[serde(default)]
    pub sockets: Vec<String>,
    #[serde(default)]
    pub colliders: Vec<Collider>,
    #[serde(default)]
    pub gear_scale: Option<f32>,
    #[serde(default)]
    pub creature: bool,
    #[serde(default)]
    pub dress: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Clip {
    pub name: String,
    pub seconds: f32,
    #[serde(rename = "loop")]
    pub looping: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Animations {
    pub file: String,
    pub pelvis: [f32; 3],
    pub fps: u32,
    pub clips: Vec<Clip>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Hair {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub files: BTreeMap<String, String>,
}

/// A skinned piece of clothing or armour. `hides[body type][target]` lists the regions it
/// drops on `target`: `"body"` or the id of a lower piece.
#[derive(Debug, Clone, Deserialize)]
pub struct Piece {
    pub id: String,
    pub slot: String,
    pub layer: f32,
    pub label: String,
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub hides: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub covers_hair: bool,
    /// A dye mask texture (R main cloth, G trim), for dyeable pieces.
    #[serde(default)]
    pub dye: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Humanoid {
    pub version: u32,
    pub regions: Vec<String>,
    pub skin_tones: Vec<Tone>,
    pub hair_colours: Vec<Tone>,
    pub body_types: BTreeMap<String, BodyType>,
    pub animations: Animations,
    pub hair: Vec<Hair>,
    pub pieces: Vec<Piece>,
    #[serde(default)]
    pub texture_variants: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub races: BTreeMap<String, Value>,
}

/// Rigid gear (`file`) or skinned back items (`files` per body type).
#[derive(Debug, Clone, Deserialize)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub family: String,
    pub slot: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub held: Option<String>,
    #[serde(default)]
    pub stowed: BTreeMap<String, String>,
    #[serde(default)]
    pub sheath: Option<String>,
    #[serde(default)]
    pub clips: BTreeMap<String, Value>,
    #[serde(default)]
    pub springs: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GearCatalog {
    pub version: u32,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Creature {
    pub id: String,
    pub label: String,
    pub family: String,
    pub file: String,
    pub clips: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatureCatalog {
    pub creatures: Vec<Creature>,
}

pub struct Catalog {
    pub root: PathBuf,
    pub humanoid: Humanoid,
    pub gear: Option<GearCatalog>,
    pub creatures: Option<CreatureCatalog>,
}

/// Something a catalogue entry promises that the files don't deliver.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem(pub String);

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Error> {
    let text = std::fs::read_to_string(path).map_err(|e| Error(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text).map_err(|e| Error(format!("{}: {e}", path.display())))
}

impl Catalog {
    /// Reads the catalogue under `root` (the repo's `assets/` folder).
    pub fn load(root: impl AsRef<Path>) -> Result<Catalog, Error> {
        let root = root.as_ref().to_path_buf();
        let humanoid = read_json(&root.join("catalog/humanoid.json"))?;
        let optional = |name: &str| root.join("catalog").join(name);
        let gear = if optional("gear.json").exists() { Some(read_json(&optional("gear.json"))?) } else { None };
        let creatures = if optional("creatures.json").exists() { Some(read_json(&optional("creatures.json"))?) } else { None };
        Ok(Catalog { root, humanoid, gear, creatures })
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Checks every promise the catalogue makes. An empty list means it's sound.
    pub fn check(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let mut problem = |s: String| out.push(Problem(s));
        let h = &self.humanoid;
        let regions: BTreeSet<&str> = h.regions.iter().map(String::as_str).collect();
        let clips: BTreeSet<&str> = h.animations.clips.iter().map(|c| c.name.as_str()).collect();
        let piece_ids: BTreeSet<&str> = h.pieces.iter().map(|p| p.id.as_str()).collect();
        // Each file is parsed once, however many entries name it.
        let cache: std::cell::RefCell<BTreeMap<String, Result<Gltf, String>>> = Default::default();
        let open = |rel: &str| -> Result<Gltf, String> {
            cache
                .borrow_mut()
                .entry(rel.to_string())
                .or_insert_with(|| Gltf::open(&self.root.join(rel)).map_err(|e| e.0))
                .clone()
        };

        // Bodies: the shared rig plus their sockets.
        let mut skeletons: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for (id, bt) in &h.body_types {
            match open(&bt.file) {
                Err(e) => problem(format!("body type {id}: {e}")),
                Ok(g) => {
                    let names = g.node_names();
                    for bone in HUMANOID_BONES {
                        if !names.contains(bone) {
                            problem(format!("body type {id}: skeleton lacks {bone}"));
                        }
                    }
                    for s in &bt.sockets {
                        if !names.contains(s.as_str()) {
                            problem(format!("body type {id}: lists socket {s} that {} lacks", bt.file));
                        }
                    }
                    for c in &bt.colliders {
                        if !names.contains(c.bone.as_str()) {
                            problem(format!("body type {id}: collider on missing bone {}", c.bone));
                        }
                    }
                    skeletons.insert(id, names);
                }
            }
        }
        match open(&h.animations.file) {
            Err(e) => problem(format!("animations: {e}")),
            Ok(g) => {
                let have = g.animation_names();
                for c in &h.animations.clips {
                    if !have.contains(&c.name) {
                        problem(format!("animations: catalogue lists clip {} that the file lacks", c.name));
                    }
                }
            }
        }

        // Skinned files: joints come from the body's skeleton, and pieces carry regions.
        let check_skinned = |kind: &str, id: &str, bt: &str, rel: &str, needs_regions: bool, problem: &mut dyn FnMut(String)| {
            let Some(skel) = skeletons.get(bt) else {
                problem(format!("{kind} {id}: file for unknown body type {bt}"));
                return;
            };
            match open(rel) {
                Err(e) => problem(format!("{kind} {id} ({bt}): {e}")),
                Ok(g) => {
                    for j in g.joint_names() {
                        if !skel.contains(&j) && !j.starts_with("cape_") {
                            problem(format!("{kind} {id} ({bt}): joint {j} isn't in the {bt} skeleton"));
                        }
                    }
                    if needs_regions && !g.all_skinned_have("_REGION") {
                        problem(format!("{kind} {id} ({bt}): a skinned primitive lacks _REGION"));
                    }
                }
            }
        };
        for p in &h.pieces {
            for (bt, rel) in &p.files {
                check_skinned("piece", &p.id, bt, rel, true, &mut problem);
            }
            if let Some(dye) = &p.dye {
                if !self.root.join(dye).exists() {
                    problem(format!("piece {}: missing dye mask {dye}", p.id));
                }
            }
            for (bt, targets) in &p.hides {
                for (target, list) in targets {
                    if target != "body" && !piece_ids.contains(target.as_str()) {
                        problem(format!("piece {} ({bt}): hides regions on unknown piece {target}", p.id));
                    }
                    for r in list {
                        if !regions.contains(r.as_str()) {
                            problem(format!("piece {} ({bt}): unknown region {r}", p.id));
                        }
                    }
                }
            }
        }
        for hair in &h.hair {
            for (bt, rel) in &hair.files {
                check_skinned("hair", &hair.id, bt, rel, false, &mut problem);
            }
        }

        // Gear: files, sockets on the human frames, and the clips it names.
        if let Some(gear) = &self.gear {
            let human_sockets: BTreeSet<String> = ["male", "female"]
                .iter()
                .filter_map(|bt| skeletons.get(bt))
                .flat_map(|s| s.iter().filter(|n| n.starts_with("socket_")).cloned())
                .collect();
            for item in &gear.items {
                for rel in item.file.iter().chain(item.sheath.iter()) {
                    if let Err(e) = open(rel) {
                        problem(format!("item {}: {e}", item.id));
                    }
                }
                for (bt, rel) in &item.files {
                    check_skinned("item", &item.id, bt, rel, false, &mut problem);
                }
                for s in item.held.iter().chain(item.stowed.values()) {
                    if !human_sockets.contains(s) {
                        problem(format!("item {}: socket {s} isn't on the human skeletons", item.id));
                    }
                }
                for v in item.clips.values() {
                    let names: Vec<&str> = match v {
                        Value::String(s) => vec![s.as_str()],
                        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                        _ => vec![],
                    };
                    for n in names {
                        if !clips.contains(n) {
                            problem(format!("item {}: clip {n} isn't in the library", item.id));
                        }
                    }
                }
            }
        }
        if let Some(c) = &self.creatures {
            for cr in &c.creatures {
                match open(&cr.file) {
                    Err(e) => problem(format!("creature {}: {e}", cr.id)),
                    Ok(g) => {
                        let have = g.animation_names();
                        for n in cr.clips.values() {
                            if !have.contains(n) {
                                problem(format!("creature {}: clip {n} isn't in its file", cr.id));
                            }
                        }
                    }
                }
            }
        }

        // The hard content rule.
        let mut words: Vec<String> = Vec::new();
        words.extend(h.pieces.iter().flat_map(|p| [p.id.clone(), p.label.clone()]));
        words.extend(h.body_types.iter().flat_map(|(id, b)| [id.clone(), b.label.clone()]));
        if let Some(g) = &self.gear {
            words.extend(g.items.iter().flat_map(|i| [i.id.clone(), i.label.clone(), i.family.clone()]));
        }
        if let Some(c) = &self.creatures {
            words.extend(c.creatures.iter().flat_map(|c| [c.id.clone(), c.label.clone(), c.family.clone()]));
        }
        for w in words {
            let lower = w.to_lowercase();
            if FORBIDDEN.iter().any(|f| lower.contains(f)) {
                problem(format!("forbidden content: {w}"));
            }
        }
        out
    }
}
