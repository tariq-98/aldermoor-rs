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
    /// Fur tones: the light and dark fur a body's `markings` mask places.
    #[serde(default)]
    pub light: Option<[f32; 3]>,
    #[serde(default)]
    pub dark: Option<[f32; 3]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dye {
    pub id: String,
    pub label: String,
    pub linear: [f32; 3],
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
    /// Fur bodies: a mask (R light fur, G dark fur) on the body's UVs.
    #[serde(default)]
    pub markings: Option<String>,
    /// Spring chains on the body itself (a tail), as for capes.
    #[serde(default)]
    pub springs: Option<Value>,
}

/// The curves a clip may carry, each a list of `[seconds, value]` keys (see [`curve`]):
/// - `off_hand`: how far the off hand is pulled onto the main item's `grips.off_hand` (0..1);
/// - `reach`: how far the drawing hand is pulled onto the stowed item (drawing and sheathing);
/// - `string`: the drawing hand holds the bowstring, so the bow's `draw` morph follows it;
/// - `arrow`: a nocked arrow shows (the bow's `arrow` morph).
pub const CURVES: [&str; 4] = ["off_hand", "reach", "string", "arrow"];

/// The moments a clip may mark: an item moves to the hand (`grab`) or back to where it rides
/// (`stow`); an arrow leaves (`release`); a blow lands (`hit`); a spell leaves (`cast`).
pub const EVENTS: [&str; 5] = ["grab", "stow", "release", "hit", "cast"];

#[derive(Debug, Clone, Deserialize)]
pub struct Event {
    pub t: f32,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Clip {
    pub name: String,
    pub seconds: f32,
    #[serde(rename = "loop")]
    pub looping: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    /// `"authored"` for clips built in `art/blender/build_clips.py`.
    #[serde(default)]
    pub source: Option<String>,
    /// Plays on the spine, arms and head only, over whatever the legs are doing.
    #[serde(default)]
    pub upper_body: bool,
    #[serde(default)]
    pub curves: BTreeMap<String, Vec<[f32; 2]>>,
    #[serde(default)]
    pub events: Vec<Event>,
}

impl Clip {
    pub fn curve(&self, name: &str, t: f32) -> f32 {
        self.curves.get(name).map(|k| curve(k, t)).unwrap_or(0.0)
    }

    pub fn event(&self, name: &str) -> Option<f32> {
        self.events.iter().find(|e| e.name == name).map(|e| e.t)
    }
}

/// The levels of detail beside every piece and item: `x.gltf` has `x.lod1.gltf` and `x.lod2.gltf`.
pub const LODS: usize = 2;

pub fn lod_path(file: &str, n: usize) -> String {
    match file.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.lod{n}.{ext}"),
        None => format!("{file}.lod{n}"),
    }
}

/// A clip curve's value at `t`: linear between keys, held before the first and after the last.
/// Two keys at the same time make a step; at that instant the later key wins.
pub fn curve(keys: &[[f32; 2]], t: f32) -> f32 {
    let Some(first) = keys.first() else { return 0.0 };
    if t < first[0] {
        return first[1];
    }
    for w in keys.windows(2) {
        let ([t0, v0], [t1, v1]) = (w[0], w[1]);
        if t0 <= t && t < t1 {
            return v0 + (v1 - v0) * (t - t0) / (t1 - t0);
        }
    }
    keys[keys.len() - 1][1]
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
    /// The idle to stand in while wearing this piece (robes hold the arms clear of the skirt).
    #[serde(default)]
    pub idle: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Humanoid {
    pub version: u32,
    pub regions: Vec<String>,
    pub skin_tones: Vec<Tone>,
    pub hair_colours: Vec<Tone>,
    /// The dye palette for pieces with a `dye` mask.
    #[serde(default)]
    pub dyes: Vec<Dye>,
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
    /// Where other hands hold the item, in its own glTF space.
    #[serde(default)]
    pub grips: BTreeMap<String, Frame>,
    /// How far along its up axis (metres, from its grip) a hand may take the item when drawing
    /// it from where it rides.
    #[serde(default)]
    pub grab_range: Option<[f32; 2]>,
    /// Bows: the string's rest point and its pull at full draw, for the `draw` morph.
    #[serde(default)]
    pub string: Option<BowString>,
    /// A dye mask (R main cloth, G trim) for dyeable back items (capes), as for pieces.
    #[serde(default)]
    pub dye: Option<String>,
}

/// A socket-like frame on an item: a hand's grip point, its up (the thumb side's direction)
/// and its front (the knuckles' direction), in the item's glTF space.
#[derive(Debug, Clone, Deserialize)]
pub struct Frame {
    pub at: [f32; 3],
    pub up: [f32; 3],
    pub front: [f32; 3],
    /// How far along `up` (metres, from the item's grip) the hand may slide to follow the clip.
    #[serde(default)]
    pub range: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BowString {
    pub rest: [f32; 3],
    pub pull: [f32; 3],
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

/// A piece of scenery: a building, tree, rock, plant or prop (`assets/catalog/environment.json`).
#[derive(Debug, Clone, Deserialize)]
pub struct Prop {
    pub id: String,
    pub label: String,
    /// house, landmark, bridge, tree, plant, rock, grass, flower, prop or part.
    pub kind: String,
    pub file: String,
    #[serde(default)]
    pub height: f32,
    #[serde(default)]
    pub triangles: u32,
    /// Width and depth at scale 1 (metres), for props the `world` crate treats as obstacles.
    #[serde(default)]
    pub footprint: Option<[f32; 2]>,
    /// Sways in the wind by its `_WIND` vertex attribute.
    #[serde(default)]
    pub wind: bool,
    /// Moving parts (the windmill's sails): a file placed at `at`, spinning about `spin.axis`.
    #[serde(default)]
    pub parts: Vec<PropPart>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PropPart {
    pub file: String,
    pub at: [f32; 3],
    #[serde(default)]
    pub spin: Option<Spin>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Spin {
    pub axis: [f32; 3],
    pub rpm: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Placed {
    pub prop: String,
    /// x and z on the ground (world metres); y comes from the terrain.
    pub at: [f32; 2],
    #[serde(default)]
    pub yaw: f32,
    #[serde(default = "one")]
    pub scale: f32,
}

fn one() -> f32 {
    1.0
}

/// Which props stand where `world::Valley` puts things.
#[derive(Debug, Clone, Deserialize)]
pub struct Placements {
    /// `Valley::houses[i]` takes `houses[i % len]`; trees likewise.
    pub houses: Vec<String>,
    pub trees: Vec<String>,
    pub bridge: String,
    #[serde(default)]
    pub landmarks: Vec<Placed>,
    #[serde(default)]
    pub dressing: Vec<Placed>,
    /// Props the renderer scatters itself, by kind.
    #[serde(default)]
    pub scatter: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvironmentCatalog {
    pub version: u32,
    pub props: Vec<Prop>,
    pub placements: Placements,
}

pub struct Catalog {
    pub root: PathBuf,
    pub humanoid: Humanoid,
    pub gear: Option<GearCatalog>,
    pub creatures: Option<CreatureCatalog>,
    pub environment: Option<EnvironmentCatalog>,
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
        let environment =
            if optional("environment.json").exists() { Some(read_json(&optional("environment.json"))?) } else { None };
        Ok(Catalog { root, humanoid, gear, creatures, environment })
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

        // Levels of detail: present, readable, and on the same skeleton as the full mesh.
        let check_lods = |what: &str, rel: &str, problem: &mut dyn FnMut(String)| {
            let Ok(full) = open(rel) else { return };
            if !full.morph_target_names().is_empty() {
                return; // Bows: no levels of detail.
            }
            for n in 1..=LODS {
                let lod = lod_path(rel, n);
                match open(&lod) {
                    Err(e) => problem(format!("{what}: level of detail {n}: {e}")),
                    Ok(g) => {
                        if g.joint_names() != full.joint_names() {
                            problem(format!("{what}: {lod} has a different skeleton from {rel}"));
                        }
                    }
                }
            }
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
                    if let Some(m) = &bt.markings {
                        if !self.root.join(m).exists() {
                            problem(format!("body type {id}: missing markings {m}"));
                        }
                    }
                    let chains = bt.springs.as_ref().and_then(|s| s.get("chains")).and_then(Value::as_array);
                    for bone in chains.into_iter().flatten().filter_map(Value::as_array).flatten().filter_map(Value::as_str) {
                        if !names.contains(bone) {
                            problem(format!("body type {id}: spring chain bone {bone} isn't in {}", bt.file));
                        }
                    }
                    skeletons.insert(id, names);
                }
            }
            check_lods(&format!("body type {id}"), &bt.file, &mut problem);
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
        for c in &h.animations.clips {
            for (name, keys) in &c.curves {
                if !CURVES.contains(&name.as_str()) {
                    problem(format!("clip {}: unknown curve {name}", c.name));
                }
                if keys.is_empty() || keys.windows(2).any(|w| w[1][0] < w[0][0]) {
                    problem(format!("clip {}: curve {name} needs keys in time order", c.name));
                }
                if keys.iter().any(|k| k[0] < 0.0 || k[0] > c.seconds + 1e-3 || !(0.0..=1.0).contains(&k[1])) {
                    problem(format!("clip {}: curve {name} has a key outside the clip or outside 0..1", c.name));
                }
            }
            for e in &c.events {
                if !EVENTS.contains(&e.name.as_str()) {
                    problem(format!("clip {}: unknown event {}", c.name, e.name));
                }
                if e.t < 0.0 || e.t > c.seconds + 1e-3 {
                    problem(format!("clip {}: event {} at {} is outside the clip", c.name, e.name, e.t));
                }
            }
            for (event, needs) in [("grab", "reach"), ("stow", "reach"), ("release", "string")] {
                if c.event(event).is_some() && !c.curves.contains_key(needs) {
                    problem(format!("clip {}: a {event} event without a {needs} curve", c.name));
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
                check_lods(&format!("piece {} ({bt})", p.id), rel, &mut problem);
            }
            if let Some(dye) = &p.dye {
                if !self.root.join(dye).exists() {
                    problem(format!("piece {}: missing dye mask {dye}", p.id));
                }
            }
            if let Some(idle) = &p.idle {
                if !h.animations.clips.iter().any(|c| &c.name == idle) {
                    problem(format!("piece {}: idle {idle} isn't in the library", p.id));
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
        if h.pieces.iter().any(|p| p.dye.is_some()) && h.dyes.is_empty() {
            problem("pieces have dye masks but the catalogue has no dyes".into());
        }
        for hair in &h.hair {
            for (bt, rel) in &hair.files {
                check_skinned("hair", &hair.id, bt, rel, false, &mut problem);
                check_lods(&format!("hair {} ({bt})", hair.id), rel, &mut problem);
            }
        }

        // Gear: files, sockets on the human frames, and the clips it names.
        if let Some(gear) = &self.gear {
            let human_sockets: BTreeSet<String> = ["male", "female"]
                .iter()
                .filter_map(|bt| skeletons.get(bt))
                .flat_map(|s| s.iter().filter(|n| n.starts_with("socket_")).cloned())
                .collect();
            let by_name: BTreeMap<&str, &Clip> = h.animations.clips.iter().map(|c| (c.name.as_str(), c)).collect();
            for item in &gear.items {
                for rel in item.file.iter().chain(item.sheath.iter()) {
                    if let Err(e) = open(rel) {
                        problem(format!("item {}: {e}", item.id));
                    }
                    check_lods(&format!("item {}", item.id), rel, &mut problem);
                }
                for (bt, rel) in &item.files {
                    check_skinned("item", &item.id, bt, rel, false, &mut problem);
                    check_lods(&format!("item {} ({bt})", item.id), rel, &mut problem);
                }
                if let Some(dye) = &item.dye {
                    if !self.root.join(dye).exists() {
                        problem(format!("item {}: missing dye mask {dye}", item.id));
                    }
                }
                for s in item.held.iter().chain(item.stowed.values()) {
                    if !human_sockets.contains(s) {
                        problem(format!("item {}: socket {s} isn't on the human skeletons", item.id));
                    }
                }
                for (role, v) in &item.clips {
                    let names: Vec<&str> = match v {
                        Value::String(s) => vec![s.as_str()],
                        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                        _ => vec![],
                    };
                    for n in names {
                        let Some(clip) = by_name.get(n) else {
                            problem(format!("item {}: clip {n} isn't in the library", item.id));
                            continue;
                        };
                        // What a clip asks of an item, the item must have.
                        if clip.curves.contains_key("off_hand") && !item.grips.contains_key("off_hand") {
                            problem(format!("item {}: clip {n} wants an off-hand grip the item doesn't give", item.id));
                        }
                        if clip.curves.contains_key("string") && item.string.is_none() {
                            problem(format!("item {}: clip {n} draws a bowstring the item lacks", item.id));
                        }
                        let wants = match role.as_str() {
                            "ready" => Some("grab"),
                            "stow" => Some("stow"),
                            _ => None,
                        };
                        if let Some(event) = wants {
                            if clip.event(event).is_none() {
                                problem(format!("item {}: its {role} clip {n} has no {event} event", item.id));
                            }
                        }
                    }
                }
                if item.string.is_some() {
                    match item.file.as_deref().map(open) {
                        Some(Ok(g)) => {
                            let morphs = g.morph_target_names();
                            for m in ["draw", "arrow"] {
                                if !morphs.contains(m) {
                                    problem(format!("item {}: bow lacks the {m} morph target", item.id));
                                }
                            }
                        }
                        Some(Err(_)) => {}
                        None => problem(format!("item {}: a bowstring on an item without a file", item.id)),
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
                check_lods(&format!("creature {}", cr.id), &cr.file, &mut problem);
            }
        }

        // Scenery: files, levels of detail, moving parts and the placements' props.
        if let Some(env) = &self.environment {
            let ids: BTreeSet<&str> = env.props.iter().map(|p| p.id.as_str()).collect();
            for prop in &env.props {
                if let Err(e) = open(&prop.file) {
                    problem(format!("prop {}: {e}", prop.id));
                }
                check_lods(&format!("prop {}", prop.id), &prop.file, &mut problem);
                for part in &prop.parts {
                    if let Err(e) = open(&part.file) {
                        problem(format!("prop {}: part {e}", prop.id));
                    }
                }
                if prop.wind {
                    if let Ok(g) = open(&prop.file) {
                        if !g.all_meshes_have("_WIND") {
                            problem(format!("prop {}: sways in the wind but a mesh lacks _WIND", prop.id));
                        }
                    }
                }
            }
            let pl = &env.placements;
            let named = pl.houses.iter().chain(&pl.trees).chain(std::iter::once(&pl.bridge))
                .chain(pl.landmarks.iter().chain(&pl.dressing).map(|p| &p.prop))
                .chain(pl.scatter.values().flatten());
            for id in named {
                if !ids.contains(id.as_str()) {
                    problem(format!("environment placements name {id}, which isn't a prop"));
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
        if let Some(e) = &self.environment {
            words.extend(e.props.iter().flat_map(|p| [p.id.clone(), p.label.clone(), p.kind.clone()]));
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
