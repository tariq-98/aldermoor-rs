//! Catalogue assembly and animation. The workshop is the reference, not filenames or rig order.
use crate::model::*;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3, Vec3Swizzles};
use protocol::{Action, ForcedKind, Look, Pose, Traversal, UnitCombat, UnitState};
use std::{collections::HashMap, sync::Arc};

pub struct Part {
    pub appearance: crate::appearance::Appearance,
    pub model: Arc<Model>,
    pub hidden: u32,
    pub tint_skin: Option<[f32; 3]>,
    pub tint_hair: Option<[f32; 3]>,
    pub attachment: Option<Attachment>,
    pub remap: Vec<usize>,
    pub eyes_hidden: bool,
}
#[derive(Clone)]
pub struct Attachment {
    pub held: String,
    pub stowed: String,
    pub sheath: bool,
    pub always: bool,
    pub main: bool,
}
pub struct Draw {
    pub dye: Option<Arc<crate::appearance::Dye>>,
    pub base_map: Option<Arc<Texture>>,
    pub instance: u64,
    pub model: Arc<Model>,
    pub primitive: usize,
    pub transform: Mat4,
    pub palette: Arc<[Mat4]>,
    pub hidden: u32,
    pub base: [f32; 4],
    pub morph: [f32; 2],
}
pub struct Character {
    pub lod: usize,
    /// Diagnostic reference path for measuring and checking partial skeleton updates.
    pub partial_updates: bool,
    /// Whether full-detail feet go on the ground (#214); off to compare with the clips as they
    /// are.
    pub foot_placement: bool,
    pub parts: Vec<Part>,
    pub nodes: Vec<Node>,
    pub local: Vec<Transform>,
    pub world: Vec<Mat4>,
    pub names: HashMap<String, usize>,
    pub look: Look,
    pub clips: Arc<Model>,
    /// `clips` bound to this character's bones.
    clip_set: Arc<ClipSet>,
    /// The model whose skeleton this character was built on, which clip sets bind to.
    skeleton: Arc<Model>,
    pub clip_meta: HashMap<String, Arc<assets::Clip>>,
    pub rig_height: f32,
    pelvis_ratio: f32,
    reference_pelvis: Vec3,
    humanoid: bool,
    beast_speeds: [f32; 2],
    role_clips: HashMap<String, &'static str>,
    main: Option<Arc<assets::Item>>,
    /// The clips the weapon and the off-hand item play in place of the defaults.
    main_clips: ItemClips,
    off_hand_clips: ItemClips,
    combat: Option<Fight>,
    windup: Option<f32>,
    rally_time: Option<f32>,
    was_guarding: bool,
    idle: &'static str,
    gear_scale: f32,
    last_pose: Pose,
    /// How the body was getting about last frame, and the climbing move playing (#247), and
    /// whether its last shimmy went right (a wrap round a corner carries on that way).
    last_traversal: Traversal,
    climb: Option<ClimbMove>,
    shimmied_right: bool,
    attack: usize,
    transition: Option<(&'static str, f32)>,
    last_drawn: bool,
    /// Settles each switch of the clip stack into the next (#210, docs/ANIMATION.md).
    pub blender: anim::Blender,
    last_clip: &'static str,
    /// The legs' locomotion clip under an action on the move.
    last_legs: Option<&'static str>,
    /// Whether last frame's base clip was an action or another one-shot.
    last_busy: bool,
    /// Last frame's upper-body layer over the base clip: ready, stow, guard or a drawn idle.
    last_upper: Option<&'static str>,
    /// A switch the clips' names won't show: an action starting over, or a LOD swap.
    forced_switch: Option<anim::Switch>,
    /// Where the walk, jog or sprint cycle is, and the speed and hip turn the legs show (#110). A
    /// beast's phase runs from 0 to 1 through its walk or run clip.
    stride: anim::Stride,
    /// The humanoid loops' gaits on this skeleton, slowest first. Without them (beasts) the walk
    /// or run plays at `stride_rate`.
    gaits: Option<[anim::Gait; 3]>,
    /// The yaw the body is drawn at, which lags the sim's while turning on the spot (#211), and
    /// the feet stepping round after it.
    turn: anim::Turn,
    steps: Option<anim::Steps>,
    /// The left and right legs' hip, knee and ankle bones, which step round and reach for the
    /// ground, and the balls of the feet.
    leg_bones: Option<[[usize; 3]; 2]>,
    ball_bones: Option<[usize; 2]>,
    /// The feet on the ground (#214): where each is held, and how far the pelvis drops.
    feet: anim::Feet,
    ground: Option<Ground>,
    /// The pelvis and the spine up to the chest, which turn toward the travel and the aim.
    spine_bones: Option<[usize; 4]>,
    /// The neck and head, which share the turn the chest can't take.
    head_bones: [Option<usize>; 2],
    /// Each arm's upper arm, forearm, hand and hand socket, by `Side`, which reach for the
    /// weapon.
    arm_bones: [Option<[usize; 4]>; 2],
    /// Where the weapon rides drawn and stowed, if it has a mesh.
    main_sockets: Option<[usize; 2]>,
    drawn_now: bool,
    main_offset: f32,
    grab_offset: Option<f32>,
    bow_morph: [f32; 2],
    springs: Vec<crate::springs::SpringBones>,
    /// How often the pose is rebuilt (#215), and the time since it last was.
    tier: anim::Tier,
    since: f32,
    /// At `Tier::Half`, the last two rebuilds, shown blended between them.
    between: Option<Between>,
    children: Vec<Vec<usize>>,
    /// Every node after its parent, to rebuild world matrices in one pass.
    order: Vec<usize>,
    /// The bone the feet last dropped (the pelvis, #214) and everything on it, parents first, and
    /// how many nodes there were when they were listed.
    dropped: (usize, Vec<usize>),
    lod_parts: HashMap<usize, Stashed>,
}
/// What `set_lod` keeps of a LOD it swapped out: its parts, its clips and their binding.
type Stashed = (Vec<Part>, Arc<Model>, Arc<ClipSet>);
/// The height of the ground at any (x, z), which full-detail characters put their feet on (#214):
/// `world::Valley::ground` in the game.
pub type Ground = Arc<dyn Fn(f32, f32) -> f32 + Send + Sync>;

/// A climbing clip under way (#247): the sim's move onto, between or off holds that it plays,
/// and how far into that move the renderer has got.
struct ClimbMove {
    clip: &'static str,
    /// The move's start, in sim time: a new start is a new move.
    start: f64,
    time: f32,
    duration: f32,
}

/// Which bones a clip drives. The upper body is `spine_01` and everything under it.
#[derive(Clone, Copy, PartialEq)]
enum Bones {
    All,
    Upper,
    Lower,
}
/// The clips an item plays in place of the defaults (its catalogue `clips`), by role: one clip,
/// or a list such as `attacks`.
#[derive(Default)]
struct ItemClips(HashMap<String, Box<[&'static str]>>);
impl ItemClips {
    fn new(item: &assets::Item) -> Self {
        use serde_json::Value;
        let names = |v: &Value| match v {
            Value::Array(a) => a.iter().filter_map(Value::as_str).map(intern).collect(),
            v => v.as_str().map(intern).into_iter().collect(),
        };
        let roles = item.clips.iter().map(|(role, v)| (role.clone(), names(v)));
        Self(roles.collect())
    }
    /// The clip for `role`, or the first of its list.
    fn get(&self, role: &str) -> Option<&'static str> {
        self.list(role).first().copied()
    }
    fn list(&self, role: &str) -> &[&'static str] {
        self.0.get(role).map_or(&[], |names| names)
    }
}
/// A half-rate character's last two rebuilds (#215): their world matrices and body yaws, and the
/// time between them. The frames between show them blended, a rebuild behind.
struct Between {
    from: Vec<Mat4>,
    to: Vec<Mat4>,
    yaws: [f32; 2],
    /// The body yaw shown now.
    yaw: f32,
    interval: f32,
    /// Off while the character is at another tier, so that coming back it starts again from the
    /// pose shown then.
    live: bool,
}
impl Between {
    fn new(world: &[Mat4]) -> Self {
        Self {
            from: world.to_vec(),
            to: world.to_vec(),
            yaws: [0.; 2],
            yaw: 0.,
            interval: 1. / 30.,
            live: false,
        }
    }
}
/// What the pose goes by in a unit's `UnitCombat`, copied each frame without its names and auras.
#[derive(Clone, Copy)]
struct Fight {
    action: Action,
    action_time: f32,
    /// The cast's length, and whether it is channelled.
    cast: Option<(f32, bool)>,
    guarding: bool,
    downed: bool,
}
impl From<&UnitCombat> for Fight {
    fn from(c: &UnitCombat) -> Self {
        Self {
            action: c.action,
            action_time: c.action_time,
            cast: c.cast.as_ref().map(|c| (c.duration, c.channel)),
            guarding: c.guarding,
            downed: c.downed,
        }
    }
}
/// A hand, which `perform` reaches with; it indexes `arm_bones`.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
}
impl Side {
    fn other(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}
/// How far round from the hips an action on the move may turn the chest, beyond the clip's own
/// twist, to face where the clip faced it.
const MAX_TWIST: f32 = std::f32::consts::FRAC_PI_3;
/// How much further round than the chest the neck and head turn, shared between them, when the
/// chest can't turn as far as it should face (#211).
const HEAD_TWIST: f32 = std::f32::consts::FRAC_PI_6;
/// The humanoid locomotion loops, slowest first, which blend by speed (#110).
const LOOPS: [&str; 3] = ["Walk_Loop", "Jog_Fwd_Loop", "Sprint_Loop"];
/// The humanoid loop for walking or running at `speed`, which the clip stack's switches and
/// `perform` go by. What shows is a blend of `LOOPS`.
fn locomotion_clip(pose: Pose, speed: f32) -> &'static str {
    match pose {
        Pose::Walk => "Walk_Loop",
        _ if speed > 7. => "Sprint_Loop",
        _ => "Jog_Fwd_Loop",
    }
}
/// How far round the vertical a bone at model-space `rotation` has turned from its `rest`: the
/// twist about +Y of the change, which stays steady however far the bone leans.
fn heading(rotation: Quat, rest: Quat) -> f32 {
    let change = rotation * rest.inverse();
    2. * change.y.atan2(change.w)
}
fn wrap(angle: f32) -> f32 {
    (angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
/// Node indices with every parent before its children.
fn parent_first(nodes: &[Node]) -> Vec<usize> {
    fn place(i: usize, nodes: &[Node], placed: &mut [bool], order: &mut Vec<usize>) {
        if placed[i] {
            return;
        }
        if let Some(p) = nodes[i].parent {
            place(p, nodes, placed, order);
        }
        placed[i] = true;
        order.push(i);
    }
    let mut order = Vec::with_capacity(nodes.len());
    let mut placed = vec![false; nodes.len()];
    for i in 0..nodes.len() {
        place(i, nodes, &mut placed, &mut order);
    }
    order
}
/// The kind of switch from pose `from` to `to`, for how fast it settles. `busy` says an action or
/// another one-shot (an attack, cast, roll, hit, death) drives the base clip.
fn switch_kind(from: Pose, from_busy: bool, to: Pose, to_busy: bool) -> anim::Switch {
    let airborne = |pose| matches!(pose, Pose::Jump | Pose::Fall);
    if airborne(from) && !airborne(to) {
        anim::Switch::Land
    } else if to_busy || to == Pose::Jump {
        anim::Switch::Action
    } else if from_busy {
        anim::Switch::Recover
    } else {
        anim::Switch::Locomotion
    }
}

pub fn region_mask(names: &[String], regions: &[String]) -> u32 {
    names
        .iter()
        .filter_map(|n| regions.iter().position(|r| r == n))
        .filter(|i| *i < 31)
        .fold(0, |m, i| m | (1 << i))
}
/// A covering item only hides geometry on strictly lower layers.
pub fn hidden_regions(
    target: &str,
    layer: f32,
    body: &str,
    coverings: &[(String, f32, serde_json::Value)],
    regions: &[String],
) -> u32 {
    coverings
        .iter()
        .filter(|(_, l, _)| *l > layer)
        .filter_map(|(_, _, h)| h.get(body)?.get(target)?.as_array())
        .flatten()
        .filter_map(|v| v.as_str())
        .filter_map(|n| regions.iter().position(|r| r == n))
        .filter(|i| *i < 31)
        .fold(0, |m, i| m | (1 << i))
}
impl Character {
    pub fn load(catalog: &assets::Catalog, cache: &mut ModelCache, look: &Look) -> Result<Self> {
        Self::load_lod(catalog, cache, look, 0)
    }
    pub fn load_lod(
        catalog: &assets::Catalog,
        cache: &mut ModelCache,
        look: &Look,
        lod: usize,
    ) -> Result<Self> {
        let h = &catalog.humanoid;
        let raw = cache.metadata(&catalog.path("catalog/humanoid.json"))?;
        let raw_gear = cache.metadata(&catalog.path("catalog/gear.json"))?;
        let beast = catalog
            .creatures
            .as_ref()
            .and_then(|c| c.creatures.iter().find(|c| c.id == look.art));
        let body = h.body_types.get(&look.art);
        ensure!(
            body.is_some() || beast.is_some(),
            "unknown art id {}",
            look.art
        );
        let body_file = body
            .map(|b| b.file.as_str())
            .unwrap_or_else(|| &beast.unwrap().file);
        let model = cache.get_lod(&catalog.path(body_file), lod)?;
        let nodes = model.nodes.clone();
        let names: HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.name.clone(), i))
            .collect();
        let local = nodes.iter().map(|n| n.rest).collect::<Vec<_>>();
        let order = parent_first(&nodes);
        let clips = if body.is_some() {
            cache.get(&catalog.path(&h.animations.file))?
        } else {
            model.clone()
        };
        let reference_pelvis = clips
            .nodes
            .iter()
            .find(|n| n.name == "pelvis")
            .map_or(Vec3::ZERO, |n| n.rest.translation);
        let clip_set = cache.clip_set(&model, &clips, body.is_some());
        let leg = |bones: [&str; 3]| {
            let [hip, knee, ankle] = bones.map(|n| names.get(n).copied());
            let [hip, knee, ankle] = [hip?, knee?, ankle?];
            // The IK bends a chain: each bone the parent of the next.
            (nodes[knee].parent == Some(hip) && nodes[ankle].parent == Some(knee))
                .then_some([hip, knee, ankle])
        };
        let leg_bones = leg(["thigh_l", "calf_l", "foot_l"])
            .zip(leg(["thigh_r", "calf_r", "foot_r"]))
            .map(|(left, right)| [left, right]);
        let ball_bones = leg_bones.and_then(|[left, right]| {
            let ball = |name: &str, foot: usize| {
                names
                    .get(name)
                    .copied()
                    .filter(|&b| nodes[b].parent == Some(foot))
            };
            Some([ball("ball_l", left[2])?, ball("ball_r", right[2])?])
        });
        let all = |bones: [&str; 4]| match bones.map(|n| names.get(n).copied()) {
            [Some(a), Some(b), Some(c), Some(d)] => Some([a, b, c, d]),
            _ => None,
        };
        let spine_bones = all(["pelvis", "spine_01", "spine_02", "spine_03"]);
        let arm_bones = [
            all(["upperarm_l", "lowerarm_l", "hand_l", "socket_hand_l"]),
            all(["upperarm_r", "lowerarm_r", "hand_r", "socket_hand_r"]),
        ];
        let head_bones = ["neck_01", "Head"].map(|n| names.get(n).copied());
        let mut c = Self {
            lod,
            partial_updates: true,
            foot_placement: true,
            parts: Vec::new(),
            world: world_matrices(&nodes, &local),
            nodes,
            names,
            local,
            look: look.clone(),
            clips,
            clip_set,
            skeleton: model.clone(),
            clip_meta: h
                .animations
                .clips
                .iter()
                .map(|c| (c.name.clone(), Arc::new(c.clone())))
                .collect(),
            rig_height: body.map_or(1., |b| b.height),
            pelvis_ratio: body.map_or(1., |b| b.pelvis[1] / h.animations.pelvis[1]),
            reference_pelvis,
            humanoid: body.is_some(),
            beast_speeds: [1.; 2],
            role_clips: beast
                .into_iter()
                .flat_map(|b| &b.clips)
                .map(|(role, name)| (role.clone(), intern(name)))
                .collect(),
            main: None,
            main_clips: ItemClips::default(),
            off_hand_clips: look
                .gear
                .iter()
                .find(|(slot, _)| slot == "off_hand")
                .and_then(|(_, id)| catalog.gear.as_ref()?.items.iter().find(|i| i.id == *id))
                .map(ItemClips::new)
                .unwrap_or_default(),
            combat: None,
            windup: None,
            rally_time: None,
            was_guarding: false,
            idle: "Idle_Loop",
            gear_scale: body.and_then(|b| b.gear_scale).unwrap_or(1.),
            last_pose: Pose::Idle,
            last_traversal: Traversal::Ground,
            climb: None,
            shimmied_right: true,
            attack: 0,
            transition: None,
            last_drawn: look.drawn,
            blender: anim::Blender::default(),
            last_clip: "",
            last_legs: None,
            last_busy: false,
            last_upper: None,
            forced_switch: None,
            stride: anim::Stride::default(),
            gaits: None,
            turn: anim::Turn::default(),
            steps: None,
            leg_bones,
            ball_bones,
            feet: anim::Feet::default(),
            ground: None,
            spine_bones,
            head_bones,
            arm_bones,
            main_sockets: None,
            drawn_now: look.drawn,
            main_offset: 0.,
            grab_offset: None,
            bow_morph: [0.; 2],
            springs: Vec::new(),
            tier: anim::Tier::Full,
            since: 0.,
            between: None,
            children: Vec::new(),
            dropped: (0, Vec::new()),
            order,
            lod_parts: HashMap::new(),
        };
        if beast.is_some() {
            let raw_creatures = cache.metadata(&catalog.path("catalog/creatures.json"))?;
            let def = raw_creatures["creatures"]
                .as_array()
                .and_then(|a| a.iter().find(|d| d["id"] == look.art))
                .context("missing beast metadata")?;
            c.rig_height = def["height"].as_f64().context("beast lacks height")? as f32;
            c.beast_speeds = [
                def["speeds"]["walk"]
                    .as_f64()
                    .context("beast lacks walk speed")? as f32,
                def["speeds"]["run"]
                    .as_f64()
                    .context("beast lacks run speed")? as f32,
            ];
            c.add_part(model, 0, None, None, None, false)?;
            let idle = *c
                .role_clips
                .get("idle")
                .context("beast must supply an idle clip")?;
            ensure!(
                c.clips.clips.contains_key(idle),
                "beast idle clip is missing"
            );
            c.apply_clip(idle, 0., true, Bones::All);
            c.world = world_matrices(&c.nodes, &c.local);
            c.last_clip = idle;
            return Ok(c);
        }
        let defaults = &raw["defaults"][&look.art];
        let tone_id = look.tone.as_deref().or_else(|| defaults["tone"].as_str());
        let hair_id = look
            .hair_colour
            .as_deref()
            .or_else(|| defaults["hair_colour"].as_str());
        let skin = h
            .skin_tones
            .iter()
            .find(|t| Some(t.id.as_str()) == tone_id)
            .map(|t| t.linear);
        let hair = h
            .hair_colours
            .iter()
            .find(|t| Some(t.id.as_str()) == hair_id)
            .map(|t| t.linear);
        let pieces: Vec<&assets::Piece> = look
            .pieces
            .iter()
            .map(|id| {
                h.pieces
                    .iter()
                    .find(|p| p.id == *id)
                    .with_context(|| format!("unknown clothing {id}"))
            })
            .collect::<Result<_>>()?;
        let items: Vec<(&str, &assets::Item)> = look
            .gear
            .iter()
            .map(|(slot, id)| {
                Ok((
                    slot.as_str(),
                    catalog
                        .gear
                        .as_ref()
                        .and_then(|g| g.items.iter().find(|i| i.id == *id))
                        .with_context(|| format!("unknown gear {id}"))?,
                ))
            })
            .collect::<Result<_>>()?;
        let item_json = |id: &str| {
            raw_gear["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["id"] == id)
                .cloned()
                .unwrap_or_default()
        };
        let mut covering: Vec<_> = pieces
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    p.layer,
                    serde_json::to_value(&p.hides).unwrap(),
                )
            })
            .collect();
        for (_, i) in &items {
            if !i.files.is_empty() {
                let j = item_json(&i.id);
                covering.push((
                    i.id.clone(),
                    j["layer"].as_f64().unwrap_or(4.) as f32,
                    j["hides"].clone(),
                ));
            }
        }
        let mask =
            |id: &str, layer: f32| hidden_regions(id, layer, &look.art, &covering, &h.regions);
        let body_mask = mask("body", 0.);
        let face = region_mask(&["face".into()], &h.regions) & body_mask != 0;
        c.add_part(model, body_mask, skin, hair, None, face)?;
        for p in &pieces {
            let f = p
                .files
                .get(&look.art)
                .with_context(|| format!("{} has no fit for {}", p.id, look.art))?;
            c.add_part(
                cache.get_lod(&catalog.path(f), lod)?,
                mask(&p.id, p.layer),
                skin,
                hair,
                None,
                false,
            )?;
            let def = raw["pieces"]
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["id"] == p.id)
                .context("missing piece metadata")?;
            c.set_appearance(catalog, cache, look, &p.id, def)?;
            if let Some(i) = &p.idle {
                c.idle = intern(i);
            }
        }
        let covers_hair = pieces.iter().any(|p| p.covers_hair);
        for (kind, id) in [
            ("hair", &look.hair),
            ("brows", &look.brows),
            ("beard", &look.beard),
        ] {
            if (kind == "hair" && covers_hair) || (kind != "hair" && face) {
                continue;
            }
            if let Some(id) = id {
                let p = h
                    .hair
                    .iter()
                    .find(|p| p.id == *id)
                    .with_context(|| format!("unknown hair {id}"))?;
                let f = p.files.get(&look.art).context("missing hair fit")?;
                c.add_part(
                    cache.get_lod(&catalog.path(f), lod)?,
                    0,
                    skin,
                    hair,
                    None,
                    false,
                )?;
            }
        }
        let back_kind = items
            .iter()
            .find(|(s, _)| *s == "back")
            .and_then(|(_, i)| item_json(&i.id)["back_kind"].as_str().map(str::to_owned))
            .unwrap_or_default();
        for (slot, i) in items {
            let stowed = i
                .stowed
                .get(&back_kind)
                .or_else(|| i.stowed.get("default"))
                .cloned()
                .or_else(|| i.held.clone())
                .unwrap_or_default();
            let held = i.held.clone().unwrap_or_else(|| stowed.clone());
            let attachment = Attachment {
                held,
                stowed,
                sheath: false,
                always: slot == "back" || slot == "ammo",
                main: slot == "main_hand",
            };
            if slot == "main_hand" {
                c.main = Some(Arc::new(i.clone()));
                c.main_clips = ItemClips::new(i);
            }
            if let Some(f) = &i.file {
                ensure!(
                    c.names.contains_key(&attachment.held)
                        && c.names.contains_key(&attachment.stowed),
                    "missing equipment socket for {}",
                    i.id
                );
                if attachment.main {
                    let sockets = [&attachment.held, &attachment.stowed];
                    c.main_sockets = Some(sockets.map(|n| c.names[n]));
                }
                c.add_part(
                    cache.get_lod(&catalog.path(f), lod)?,
                    0,
                    None,
                    None,
                    Some(attachment.clone()),
                    false,
                )?;
            }
            if let Some(f) = &i.sheath {
                let mut a = attachment.clone();
                a.sheath = true;
                c.add_part(
                    cache.get_lod(&catalog.path(f), lod)?,
                    0,
                    None,
                    None,
                    Some(a),
                    false,
                )?;
            }
            if let Some(f) = i.files.get(&look.art) {
                let j = item_json(&i.id);
                c.add_part(
                    cache.get_lod(&catalog.path(f), lod)?,
                    mask(&i.id, j["layer"].as_f64().unwrap_or(4.) as f32),
                    skin,
                    hair,
                    None,
                    false,
                )?;
                c.set_appearance(catalog, cache, look, &i.id, &j)?;
            }
        }
        let set = c.clip_set.clone();
        c.gaits = *set.gaits.get_or_init(|| c.measure_gaits());
        for (n, l) in c.nodes.iter().zip(&mut c.local) {
            *l = n.rest;
        }
        c.world = world_matrices(&c.nodes, &c.local);
        for (_, id) in &look.gear {
            if let Some(item) = catalog
                .gear
                .as_ref()
                .and_then(|g| g.items.iter().find(|i| i.id == *id))
                && let Some(spec) = &item.springs
            {
                c.springs.push(crate::springs::SpringBones::new(
                    &c,
                    spec,
                    &body.unwrap().colliders,
                ));
            }
        }
        Ok(c)
    }
    fn set_appearance(
        &mut self,
        catalog: &assets::Catalog,
        cache: &mut ModelCache,
        look: &Look,
        id: &str,
        def: &serde_json::Value,
    ) -> Result<()> {
        let part = self.parts.last_mut().context("no part for appearance")?;
        part.appearance = crate::appearance::Appearance::load(
            catalog,
            cache,
            look,
            id,
            def,
            part.model
                .primitives
                .iter()
                .filter_map(|p| p.material.maps[0].clone()),
        )?;
        Ok(())
    }
    fn add_part(
        &mut self,
        model: Arc<Model>,
        hidden: u32,
        skin: Option<[f32; 3]>,
        hair: Option<[f32; 3]>,
        attachment: Option<Attachment>,
        eyes_hidden: bool,
    ) -> Result<()> {
        // Register a piece's extra bones recursively, preserving each local rest transform.
        fn bind(i: usize, m: &Model, c: &mut Character) -> usize {
            if let Some(n) = c.names.get(&m.nodes[i].name) {
                return *n;
            }
            let parent = m.nodes[i].parent.map(|p| bind(p, m, c));
            let idx = c.nodes.len();
            let mut node = m.nodes[i].clone();
            node.parent = parent;
            c.names.insert(node.name.clone(), idx);
            c.local.push(node.rest);
            c.nodes.push(node);
            idx
        }
        let mut remap = vec![usize::MAX; model.nodes.len()];
        for s in &model.skins {
            for &i in &s.joints {
                remap[i] = bind(i, &model, self);
            }
        }
        if self.order.len() != self.nodes.len() {
            // A piece brought bones of its own (a cape's): the world matrices cover them too.
            self.order = parent_first(&self.nodes);
        }
        self.parts.push(Part {
            appearance: Default::default(),
            model,
            hidden,
            tint_skin: skin,
            tint_hair: hair,
            attachment,
            remap,
            eyes_hidden,
        });
        Ok(())
    }
    /// Swap only geometry and beast clip data; the live rig, blends, draws and springs survive.
    pub fn set_lod(
        &mut self,
        catalog: &assets::Catalog,
        cache: &mut ModelCache,
        lod: usize,
    ) -> Result<()> {
        if lod == self.lod {
            return Ok(());
        }
        let (parts, clips, clip_set) = if let Some(parts) = self.lod_parts.remove(&lod) {
            parts
        } else {
            let mut next = Self::load_lod(catalog, cache, &self.look, lod)?;
            for part in &mut next.parts {
                for index in &mut part.remap {
                    if *index != usize::MAX {
                        *index = *self
                            .names
                            .get(&next.nodes[*index].name)
                            .context("LOD changed skeleton")?;
                    }
                }
            }
            // A beast's clips come with each LOD's model; they still drive this skeleton.
            let clip_set = cache.clip_set(&self.skeleton, &next.clips, self.humanoid);
            (next.parts, next.clips, clip_set)
        };
        self.lod_parts.insert(
            self.lod,
            (
                std::mem::replace(&mut self.parts, parts),
                std::mem::replace(&mut self.clips, clips),
                std::mem::replace(&mut self.clip_set, clip_set),
            ),
        );
        self.lod = lod;
        self.forced_switch = Some(anim::Switch::Detail);
        Ok(())
    }
    /// Rebuilds every world matrix from the local pose.
    fn pose_world(&mut self) {
        for &i in &self.order {
            let local = self.local[i].matrix();
            self.world[i] = match self.nodes[i].parent {
                Some(p) => self.world[p] * local,
                None => local,
            };
        }
    }
    /// Lists each node's children, once there are as many nodes as there will be.
    fn find_children(&mut self) {
        if self.children.len() != self.nodes.len() {
            self.children = vec![Vec::new(); self.nodes.len()];
            for (i, n) in self.nodes.iter().enumerate() {
                if let Some(p) = n.parent {
                    self.children[p].push(i);
                }
            }
        }
    }
    pub(crate) fn update_chain(&mut self, root: usize) {
        if !self.partial_updates {
            self.pose_world();
            return;
        }
        self.find_children();
        fn update(
            i: usize,
            nodes: &[Node],
            local: &[Transform],
            world: &mut [Mat4],
            children: &[Vec<usize>],
        ) {
            world[i] = nodes[i].parent.map_or(Mat4::IDENTITY, |p| world[p]) * local[i].matrix();
            for &child in &children[i] {
                update(child, nodes, local, world, children);
            }
        }
        update(
            root,
            &self.nodes,
            &self.local,
            &mut self.world,
            &self.children,
        );
    }
    fn item_clip(&self, key: &str, fallback: &'static str) -> &'static str {
        self.main_clips.get(key).unwrap_or(fallback)
    }
    fn shield_clip(&self, key: &str, fallback: &'static str) -> &'static str {
        self.off_hand_clips
            .get(key)
            .unwrap_or_else(|| self.item_clip(key, fallback))
    }
    fn attack_clip(&self, index: usize) -> &'static str {
        let attacks = self.main_clips.list("attacks");
        attacks
            .get(index.min(attacks.len().saturating_sub(1)))
            .copied()
            .unwrap_or("Punch_Jab")
    }
    fn action_clip(&self, u: &UnitState) -> Option<&'static str> {
        let c = self.combat.as_ref()?;
        if c.downed {
            return if self.humanoid {
                Some("Death01")
            } else {
                self.role_clips.get("death").copied()
            };
        }
        if self.rally_time.is_some() && self.humanoid {
            return Some("LayToIdle");
        }
        if c.guarding || c.action == Action::Block {
            return None;
        } // upper-body layer, keep the legs moving
        if !matches!(
            u.pose,
            Pose::Attack | Pose::Cast | Pose::Hit | Pose::Jump | Pose::Run
        ) {
            return None;
        }
        if u.pose == Pose::Run && c.action != Action::Charge
            || u.pose == Pose::Jump && c.action != Action::Leap
            || u.pose == Pose::Hit && c.action != Action::Stagger
        {
            return None;
        }
        if !self.humanoid {
            let role = match c.action {
                Action::Bite | Action::Claw => "attack",
                Action::Roar => "special",
                Action::Leap => "jump",
                _ => return None,
            };
            return self.role_clips.get(role).copied();
        }
        let family = self.main.as_ref().map_or("", |i| i.family.as_str());
        Some(match c.action {
            Action::Slash => self.attack_clip(0),
            Action::Slash2 => self.attack_clip(1),
            Action::Thrust => {
                if family == "spear" {
                    "Spear_Thrust"
                } else {
                    self.attack_clip(2)
                }
            }
            Action::Overhead | Action::Slam => {
                if family == "greatsword" {
                    "GreatSword_Overhead"
                } else {
                    "Sword_Overhead"
                }
            }
            Action::Spin => {
                if family == "greatsword" {
                    "GreatSword_Swing"
                } else {
                    "Sword_Regular_Combo"
                }
            }
            Action::Bash => self.shield_clip("bash", "Punch_Jab"),
            Action::Parry => "Sword_Block",
            Action::Shoot => match family {
                "bow" => "Bow_Shoot",
                "wand" => "Spell_Simple_Shoot",
                _ => "OverhandThrow",
            },
            Action::CastBolt => self.attack_clip(0),
            Action::CastArea => self.item_clip("slam", "Spell_Simple_Shoot"),
            Action::Channel => self.item_clip("channel", "Spell_Simple_Idle_Loop"),
            Action::CastSelf => {
                if c.cast.is_some() {
                    "Spell_Simple_Enter"
                } else {
                    "Spell_Simple_Exit"
                }
            }
            Action::Kick => "Punch_Cross",
            Action::Leap => self.shield_clip("dash", "NinjaJump_Start"),
            Action::Charge => self.item_clip(
                "dash",
                if u.velocity.xz().length() > 7. {
                    "Sprint_Loop"
                } else {
                    "Jog_Fwd_Loop"
                },
            ),
            Action::Roar => "Spell_Simple_Enter",
            Action::Stagger => {
                if self.was_guarding {
                    "Idle_Shield_Break"
                } else {
                    "Hit_Knockback"
                }
            }
            Action::Revive => "Fixing_Kneeling",
            _ => return None,
        })
    }
    fn clip_for(&self, u: &UnitState) -> &'static str {
        if let Some(clip) = self.action_clip(u) {
            return clip;
        }
        if !self.humanoid {
            let role = match u.pose {
                Pose::Idle => "idle",
                Pose::Walk => "walk",
                Pose::Run | Pose::Dodge => "run",
                Pose::Jump | Pose::Fall | Pose::Hang => "jump",
                Pose::Attack | Pose::Cast => "attack",
                Pose::Hit => "hit",
                Pose::Dead => "death",
            };
            return self.role_clips.get(role).copied().unwrap_or_default();
        }
        match u.pose {
            Pose::Idle if u.look.drawn => self.main_clips.get("idle").unwrap_or(self.idle),
            Pose::Idle => self.idle,
            Pose::Walk | Pose::Run => locomotion_clip(u.pose, u.velocity.xz().length()),
            Pose::Jump => "Jump_Start",
            // Hanging borrows the falling loop until the climbing clips land (#247).
            Pose::Fall | Pose::Hang => "Jump_Loop",
            Pose::Dodge => "Roll",
            Pose::Attack | Pose::Cast => {
                let attacks = self.main_clips.list("attacks");
                match attacks.get(self.attack % attacks.len().max(1)) {
                    Some(&attack) => attack,
                    None if u.pose == Pose::Cast => "Spell_Simple_Shoot",
                    None => "Sword_Regular_A",
                }
            }
            Pose::Hit => "Hit_Chest",
            Pose::Dead => "Death01",
        }
    }
    /// What a humanoid on a wall plays (#247; docs/CONTRACT.md, "Poses to animation"), with the
    /// clip time and whether it loops: the clip for the sim's move onto, between or off holds,
    /// timed so it ends as the move does, or else the hang, shimmying whichever way the body
    /// slides along the wall. `None` off the wall.
    fn climbing(&mut self, u: &UnitState, dt: f32) -> Option<(&'static str, f32, bool)> {
        let (last, last_pose) = (self.last_traversal, self.last_pose);
        self.last_traversal = u.traversal;
        let (s, c) = u.yaw.sin_cos();
        let right = Vec3::new(-c, 0., s);
        match u.traversal {
            Traversal::Hanging => {
                self.climb = None;
                let side = u.velocity.dot(right);
                if side.abs() > 0.3 {
                    self.shimmied_right = side > 0.;
                }
                let clip = if side > 0.3 {
                    "Hang_Shimmy_R_Loop"
                } else if side < -0.3 {
                    "Hang_Shimmy_L_Loop"
                } else {
                    "Hang_Idle_Loop"
                };
                Some((clip, u.pose_time, true))
            }
            Traversal::Forced(f) => {
                if self.climb.as_ref().is_none_or(|m| m.start != f.start) {
                    // Which move this is, from how the body got here: a new move is decided
                    // once, on its first frame.
                    let from_hang = matches!(last, Traversal::Hanging)
                        || matches!(last, Traversal::Forced(g) if matches!(g.kind, ForcedKind::Hold | ForcedKind::Corner));
                    let d = f.to - f.from;
                    let clip = match f.kind {
                        ForcedKind::Climb if from_hang => "Hang_ClimbUp",
                        // Up or down to the next hold, or across to one (a side eject's longer
                        // hop until it has a clip of its own).
                        ForcedKind::Hold if from_hang => {
                            let across = d.dot(right);
                            if d.y > 0.3 && d.y > across.abs() {
                                "Hang_Climb_Up"
                            } else if d.y < -0.3 && -d.y > across.abs() {
                                "Hang_Climb_Down"
                            } else if across > 0. {
                                "Hang_Hop_R"
                            } else {
                                "Hang_Hop_L"
                            }
                        }
                        ForcedKind::Corner if self.shimmied_right => "Hang_Hop_R",
                        ForcedKind::Corner => "Hang_Hop_L",
                        ForcedKind::Hold if matches!(last_pose, Pose::Jump | Pose::Fall) => {
                            "Hang_Catch"
                        }
                        ForcedKind::Hold => "Hang_Drop",
                        _ => {
                            self.climb = None;
                            return None;
                        }
                    };
                    self.climb = Some(ClimbMove {
                        clip,
                        start: f.start,
                        time: 0.,
                        duration: f.duration.max(0.05),
                    });
                } else if let Some(m) = &mut self.climb {
                    m.time += dt;
                }
                let m = self.climb.as_ref()?;
                let length = self
                    .clips
                    .clips
                    .get(m.clip)
                    .map_or(m.duration, |a| a.duration);
                Some((m.clip, (m.time / m.duration).min(1.) * length, false))
            }
            _ => {
                self.climb = None;
                None
            }
        }
    }
    /// The locomotion pose the legs keep while a humanoid attacks or casts on the move, by the
    /// sim's own speed thresholds (`update_poses`). `None` plays the action on the whole body.
    fn moving_legs(&self, u: &UnitState) -> Option<Pose> {
        let kneeling = self
            .combat
            .as_ref()
            .is_some_and(|c| c.downed || c.action == Action::Revive);
        if !self.humanoid
            || kneeling
            || self.rally_time.is_some()
            || !matches!(u.pose, Pose::Attack | Pose::Cast)
        {
            return None;
        }
        let speed = u.velocity.xz().length();
        (speed > 0.3).then_some(if speed > 3.2 { Pose::Run } else { Pose::Walk })
    }
    /// How fast the walk or run cycle plays at `speed`.
    fn stride_rate(&self, pose: Pose, speed: f32) -> f32 {
        if pose == Pose::Walk {
            (speed
                / if self.humanoid {
                    1.5
                } else {
                    self.beast_speeds[0]
                })
            .clamp(0.3, 2.)
        } else {
            (speed
                / if self.humanoid {
                    4.5
                } else {
                    self.beast_speeds[1]
                })
            .clamp(0.5, 1.6)
        }
    }
    /// Puts walking legs under the action just applied: the step's loops (or `legs` at the
    /// stride phase, without measured gaits) drive the hips and legs. Returns where the action's
    /// chest faced about the vertical and its twist from its own hips, which `turn_hips` turns the
    /// chest back toward.
    fn walk_under(&mut self, legs: &str, step: Option<anim::Step>) -> Option<(f32, f32)> {
        let [pelvis, _, _, chest] = self.spine_bones?;
        let clip_hips = heading(
            self.rotation_to(pelvis, None, false),
            self.rotation_to(pelvis, None, true),
        );
        let clip_chest = heading(
            self.rotation_to(chest, None, false),
            self.rotation_to(chest, None, true),
        );
        match step {
            Some(step) => self.apply_step(step, Bones::Lower),
            None => {
                let time =
                    self.stride.phase * self.clips.clips.get(legs).map_or(1., |a| a.duration);
                self.apply_clip(legs, time, true, Bones::Lower);
            }
        }
        Some((clip_chest, wrap(clip_chest - clip_hips)))
    }
    /// Turns the hips `turn` round the vertical through the model's origin, so the legs and their
    /// planted feet turn with them toward where the body travels (orientation warping, #110). Then
    /// turns the upper body back toward `aim`: where the chest should face and its own twist from
    /// the hips, or where it faced before the turn when `None`; turned on by `look`, how far round
    /// from the body the sim faces it (#211). The chest stops `MAX_TWIST` round from the hips (or
    /// its own twist, if that is more), so swings that pivot the whole body keep a believable spine
    /// while side-on stances (a bow, a wand) still aim forward. The turn back is spread evenly up
    /// the spine, and the neck and head share up to `HEAD_TWIST` of whatever the chest can't take.
    fn turn_hips(&mut self, turn: f32, aim: Option<(f32, f32)>, look: f32) {
        let Some([pelvis, low, mid, chest]) = self.spine_bones else {
            return;
        };
        let rest_hips = self.rotation_to(pelvis, None, true);
        let rest_chest = self.rotation_to(chest, None, true);
        let facing = |c: &Self, i: usize, rest: Quat| heading(c.rotation_to(i, None, false), rest);
        let (aim, own) = aim.unwrap_or_else(|| {
            let chest = facing(self, chest, rest_chest);
            (chest, wrap(chest - facing(self, pelvis, rest_hips)))
        });
        if turn != 0. {
            let above = self.nodes[pelvis]
                .parent
                .map_or(Quat::IDENTITY, |p| self.rotation_to(p, None, false));
            let spin = above.inverse() * Quat::from_rotation_y(turn) * above;
            let hips = &mut self.local[pelvis];
            hips.rotation = (spin * hips.rotation).normalize();
            hips.translation = spin * hips.translation;
        }
        let hips = facing(self, pelvis, rest_hips);
        let wanted = wrap(aim + look - hips);
        let twist = wanted.clamp(own.min(-MAX_TWIST), own.max(MAX_TWIST));
        // Turns about the vertical, applied above the hips, add straight onto the chest's heading.
        let back = wrap(hips + twist - facing(self, chest, rest_chest)) / 3.;
        for bone in [low, mid, chest] {
            self.turn_upright(bone, back);
        }
        let rest = (wanted - twist).clamp(-HEAD_TWIST, HEAD_TWIST) / 2.;
        if rest != 0. {
            for i in self.head_bones.into_iter().flatten() {
                self.turn_upright(i, rest);
            }
        }
    }
    /// Turns bone `i` by `angle` round the model's vertical, taking its children with it.
    fn turn_upright(&mut self, i: usize, angle: f32) {
        let above = self.nodes[i]
            .parent
            .map_or(Quat::IDENTITY, |p| self.rotation_to(p, None, false));
        self.local[i].rotation =
            (above.inverse() * Quat::from_rotation_y(angle) * above * self.local[i].rotation)
                .normalize();
    }
    /// While the body turns on the spot (#211), keeps the feet where they stand and steps them
    /// one at a time to where the clip has them under the yaw the body turns to, the legs reaching
    /// with two-bone IK. Once they stand where the clip has them, it takes them back. Returns
    /// whether it let go of them anywhere else this frame (the body moved off), which the blender
    /// has to smooth.
    fn step_round(&mut self, standing: bool, position: Vec3, body: f32, dt: f32) -> bool {
        if !standing {
            return self.steps.take().is_some();
        }
        let active = self.turn.stepping() || self.steps.is_some();
        let Some(legs) = self.leg_bones.filter(|_| active) else {
            return false;
        };
        // Where the clip has each leg's joints in the model's frame, and so the ankles in the
        // world under a yaw.
        let joints = legs.map(|leg| self.chain(leg));
        let place = |yaw: f32| {
            joints.map(|[.., ankle]| anim::Plant {
                at: position + Quat::from_rotation_y(yaw) * ankle,
                yaw,
            })
        };
        // Toward the yaw the body turns to, or once it has stopped, the yaw it stopped at, so that
        // the clip takes back feet standing just where it has them.
        let stepping = self.turn.stepping();
        let rest = place(if stepping { self.turn.step_yaw() } else { body });
        let steps = self
            .steps
            .get_or_insert_with(|| anim::Steps::new(place(body)));
        if !stepping && steps.settled(rest) {
            self.steps = None;
            return false;
        }
        let feet = steps.update(rest, !stepping, dt);
        let to_model = Quat::from_rotation_y(-body);
        for ((leg, joints), foot) in legs.into_iter().zip(joints).zip(feet) {
            let turn = Quat::from_rotation_y(foot.yaw - body);
            self.reach(leg, joints, to_model * (foot.at - position), turn, turn);
        }
        false
    }
    /// Puts the feet on the ground (#214): `anim::Feet` says where each goes and how far the
    /// pelvis drops for the body at `root` turned `body`, then the pelvis drops, taking everything
    /// on it down, and each leg bends to its foot. `contact` is `None` off the ground, which
    /// fades the placement out.
    fn place_feet(&mut self, root: Vec3, body: f32, contact: Option<anim::Contact>, dt: f32) {
        let (Some(legs), Some(balls), Some([pelvis, ..])) =
            (self.leg_bones, self.ball_bones, self.spine_bones)
        else {
            return;
        };
        let at = |i: usize| self.world[i].w_axis.truncate();
        let clip = [0, 1].map(|side| {
            let [hip, knee, ankle] = legs[side];
            anim::Leg {
                hip: at(hip),
                knee: at(knee),
                ankle: at(ankle),
                ball: at(balls[side]),
            }
        });
        // Without a ground, it is level with the unit.
        let placed = match &self.ground {
            Some(ground) => self
                .feet
                .update(root, body, clip, contact, |x, z| ground(x, z), dt),
            None => self
                .feet
                .update(root, body, clip, contact, |_, _| root.y, dt),
        };
        let Some(placed) = placed else {
            return;
        };
        let drop = Vec3::Y * placed.drop;
        if placed.drop != 0. {
            let above = self.nodes[pelvis]
                .parent
                .map_or(Quat::IDENTITY, |p| self.rotation_to(p, None, false));
            self.local[pelvis].translation += above.inverse() * drop;
            self.shift(pelvis, drop);
        }
        for ((leg, clip), to) in legs.into_iter().zip(clip).zip(placed.legs) {
            let joints = [clip.hip + drop, clip.knee + drop, clip.ankle + drop];
            if joints[2].distance_squared(to.ankle) < 1e-10 && to.tilt.w.abs() > 1. - 1e-7 {
                continue;
            }
            self.reach(leg, joints, to.ankle, Quat::IDENTITY, to.tilt);
            self.update_chain(leg[0]);
        }
    }
    /// Moves bone `i` and everything on it by `by` in the model's frame: their world matrices
    /// move with it, so need no rebuilding.
    fn shift(&mut self, i: usize, by: Vec3) {
        if !self.partial_updates {
            self.pose_world();
            return;
        }
        let (count, on) = &mut self.dropped;
        if *count != self.nodes.len() || on.first() != Some(&i) {
            // Listed once (again if a piece brings bones): a flat list is quicker to go through
            // than the tree, and the pelvis carries nearly the whole skeleton.
            let mut under = vec![false; self.nodes.len()];
            under[i] = true;
            on.clear();
            on.push(i);
            for &n in &self.order {
                if let Some(p) = self.nodes[n].parent
                    && under[p]
                    && !under[n]
                {
                    under[n] = true;
                    on.push(n);
                }
            }
            *count = self.nodes.len();
        }
        let by = by.extend(0.);
        for &n in on.iter() {
            self.world[n].w_axis += by;
        }
    }
    /// Bends a leg (hip, knee and ankle bones, its `joints` where `chain` has them) with two-bone
    /// IK so that the ankle is at `target` in the model's frame. The knee points as the clip has
    /// it, turned by `pole`, and the foot turns by `foot` in the model's frame.
    fn reach(
        &mut self,
        [hip, knee, ankle]: [usize; 3],
        joints: [Vec3; 3],
        target: Vec3,
        pole: Quat,
        foot: Quat,
    ) {
        let [a, b, c] = joints;
        // The knee points as the clip has it, out from the line from hip to ankle.
        let line = (c - a).normalize_or(Vec3::NEG_Y);
        let out = (b - a) - line * (b - a).dot(line);
        let pole = pole * if out.length() > 0.01 { out } else { Vec3::Z };
        let (upper, lower) = anim::ik::two_bone(a, b, c, target, pole);
        let above = self.nodes[hip]
            .parent
            .map_or(Quat::IDENTITY, |p| self.rotation_to(p, None, false));
        let hip_was = above * self.local[hip].rotation;
        let knee_was = hip_was * self.local[knee].rotation;
        let ankle_was = knee_was * self.local[ankle].rotation;
        let [hip_now, knee_now] = [upper * hip_was, lower * knee_was];
        self.local[hip].rotation = (above.inverse() * hip_now).normalize();
        self.local[knee].rotation = (hip_now.inverse() * knee_now).normalize();
        self.local[ankle].rotation = (knee_now.inverse() * foot * ankle_was).normalize();
    }
    /// Where the joints of `bones`, each the parent of the next, are in the model's frame, in the
    /// live pose.
    fn chain<const N: usize>(&self, bones: [usize; N]) -> [Vec3; N] {
        let mut matrix = Mat4::IDENTITY;
        let mut at = self.nodes[bones[0]].parent;
        while let Some(p) = at {
            matrix = self.local[p].matrix() * matrix;
            at = self.nodes[p].parent;
        }
        bones.map(|i| {
            matrix *= self.local[i].matrix();
            matrix.w_axis.truncate()
        })
    }
    /// Samples a step's two loops into the pose, blended by its weight, or one alone where the
    /// blend is nearly all one of them.
    fn apply_step(&mut self, step: anim::Step, bones: Bones) {
        let ((a, at), (b, bt)) = (step.from, step.to);
        if step.weight > 0.98 {
            self.apply_clip(LOOPS[b], bt, true, bones);
            return;
        }
        self.apply_clip(LOOPS[a], at, true, bones);
        if step.weight >= 0.02 {
            self.mix_clip(LOOPS[b], bt, true, bones, step.weight);
        }
    }
    /// Measures the humanoid loops on this skeleton (#110), from where the balls of the feet are
    /// at evenly spaced times through each cycle. `None` if a loop is missing or won't measure, or
    /// they don't get faster from walk to sprint.
    fn measure_gaits(&mut self) -> Option<[anim::Gait; 3]> {
        const SAMPLES: usize = 64;
        let feet = [*self.names.get("ball_l")?, *self.names.get("ball_r")?];
        // Pieces may have brought bones since the world matrices were last built (a cape's).
        self.world.resize(self.nodes.len(), Mat4::IDENTITY);
        let mut gaits = [None; 3];
        for (gait, name) in gaits.iter_mut().zip(LOOPS) {
            let duration = self.clip_set.clips.get(name)?.clip.duration;
            let mut at = Vec::with_capacity(SAMPLES);
            for k in 0..SAMPLES {
                for (n, l) in self.nodes.iter().zip(&mut self.local) {
                    *l = n.rest;
                }
                self.apply_clip(name, duration * k as f32 / SAMPLES as f32, true, Bones::All);
                self.pose_world();
                at.push(feet.map(|i| self.world[i].w_axis.truncate()));
            }
            *gait = anim::Gait::measure(&at, duration);
        }
        let [Some(walk), Some(jog), Some(sprint)] = gaits else {
            return None;
        };
        (walk.speed() < jog.speed() && jog.speed() < sprint.speed()).then_some([walk, jog, sprint])
    }
    /// The rotation from `top`'s frame (the model's when `None`) down to bone `i`'s, in the live
    /// pose or the rest pose.
    fn rotation_to(&self, i: usize, top: Option<usize>, rest: bool) -> Quat {
        let mut rotation = Quat::IDENTITY;
        let mut at = Some(i);
        while let Some(n) = at.filter(|n| Some(*n) != top) {
            let local = if rest {
                self.nodes[n].rest.rotation
            } else {
                self.local[n].rotation
            };
            rotation = local * rotation;
            at = self.nodes[n].parent;
        }
        rotation
    }
    fn apply_clip(&mut self, name: &str, t: f32, looping: bool, bones: Bones) {
        self.mix_clip(name, t, looping, bones, 1.);
    }
    /// Samples a clip into the pose, `weight` of the way from the pose as it is (all of it at 1).
    fn mix_clip(&mut self, name: &str, t: f32, looping: bool, bones: Bones, weight: f32) {
        let Some(bound) = self.clip_set.clips.get(name) else {
            return;
        };
        let clip = &bound.clip;
        let t = if looping {
            t.rem_euclid(clip.duration.max(0.001))
        } else {
            t.clamp(0., clip.duration)
        };
        let tracks = match bones {
            Bones::All => &bound.tracks[..],
            Bones::Upper => &bound.tracks[..bound.upper],
            Bones::Lower => &bound.tracks[bound.upper..],
        };
        for b in tracks {
            let v = clip.tracks[b.track as usize].sample(t);
            let i = b.bone as usize;
            let local = &mut self.local[i];
            match b.apply {
                Apply::Rotation => {
                    let q = Quat::from_vec4(v);
                    local.rotation = if weight < 1. {
                        local.rotation.lerp(q, weight)
                    } else {
                        q
                    };
                }
                Apply::Translation | Apply::Pelvis | Apply::Scale => {
                    let mut to = v.truncate();
                    if b.apply == Apply::Pelvis {
                        to = self.nodes[i].rest.translation
                            + (to - self.reference_pelvis) * self.pelvis_ratio;
                    }
                    let at = if b.apply == Apply::Scale {
                        &mut local.scale
                    } else {
                        &mut local.translation
                    };
                    *at = if weight < 1. { at.lerp(to, weight) } else { to };
                }
            }
        }
    }
    /// The yaw to draw the body at (#211): the sim's, except that turning on the spot the body
    /// lags behind it while the upper body takes up the turn. 0 before the first update.
    pub fn body_yaw(&self) -> f32 {
        match &self.between {
            Some(b) if b.live && self.tier == anim::Tier::Half => b.yaw,
            _ => self.turn.yaw().unwrap_or(0.),
        }
    }
    pub fn rallied(&mut self) {
        self.rally_time = Some(0.);
    }
    pub fn update_combat(&mut self, u: &UnitState, combat: Option<&UnitCombat>, dt: f32) {
        if let Some(c) = combat {
            let new_action = self
                .combat
                .as_ref()
                .is_none_or(|old| old.action != c.action || c.action_time < old.action_time);
            if new_action {
                self.windup = None;
                // The same clip may start over: still a switch.
                self.forced_switch = Some(anim::Switch::Action);
            }
            if u.pose == Pose::Attack
                && let Some(cast) = &c.cast
                && !cast.channel
            {
                self.windup = Some(cast.duration);
            }
            if c.action != Action::Stagger {
                self.was_guarding = c.guarding;
            }
        }
        self.combat = combat.map(Fight::from);
        self.update(u, dt);
    }
    pub fn update(&mut self, u: &UnitState, dt: f32) {
        if let Some(t) = &mut self.rally_time {
            *t += dt;
            if *t
                >= self
                    .clips
                    .clips
                    .get("LayToIdle")
                    .map_or(1.5, |a| a.duration)
            {
                self.rally_time = None;
            }
        }
        if u.pose == Pose::Attack && self.last_pose != Pose::Attack {
            self.attack += 1;
        }
        if u.look.drawn != self.last_drawn {
            self.grab_offset = None;
            self.transition = self
                .main_clips
                .get(if u.look.drawn { "ready" } else { "stow" })
                .map(|name| (name, 0.));
            self.last_drawn = u.look.drawn;
        }
        if matches!(
            u.pose,
            Pose::Attack | Pose::Cast | Pose::Dodge | Pose::Hit | Pose::Dead
        ) {
            self.transition = None;
        }
        if let Some((name, time)) = &mut self.transition {
            *time += dt;
            if *time >= self.clips.clips.get(*name).map_or(0., |a| a.duration) {
                self.transition = None;
            }
        }
        let climbing = if self.humanoid {
            self.climbing(u, dt)
        } else {
            None
        };
        let name = match climbing {
            Some((clip, _, _)) => clip,
            None => self.clip_for(u),
        };
        let speed = u.velocity.xz().length();
        let moving = self.moving_legs(u);
        let legs = moving.map(|pose| locomotion_clip(pose, speed));
        let acting = self.action_clip(u).is_some();
        let busy = acting
            || matches!(
                u.pose,
                Pose::Attack | Pose::Cast | Pose::Dodge | Pose::Hit | Pose::Dead
            );
        // Any change in what plays is a switch for the blender, even one that looks continuous.
        let mut switch = self.forced_switch.take();
        if name != self.last_clip || legs != self.last_legs || u.pose != self.last_pose {
            switch = Some(switch_kind(self.last_pose, self.last_busy, u.pose, busy));
            self.last_clip = name;
            self.last_legs = legs;
        }
        self.last_pose = u.pose;
        self.last_busy = busy;
        for (n, l) in self.nodes.iter().zip(&mut self.local) {
            *l = n.rest;
        }
        let locomotion = matches!(u.pose, Pose::Walk | Pose::Run);
        let walking = locomotion && !acting;
        let looping = matches!(u.pose, Pose::Idle | Pose::Walk | Pose::Run | Pose::Fall)
            || self.combat.as_ref().is_some_and(|c| {
                matches!(c.action, Action::Channel | Action::Revive)
                    || c.action == Action::Spin && c.cast.is_some_and(|(_, channel)| channel)
            });
        let rate = match u.pose {
            Pose::Walk | Pose::Run => self.stride_rate(u.pose, speed),
            Pose::Dodge => self.clips.clips.get(name).map_or(1., |a| a.duration / 0.45),
            Pose::Attack => self.clips.clips.get(name).map_or(1., |a| a.duration / 0.5),
            _ => 1.,
        };
        // The body turns on the spot behind the sim's yaw (#211): standing, the upper body takes
        // up the turn until the feet step round; moving or acting, the body follows on a spring.
        // Below full detail (#215) the feet don't step: the body follows on the spring.
        let full = self.tier == anim::Tier::Full;
        let stance = if full && self.humanoid && u.pose == Pose::Idle && !busy {
            anim::Stance::Standing
        } else {
            anim::Stance::Moving
        };
        let body = self.turn.update(u.yaw, stance, dt);
        // The humanoid loops follow the body (#110): blended by speed, the hips turned toward
        // where it travels. One stride phase runs through attacks on the move and every change of
        // speed or direction, so the feet never restart.
        let step = match self.gaits.as_ref().filter(|_| walking || moving.is_some()) {
            Some(gaits) => {
                let travel = Quat::from_rotation_y(-body) * u.velocity;
                Some(self.stride.advance(gaits, travel.xz(), dt))
            }
            None => {
                self.stride.stop();
                None
            }
        };
        // Without measured loops (beasts), the walk or run plays at `stride_rate`.
        let stride = match moving {
            Some(pose) => Some((locomotion_clip(pose, speed), self.stride_rate(pose, speed))),
            None => walking.then_some((name, rate)),
        };
        if let Some((clip, rate)) = stride.filter(|_| step.is_none()) {
            let length = self
                .clips
                .clips
                .get(clip)
                .map_or(1., |a| a.duration.max(0.001));
            self.stride.phase = (self.stride.phase + dt * rate / length).fract();
        }
        let mut clip_time = if walking {
            match (step, self.gaits, LOOPS.iter().position(|l| *l == name)) {
                (Some(_), Some(gaits), Some(i)) => gaits[i].time_at(self.stride.phase),
                _ => self.stride.phase * self.clips.clips.get(name).map_or(1., |a| a.duration),
            }
        } else {
            u.pose_time * rate
        };
        if let Some(c) = &self.combat {
            if c.downed {
                clip_time = self.clips.clips.get(name).map_or(0., |a| a.duration);
            } else if let Some(t) = self.rally_time {
                clip_time = t;
            } else if let Some(windup) = self.windup.filter(|_| u.pose == Pose::Attack) {
                // The blow lands on the clip's `hit` marker just as the wind-up ends.
                let duration = self.clips.clips.get(name).map_or(1., |a| a.duration);
                let hit = self
                    .clip_meta
                    .get(name)
                    .and_then(|m| m.event("hit"))
                    .unwrap_or(duration * 0.5);
                clip_time = swing_time(c.action_time, windup, hit, duration);
            } else if acting {
                clip_time = if c.action == Action::Parry {
                    c.action_time * 2.
                } else if u.pose == Pose::Attack {
                    c.action_time * rate
                } else {
                    c.action_time
                };
            }
        }
        let (clip_time, looping) =
            climbing.map_or((clip_time, looping), |(_, time, looping)| (time, looping));
        match step.filter(|_| walking) {
            Some(step) => self.apply_step(step, Bones::All),
            None => self.apply_clip(name, clip_time, looping, Bones::All),
        }
        let aim = legs.and_then(|legs| self.walk_under(legs, step));
        let mut upper = None;
        if locomotion
            && u.look.drawn
            && self.humanoid
            && let Some(idle) = self.main_clips.get("idle")
        {
            self.apply_clip(idle, u.pose_time, true, Bones::Upper);
            upper = Some(idle);
        }
        if self.humanoid && self.combat.as_ref().is_some_and(|c| c.guarding) {
            let c = self.combat.as_ref().unwrap();
            let parrying = c.action == Action::Parry && c.action_time < 0.25;
            let time = if parrying {
                c.action_time * 2.
            } else {
                c.action_time
            };
            let guard = if parrying {
                "Sword_Block"
            } else {
                self.shield_clip("block", "Sword_Block")
            };
            let looping = !parrying && self.clip_meta.get(guard).is_some_and(|m| m.looping);
            self.apply_clip(guard, time, looping, Bones::Upper);
            upper = Some(guard);
            self.transition = None;
        }
        let transition = self.transition;
        if let Some((n, t)) = transition {
            self.apply_clip(n, t, false, Bones::Upper);
            upper = Some(n);
        }
        if upper != self.last_upper {
            switch = switch.or(Some(anim::Switch::Layer));
            self.last_upper = upper;
        }
        // The upper body faces the sim's yaw (closely followed), however far round the body is.
        let look = wrap(self.turn.aim() - body);
        if step.is_some() || aim.is_some() || look.abs() > 1e-4 {
            self.turn_hips(step.map_or(0., |s| s.turn), aim, look);
        }
        if self.step_round(stance == anim::Stance::Standing, u.position, body, dt) {
            switch = switch.or(Some(anim::Switch::Layer));
        }
        self.pose_world();
        self.drawn_now = u.look.drawn;
        self.main_offset = 0.;
        let (active, t) = transition.unwrap_or((name, clip_time));
        self.perform(active, t);
        // Last, so a switch also smooths the hand reaching and gripping that `perform` adds.
        self.blender.update(&mut self.local, switch, dt);
        if self.blender.blending() {
            self.pose_world();
        }
        // Then the feet go on the ground (#214), so that nothing smooths them off it: at full
        // detail, standing, walking or acting on it, the feet planted in the stride held still.
        // Not on a wall (#247), even taking a blow there.
        let kneeling = self
            .combat
            .as_ref()
            .is_some_and(|c| c.downed || c.action == Action::Revive);
        let grounded = full
            && self.humanoid
            && !kneeling
            && self.rally_time.is_none()
            && climbing.is_none()
            && matches!(
                u.pose,
                Pose::Idle | Pose::Walk | Pose::Run | Pose::Attack | Pose::Cast | Pose::Hit
            );
        let contact = grounded.then(|| match step {
            Some(step) => anim::Contact {
                planted: step.planted,
                floors: step.floors,
                stride: Quat::from_rotation_y(step.turn) * Vec3::Z,
                warp: step.warp,
            },
            None => anim::Contact::STILL,
        });
        if self.foot_placement {
            self.place_feet(u.position, body, contact, dt);
        } else {
            self.feet = anim::Feet::default();
        }
        let root = Mat4::from_rotation_translation(Quat::from_rotation_y(body), u.position);
        let mut springs = std::mem::take(&mut self.springs);
        for s in &mut springs {
            s.update(self, root, dt, full);
        }
        self.springs = springs;
    }
    /// How often the pose is rebuilt (#215).
    pub fn tier(&self) -> anim::Tier {
        self.tier
    }
    /// Stands the character on `ground` (#214). Without one, the ground is level with the unit.
    pub fn set_ground(&mut self, ground: Ground) {
        self.ground = Some(ground);
    }
    /// Sets how often the pose is rebuilt, as `anim::Crowd` ranks the character this frame.
    pub fn set_tier(&mut self, tier: anim::Tier) {
        if tier != self.tier
            && let Some(between) = &mut self.between
        {
            // Back at half rate, it starts again from the pose shown then.
            between.live = false;
        }
        self.tier = tier;
    }
    /// Rebuilds the pose with the time since it last did, if the character is due on `frame` at
    /// its tier (#215): every frame, every other frame or every fourth, the frames spread out by
    /// unit id. Its first pose is built at once, whatever the tier, so nobody shows the rest
    /// pose. Between rebuilds, at half rate it shows the last two blended, a rebuild behind; at
    /// quarter rate it holds the last. Returns whether it rebuilt.
    pub fn animate(
        &mut self,
        u: &UnitState,
        combat: Option<&UnitCombat>,
        dt: f32,
        frame: u64,
    ) -> bool {
        self.since += dt;
        let half = self.tier == anim::Tier::Half;
        let first = self.turn.yaw().is_none();
        if !first && !self.tier.due(frame, u.id as u64) {
            if let Some(b) = self.between.as_mut().filter(|b| b.live && half) {
                let t = (self.since / b.interval.max(1e-4)).min(1.);
                for ((shown, from), to) in self.world.iter_mut().zip(&b.from).zip(&b.to) {
                    *shown = *from * (1. - t) + *to * t;
                }
                b.yaw = b.yaws[0] + wrap(b.yaws[1] - b.yaws[0]) * t;
            }
            return false;
        }
        if half {
            let world = &self.world;
            let b = self.between.get_or_insert_with(|| Between::new(world));
            if !b.live {
                b.from.copy_from_slice(world);
                b.to.copy_from_slice(world);
                b.yaws = [self.turn.yaw().unwrap_or(u.yaw); 2];
                b.live = true;
            }
        }
        let dt = std::mem::take(&mut self.since);
        self.update_combat(u, combat, dt);
        if let Some(b) = self.between.as_mut().filter(|b| b.live && half) {
            if first {
                // Nothing has been shown yet, so this pose shows at once.
                b.to.copy_from_slice(&self.world);
                b.yaws[1] = self.turn.yaw().unwrap_or(u.yaw);
            }
            std::mem::swap(&mut b.from, &mut b.to);
            b.to.copy_from_slice(&self.world);
            self.world.copy_from_slice(&b.from);
            b.yaws = [b.yaws[1], self.turn.yaw().unwrap_or(u.yaw)];
            b.yaw = b.yaws[0];
            b.interval = dt;
        }
        true
    }
    /// Where the hand socket on `side` is.
    fn hand(&self, side: Side) -> Mat4 {
        self.arm_bones[side as usize].map_or(Mat4::IDENTITY, |[.., s]| self.world[s])
    }
    fn socket(&self, name: &str) -> Mat4 {
        self.names
            .get(name)
            .map_or(Mat4::IDENTITY, |i| self.world[*i])
    }
    pub(crate) fn turn_bone(&mut self, i: usize, from: Vec3, to: Vec3, weight: f32) {
        if from.length_squared() < 1e-10 || to.length_squared() < 1e-10 {
            return;
        }
        let q = Quat::IDENTITY.slerp(
            Quat::from_rotation_arc(from.normalize(), to.normalize()),
            weight,
        );
        let world = self.world[i].to_scale_rotation_translation().1;
        let parent = self.nodes[i].parent.map_or(Quat::IDENTITY, |p| {
            self.world[p].to_scale_rotation_translation().1
        });
        self.local[i].rotation = (parent.inverse() * q * world).normalize();
        self.update_chain(i);
    }
    fn arm_to(&mut self, side: Side, target: Mat4, weight: f32) {
        if weight <= 0. {
            return;
        }
        let Some([a, b, h, s]) = self.arm_bones[side as usize] else {
            return;
        };
        let want = target * self.local[s].matrix().inverse();
        let goal = want.w_axis.truncate();
        let shoulder = self.world[a].w_axis.truncate();
        let elbow = self.world[b].w_axis.truncate();
        let wrist = self.world[h].w_axis.truncate();
        let l1 = shoulder.distance(elbow);
        let l2 = elbow.distance(wrist);
        if l1 * l2 < 1e-8 {
            return;
        }
        let diff = goal - shoulder;
        let d = diff.length().clamp(1e-4, (l1 + l2) * 0.9995);
        let dir = diff.normalize_or_zero();
        let cos = ((l1 * l1 + d * d - l2 * l2) / (2. * l1 * d)).clamp(-1., 1.);
        let mut plane = elbow - shoulder;
        plane -= dir * plane.dot(dir);
        if plane.length_squared() < 1e-10 {
            plane = dir.any_orthonormal_vector();
        }
        plane = plane.normalize();
        let new_elbow = shoulder + dir * l1 * cos + plane * l1 * (1. - cos * cos).max(0.).sqrt();
        self.turn_bone(a, elbow - shoulder, new_elbow - shoulder, weight);
        self.turn_bone(
            b,
            self.world[h].w_axis.truncate() - self.world[b].w_axis.truncate(),
            goal - self.world[b].w_axis.truncate(),
            weight,
        );
        let now = self.world[h].to_scale_rotation_translation().1;
        let target_q = want.to_scale_rotation_translation().1;
        let parent = self.world[self.nodes[h].parent.unwrap()]
            .to_scale_rotation_translation()
            .1;
        self.local[h].rotation = (parent.inverse() * now.slerp(target_q, weight)).normalize();
        self.update_chain(h);
    }
    fn perform(&mut self, name: &str, t: f32) {
        self.bow_morph = [0.; 2];
        let Some(main) = self.main.clone() else {
            return;
        };
        let Some(meta) = self.clip_meta.get(name).cloned() else {
            return;
        };
        let side = if main.held.as_deref() == Some("socket_hand_l") {
            Side::Left
        } else {
            Side::Right
        };
        let Some([held, stowed]) = self.main_sockets else {
            return;
        };
        let mut offset = 0.;
        let grab = meta.event("grab");
        let stow = meta.event("stow");
        if grab.is_some() || stow.is_some() {
            let socket = self.world[stowed];
            let hand = self.hand(side);
            let range = main.grab_range.unwrap_or([0.; 2]);
            let s = ((hand.w_axis.truncate() - socket.w_axis.truncate())
                .dot(socket.y_axis.truncate().normalize())
                / self.gear_scale)
                .clamp(range[0], range[1]);
            self.arm_to(
                side,
                socket * Mat4::from_translation(Vec3::Y * s * self.gear_scale),
                meta.curve("reach", t),
            );
            if let Some(grab) = grab {
                if t < grab || self.grab_offset.is_none() {
                    self.grab_offset = Some(s);
                }
                let s = self.grab_offset.unwrap();
                self.drawn_now = t >= grab;
                if t >= grab {
                    offset = -s * (1. - smooth((t - grab) / (meta.seconds - grab).max(0.001)));
                }
            }
            if let Some(stow) = stow {
                self.drawn_now = t < stow;
                if t < stow {
                    offset = -s * smooth(t / stow.max(0.001));
                }
            }
        }
        self.main_offset = offset;
        let item = self.world[if self.drawn_now { held } else { stowed }]
            * Mat4::from_scale(Vec3::splat(self.gear_scale))
            * Mat4::from_translation(Vec3::Y * offset);
        if let Some(grip) = main.grips.get("off_hand") {
            let other = side.other();
            let hand = self.hand(other);
            let h = item.inverse().transform_point3(hand.w_axis.truncate());
            let up = Vec3::from_array(grip.up).normalize();
            let range = grip.range.unwrap_or([Vec3::from_array(grip.at).dot(up); 2]);
            let along = h.dot(up).clamp(range[0], range[1]);
            let at = Vec3::from_array(grip.at) + up * (along - Vec3::from_array(grip.at).dot(up));
            self.arm_to(
                other,
                item * frame(at, up, Vec3::from_array(grip.front)),
                meta.curve("off_hand", t),
            );
        }
        if let Some(string) = &main.string {
            let hand = self.hand(side.other());
            let h = item.inverse().transform_point3(hand.w_axis.truncate());
            let pull = Vec3::from_array(string.pull);
            self.bow_morph = [
                meta.curve("arrow", t),
                ((h - Vec3::from_array(string.rest)).dot(pull) / pull.length_squared().max(1e-8))
                    .clamp(0., 1.)
                    * meta.curve("string", t),
            ];
        }
    }
    pub fn draws(&self, position: Vec3, yaw: f32) -> Vec<Draw> {
        let unit = Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), position);
        let mut out = Vec::new();
        for (part_index, part) in self.parts.iter().enumerate() {
            let palettes: Vec<Arc<[Mat4]>> = part
                .model
                .skins
                .iter()
                .map(|skin| {
                    skin.joints
                        .iter()
                        .zip(&skin.inverse)
                        .map(|(j, inv)| self.world[part.remap[*j]] * *inv)
                        .collect()
                })
                .collect();
            for (pi, p) in part.model.primitives.iter().enumerate() {
                if part.eyes_hidden && p.vertices.first().is_some_and(|v| v.position[3] < 0.) {
                    continue;
                }
                let palette = p
                    .skin
                    .map_or_else(|| Arc::from([]), |si| palettes[si].clone());
                let mut transform = unit;
                if let Some(a) = &part.attachment {
                    let held = self.drawn_now || a.always;
                    let socket = if held && !a.sheath {
                        &a.held
                    } else {
                        &a.stowed
                    };
                    transform *=
                        self.socket(socket) * Mat4::from_scale(Vec3::splat(self.gear_scale));
                    if a.main && held && !a.sheath {
                        transform *= Mat4::from_translation(Vec3::Y * self.main_offset);
                    }
                }
                if palette.is_empty() {
                    transform *= part.model.rest_world[p.node];
                }
                let mut base = p.material.base;
                let tint = if p.material.name.starts_with("skin.") {
                    part.tint_skin
                } else if p.material.name.starts_with("hair.") {
                    part.tint_hair
                } else {
                    None
                };
                if let Some(t) = tint {
                    base[0] = 2. * t[0];
                    base[1] = 2. * t[1];
                    base[2] = 2. * t[2];
                }
                out.push(Draw {
                    dye: part.appearance.dye.clone(),
                    base_map: p.material.maps[0]
                        .as_ref()
                        .and_then(|t| part.appearance.variants.get(&t.key))
                        .cloned(),
                    instance: ((part_index as u64) << 16) | pi as u64,
                    model: part.model.clone(),
                    primitive: pi,
                    transform,
                    palette,
                    hidden: part.hidden,
                    base,
                    morph: if part.attachment.as_ref().is_some_and(|a| a.main) {
                        self.bow_morph
                    } else {
                        [0.; 2]
                    },
                });
            }
        }
        out
    }
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn frame(at: Vec3, up: Vec3, front: Vec3) -> Mat4 {
    let y = up.normalize();
    let z = (front - y * front.dot(y)).normalize();
    Mat4::from_cols(
        y.cross(z).extend(0.),
        y.extend(0.),
        z.extend(0.),
        at.extend(1.),
    )
}

/// Piecewise remapping keeps the strike at the authoritative wind-up boundary and recovery at 0.5 s.
fn swing_time(elapsed: f32, windup: f32, hit: f32, end: f32) -> f32 {
    if elapsed <= windup {
        hit * (elapsed / windup.max(0.001)).clamp(0., 1.)
    } else {
        hit + (end - hit) * ((elapsed - windup) / 0.5).clamp(0., 1.)
    }
}
#[cfg(test)]
mod combat_tests {
    use super::*;
    #[test]
    fn actions_locomotion_guard_and_rally_follow_contract() {
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let mut cache = ModelCache::default();
        let look = Look {
            art: "male".into(),
            drawn: true,
            gear: vec![
                ("main_hand".into(), "sword.arming".into()),
                ("off_hand".into(), "shield.round".into()),
            ],
            ..Default::default()
        };
        let mut c = Character::load(&cat, &mut cache, &look).unwrap();
        // These check the clip stack itself, so every switch cuts.
        c.blender.set_method(anim::Method::Off);
        let mut u = UnitState {
            id: 1,
            kind: protocol::UnitKind::Player,
            body: "player".into(),
            name: "test".into(),
            level: 1,
            position: Vec3::ZERO,
            velocity: Vec3::new(6., 8., 0.),
            yaw: 0.,
            pose: Pose::Run,
            pose_time: 0.2,
            traversal: Default::default(),
            health: 100.,
            max_health: 100.,
            hostile: false,
            target: None,
            look: look.into(),
        };
        assert_eq!(
            c.clip_for(&u),
            "Jog_Fwd_Loop",
            "slopes must not select sprint"
        );
        u.velocity.x = 7.01;
        assert_eq!(c.clip_for(&u), "Sprint_Loop");
        let mut fight = UnitCombat {
            id: 1,
            action: Action::None,
            action_time: 0.2,
            cast: None,
            auras: vec![],
            resource_kind: protocol::ResourceKind::Rage,
            resource: 0.,
            max_resource: 100.,
            guarding: false,
            downed: false,
            defiance: None,
        };
        for (action, expected) in [
            (Action::Slash, "Sword_Regular_A"),
            (Action::Slash2, "Sword_Regular_B"),
            (Action::Thrust, "Sword_Regular_C"),
            (Action::Overhead, "Sword_Overhead"),
            (Action::Bash, "Shield_OneShot"),
            (Action::Channel, "Spell_Simple_Idle_Loop"),
            (Action::Revive, "Fixing_Kneeling"),
        ] {
            u.pose = Pose::Attack;
            fight.action = action;
            c.update_combat(&u, Some(&fight), 1. / 60.);
            assert_eq!(c.last_clip, expected);
            let full = world_matrices(&c.nodes, &c.local);
            assert!(
                full.iter()
                    .zip(&c.world)
                    .all(|(a, b)| (*a - *b).to_cols_array().iter().all(|v| v.abs() < 1e-5)),
                "partial skeleton updates must match a full solve"
            );
        }
        u.pose = Pose::Run;
        fight.action = Action::Slash;
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(
            c.last_clip, "Sprint_Loop",
            "stale actions must not override locomotion"
        );
        // A sword's non-looping guard must hold, even when the locomotion pose clock changes.
        c.off_hand_clips = ItemClips::default();
        u.pose = Pose::Idle;
        fight.guarding = true;
        fight.action = Action::Block;
        fight.action_time = 3.;
        c.update_combat(&u, Some(&fight), 0.2);
        let arm = c.names["upperarm_r"];
        let held = c.local[arm].rotation;
        fight.action_time = 4.1;
        u.pose_time = 0.;
        c.update_combat(&u, Some(&fight), 0.2);
        assert!(held.abs_diff_eq(c.local[arm].rotation, 1e-5));
        fight.guarding = false;
        fight.downed = true;
        u.pose = Pose::Dead;
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Death01");
        fight.downed = false;
        u.pose = Pose::Idle;
        c.rallied();
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "LayToIdle");
    }
    #[test]
    fn heavy_blow_has_one_continuous_strike_and_recovery() {
        assert_eq!(super::swing_time(0., 0.4, 0.6, 1.2), 0.);
        assert!((super::swing_time(0.4, 0.4, 0.6, 1.2) - 0.6).abs() < 1e-6);
        assert!((super::swing_time(0.9, 0.4, 0.6, 1.2) - 1.2).abs() < 1e-6);
        let values: Vec<_> = (0..100)
            .map(|i| super::swing_time(i as f32 / 100., 0.4, 0.6, 1.2))
            .collect();
        assert!(values.windows(2).all(|w| w[1] >= w[0]));
    }
    #[test]
    fn wound_up_overhead_strikes_on_its_hit_marker() {
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let mut cache = ModelCache::default();
        let look = Look {
            art: "male".into(),
            drawn: true,
            gear: vec![("main_hand".into(), "sword.arming".into())],
            ..Default::default()
        };
        let mut c = Character::load(&cat, &mut cache, &look).unwrap();
        c.blender.set_method(anim::Method::Off);
        let u = UnitState {
            id: 1,
            kind: protocol::UnitKind::Player,
            body: "player".into(),
            name: "test".into(),
            level: 1,
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            yaw: 0.,
            pose: Pose::Attack,
            pose_time: 0.,
            traversal: Default::default(),
            health: 100.,
            max_health: 100.,
            hostile: false,
            target: None,
            look: look.into(),
        };
        let mut fight = UnitCombat {
            id: 1,
            action: Action::Overhead,
            action_time: 0.,
            cast: Some(protocol::CastBar {
                ability: "sword_overhead".into(),
                name: "Overhead".into(),
                elapsed: 0.,
                duration: 0.4,
                channel: false,
                interruptible: true,
            }),
            auras: vec![],
            resource_kind: protocol::ResourceKind::Rage,
            resource: 0.,
            max_resource: 100.,
            guarding: false,
            downed: false,
            defiance: None,
        };
        for frame in 0..=24 {
            fight.action_time = frame as f32 / 60.;
            c.update_combat(&u, Some(&fight), 1. / 60.);
        }
        assert_eq!(c.last_clip, "Sword_Overhead");
        let hit = c.clip_meta["Sword_Overhead"].event("hit").unwrap();
        let arm = c.names["upperarm_r"];
        let struck = c.local[arm].rotation;
        c.apply_clip("Sword_Overhead", hit, false, Bones::All);
        assert!(
            struck.abs_diff_eq(c.local[arm].rotation, 1e-4),
            "the blade must reach its strike as the wind-up ends"
        );
    }
    /// A swordsman with a shield, `speed` m/s forward, and a combat view with no action yet.
    /// Switches cut, to check the clip stack on its own.
    fn fighter(speed: f32) -> (Character, UnitState, UnitCombat) {
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let look = Look {
            art: "male".into(),
            drawn: true,
            gear: vec![
                ("main_hand".into(), "sword.arming".into()),
                ("off_hand".into(), "shield.round".into()),
            ],
            ..Default::default()
        };
        let mut c = Character::load(&cat, &mut ModelCache::default(), &look).unwrap();
        c.blender.set_method(anim::Method::Off);
        let u = UnitState {
            id: 1,
            kind: protocol::UnitKind::Player,
            body: "player".into(),
            name: "test".into(),
            level: 1,
            position: Vec3::ZERO,
            velocity: Vec3::new(0., 0., speed),
            yaw: 0.,
            pose: if speed > 3.2 { Pose::Run } else { Pose::Idle },
            pose_time: 0.,
            traversal: Default::default(),
            health: 100.,
            max_health: 100.,
            hostile: false,
            target: None,
            look: look.into(),
        };
        let fight = UnitCombat {
            id: 1,
            action: Action::None,
            action_time: 0.,
            cast: None,
            auras: vec![],
            resource_kind: protocol::ResourceKind::Rage,
            resource: 0.,
            max_resource: 100.,
            guarding: false,
            downed: false,
            defiance: None,
        };
        (c, u, fight)
    }
    fn sampled(c: &Character, clip: &str, bone: &str, t: f32) -> Quat {
        let track = c.clips.clips[clip]
            .tracks
            .iter()
            .find(|k| k.node == bone && k.property == Property::Rotation)
            .unwrap();
        Quat::from_array(track.sample(t).to_array()).normalize()
    }
    fn same(a: Quat, b: Quat) -> bool {
        a.dot(b).abs() > 1. - 1e-6
    }
    #[test]
    fn attacks_on_the_move_keep_the_legs_striding() {
        // A twin keeps jogging alongside, with no swing.
        let (mut c, mut u, mut fight) = fighter(4.5);
        let (mut twin, jog, calm) = fighter(4.5);
        let both = |c: &mut Character, twin: &mut Character, u: &UnitState, fight: &UnitCombat| {
            c.update_combat(u, Some(fight), 1. / 60.);
            twin.update_combat(&jog, Some(&calm), 1. / 60.);
            (c.stride.phase - twin.stride.phase).abs() < 1e-6
        };
        for _ in 0..20 {
            both(&mut c, &mut twin, &u, &fight);
        }
        u.pose = Pose::Attack;
        fight.action = Action::Slash;
        for frame in 0..30 {
            fight.action_time = frame as f32 / 60.;
            u.pose_time = fight.action_time;
            assert!(
                both(&mut c, &mut twin, &u, &fight),
                "the stride runs on through the swing"
            );
        }
        assert_eq!(c.last_clip, "Sword_Regular_A");
        assert_eq!(c.last_legs, Some("Jog_Fwd_Loop"));
        for leg in ["thigh_l", "calf_l", "foot_r"] {
            let [a, b] = [&c, &twin].map(|c| c.local[c.names[leg]].rotation);
            assert!(same(a, b), "{leg} keeps jogging under the swing");
        }
        let swing = c.clips.clips["Sword_Regular_A"].duration;
        let arm = sampled(&c, "Sword_Regular_A", "upperarm_r", 29. / 60. * swing / 0.5);
        assert!(
            same(c.local[c.names["upperarm_r"]].rotation, arm),
            "the arm swings"
        );
        u.pose = Pose::Run;
        u.pose_time = 0.;
        assert!(
            both(&mut c, &mut twin, &u, &fight),
            "the jog carries on mid-stride after the swing"
        );
        assert_eq!(c.last_clip, "Jog_Fwd_Loop");
        let (mut still, mut u, mut fight) = fighter(0.);
        u.pose = Pose::Attack;
        fight.action = Action::Slash;
        for frame in 0..30 {
            fight.action_time = frame as f32 / 60.;
            still.update_combat(&u, Some(&fight), 1. / 60.);
        }
        assert_eq!(still.last_legs, None);
        let thigh = sampled(
            &still,
            "Sword_Regular_A",
            "thigh_l",
            29. / 60. * swing / 0.5,
        );
        assert!(
            same(still.local[still.names["thigh_l"]].rotation, thigh),
            "standing swings keep their footwork"
        );
    }
    #[test]
    fn switches_carry_the_motion_on_instead_of_popping() {
        // Jog, swing on the move, jog, raise and drop the guard, stop, roll and run off. Every 20
        // frames something switches.
        let play = |method: anim::Method| {
            let (mut c, mut u, mut fight) = fighter(4.5);
            c.blender.set_method(method);
            // The blender's own smoothness: the feet go on the ground after it (#214), which has
            // tests of its own.
            c.foot_placement = false;
            let mut shown = Vec::new();
            for frame in 0..160 {
                let (pose, speed, action) = match frame / 20 {
                    1 => (Pose::Attack, 4.5, Action::Slash),
                    3 => (Pose::Run, 4.5, Action::Block),
                    5 => (Pose::Idle, 0., Action::None),
                    6 => (Pose::Dodge, 6., Action::None),
                    _ => (Pose::Run, 4.5, Action::None),
                };
                if pose != u.pose {
                    u.pose_time = 0.;
                }
                if action != fight.action {
                    fight.action_time = 0.;
                }
                (u.pose, u.velocity.z, fight.action) = (pose, speed, action);
                fight.guarding = action == Action::Block;
                c.update_combat(&u, Some(&fight), 1. / 60.);
                u.pose_time += 1. / 60.;
                fight.action_time += 1. / 60.;
                shown.push(c.local.iter().map(|l| l.rotation).collect::<Vec<_>>());
            }
            shown
        };
        // How much more or less each bone turned on a switch frame than on the frame before, at
        // worst.
        let jolt = |shown: &[Vec<Quat>], at: usize| {
            let turn = |a: &Quat, b: &Quat| {
                let d = *b * a.inverse();
                2. * d.xyz().length().atan2(d.w.abs())
            };
            (0..shown[at].len())
                .map(|b| {
                    (turn(&shown[at - 1][b], &shown[at][b])
                        - turn(&shown[at - 2][b], &shown[at - 1][b]))
                    .abs()
                })
                .fold(0., f32::max)
        };
        let [cut, smooth, dead] = [
            anim::Method::Off,
            anim::Method::Inertialize,
            anim::Method::DeadBlend,
        ]
        .map(play);
        for at in [20, 40, 60, 80, 100, 120, 140] {
            assert!(jolt(&cut, at) > 0.5, "cut, frame {at} pops");
            assert!(
                jolt(&smooth, at) < 1e-5,
                "frame {at} jolts {}",
                jolt(&smooth, at)
            );
            // Dead blending lets bones the switch barely moved take the new clip's speed at once.
            assert!(
                jolt(&dead, at) < 0.01,
                "dead blending, frame {at}: {}",
                jolt(&dead, at)
            );
        }
    }
    #[test]
    fn moving_swings_face_where_the_clip_does_within_reach_of_the_hips() {
        // How far round the chest and hips of each character face, against their rest pose.
        let facing = |c: &Character| {
            let [hips, chest] = ["pelvis", "spine_03"].map(|n| c.names[n]);
            [hips, chest]
                .map(|i| heading(c.rotation_to(i, None, false), c.rotation_to(i, None, true)))
        };
        for (action, clip) in [
            (Action::Bash, "Shield_OneShot"),
            (Action::Slash, "Sword_Regular_A"),
        ] {
            let (mut moving, mut u, mut fight) = fighter(4.5);
            let (mut still, mut s, _) = fighter(0.);
            u.pose = Pose::Attack;
            s.pose = Pose::Attack;
            fight.action = action;
            let mut capped = false;
            for frame in 0..30 {
                fight.action_time = frame as f32 / 60.;
                moving.update_combat(&u, Some(&fight), 1. / 60.);
                still.update_combat(&s, Some(&fight), 1. / 60.);
                assert_eq!(moving.last_clip, clip);
                let [hips, chest] = facing(&moving);
                let [clip_hips, clip_chest] = facing(&still);
                // The chest faces where the clip's does, unless that twists it further round
                // from the jogging hips than MAX_TWIST (or the clip's own twist).
                let own = wrap(clip_chest - clip_hips);
                let wanted = wrap(clip_chest - hips);
                let twist = wanted.clamp(own.min(-MAX_TWIST), own.max(MAX_TWIST));
                assert!(
                    wrap(chest - hips - twist).abs() < 1e-3,
                    "{clip} at frame {frame}: twist {} for {}",
                    wrap(chest - hips).to_degrees(),
                    twist.to_degrees()
                );
                capped |= twist != wanted;
            }
            assert_eq!(
                capped,
                clip == "Sword_Regular_A",
                "{clip}: only the pivot is capped"
            );
        }
    }
    /// A swordsman travelling at `speed` m/s, `travel` degrees round from the way they face.
    fn traveller(speed: f32, travel: f32) -> (Character, UnitState) {
        let (c, mut u, _) = fighter(0.);
        u.yaw = 0.3;
        let way = u.yaw + travel.to_radians();
        u.velocity = Vec3::new(way.sin(), 0., way.cos()) * speed;
        u.pose = if speed > 3.2 { Pose::Run } else { Pose::Walk };
        (c, u)
    }
    /// The furthest a planted foot slides over the ground in one contact, travelling at `speed`
    /// m/s `travel` degrees round from the facing, as the stride plays the clips (#110): without
    /// foot locking (#214) to pin the feet. A foot is planted while the ball of the foot is within
    /// 1 cm of the lowest it gets.
    fn foot_skate(speed: f32, travel: f32) -> f32 {
        let (mut c, mut u) = traveller(speed, travel);
        c.foot_placement = false;
        let feet = ["ball_l", "ball_r"].map(|n| c.names[n]);
        let mut track = Vec::new();
        for frame in 0..240 {
            c.update(&u, 1. / 60.);
            u.position += u.velocity / 60.;
            let root =
                Mat4::from_rotation_translation(Quat::from_rotation_y(c.body_yaw()), u.position);
            if frame >= 60 {
                track.push(feet.map(|i| root.transform_point3(c.world[i].w_axis.truncate())));
            }
        }
        let mut worst = 0f32;
        for foot in 0..2 {
            let low = track.iter().map(|t| t[foot].y).fold(f32::MAX, f32::min);
            let mut planted = None;
            for at in track.iter().map(|t| t[foot]) {
                if at.y < low + 0.01 {
                    let from = *planted.get_or_insert(at);
                    worst = worst.max((at - from).with_y(0.).length());
                } else {
                    planted = None;
                }
            }
        }
        worst
    }
    #[test]
    fn planted_feet_stay_put_walking_running_and_sprinting_every_way() {
        // Ahead, to either side and backward, at the sim's speeds (backpedalling at 0.6 of them).
        for (speed, gait) in [(2.5, "walk"), (6., "run"), (9., "sprint")] {
            for travel in [0., 90., -90., 180.] {
                if gait == "walk" && travel == 180. {
                    continue;
                }
                let speed = if travel == 180. { speed * 0.6 } else { speed };
                let skate = foot_skate(speed, travel);
                assert!(
                    skate < 0.02,
                    "{gait} at {travel}°: planted feet slide {:.1} cm",
                    skate * 100.
                );
            }
        }
        // Backing off at a walk (1.5 m/s, as behind a raised guard) is mostly the walk clip,
        // whose trailing foot slides as it lifts even at the clip's own pace (4 cm on this body).
        // The blend mustn't make that worse; foot locking (#214) pins it (see
        // `held_feet_stay_put_through_each_contact_every_way`).
        let own = traveller(0., 0.).0.gaits.unwrap()[0].speed();
        let (back, clip) = (foot_skate(1.5, 180.), foot_skate(own, 0.));
        assert!(
            back <= clip,
            "backing off at a walk: {:.1} cm, the walk clip alone {:.1} cm",
            back * 100.,
            clip * 100.
        );
    }
    #[test]
    fn held_feet_stay_put_through_each_contact_every_way() {
        // With foot locking on (#214), at the sim's speeds ahead, to either side and backward
        // (backpedalling at 0.6 of them, and backing off at a walk): each foot holds in every
        // stride, for at least 4 frames each time, and moves under 5 mm over the ground held.
        for (speed, travel) in [
            (2.5, 0.),
            (2.5, 90.),
            (1.5, 180.),
            (6., 0.),
            (6., -90.),
            (3.6, 180.),
            (9., 0.),
            (9., 90.),
            (5.4, 180.),
        ] {
            let (mut c, mut u) = traveller(speed, travel);
            let feet = ["ball_l", "ball_r"].map(|n| c.names[n]);
            let mut from: [Option<Vec3>; 2] = [None; 2];
            let [mut holds, mut frames] = [[0; 2]; 2];
            let mut worst = 0f32;
            for frame in 0..240 {
                c.update(&u, 1. / 60.);
                let root = Mat4::from_rotation_translation(
                    Quat::from_rotation_y(c.body_yaw()),
                    u.position,
                );
                u.position += u.velocity / 60.;
                let held = c.feet.held();
                for s in 0..2 {
                    if frame < 60 || !held[s] {
                        from[s] = None;
                        continue;
                    }
                    let at = root.transform_point3(c.world[feet[s]].w_axis.truncate());
                    holds[s] += usize::from(from[s].is_none());
                    frames[s] += 1;
                    let from = *from[s].get_or_insert(at);
                    worst = worst.max((at - from).with_y(0.).length());
                }
            }
            assert!(
                worst < 0.005,
                "{speed} m/s at {travel}°: a held foot moved {:.1} mm",
                worst * 1000.
            );
            for s in 0..2 {
                assert!(
                    holds[s] >= 2 && frames[s] >= 4 * holds[s],
                    "{speed} m/s at {travel}°: foot {s} held {} frames over {} contacts",
                    frames[s],
                    holds[s]
                );
            }
        }
    }
    #[test]
    fn on_a_wall_the_feet_leave_the_ground_even_taking_a_blow() {
        // Hanging low on a wall (#247), the feet 10 cm off the ground, and struck there, so the
        // sim holds its hit pose: the feet don't reach for the ground, and the placement fades
        // out as it does in the air.
        let (mut c, mut u, _) = fighter(0.);
        c.set_ground(Arc::new(|_, _| 0.));
        for _ in 0..30 {
            c.update(&u, 1. / 60.);
        }
        assert!(
            c.feet.weight() > 0.99,
            "standing, placed {}",
            c.feet.weight()
        );
        (u.position.y, u.pose, u.traversal) = (0.1, Pose::Hit, Traversal::Hanging);
        for _ in 0..20 {
            c.update(&u, 1. / 60.);
        }
        assert_eq!(c.last_clip, "Hang_Idle_Loop");
        assert_eq!(c.feet.weight(), 0., "the feet still reached for the ground");
    }
    #[test]
    fn a_cape_with_bones_of_its_own_still_lets_the_loops_be_measured() {
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let load = |gear: Vec<(String, String)>| {
            let look = Look {
                art: "male".into(),
                gear,
                ..Default::default()
            };
            // A cache of its own, so this body is the first to play the clips and measures them.
            Character::load(&cat, &mut ModelCache::default(), &look).unwrap()
        };
        let caped = load(vec![("back".into(), "cape.traveller".into())]);
        assert!(
            caped.nodes.len() > load(vec![]).nodes.len(),
            "the cape brings bones"
        );
        assert!(caped.gaits.is_some(), "the loops measure under a cape");
    }
    #[test]
    fn strafing_turns_the_hips_and_keeps_the_chest_to_the_front() {
        // How far round the hips and chest face, against their rest pose.
        let facing = |c: &Character| {
            ["pelvis", "spine_03"].map(|n| {
                let i = c.names[n];
                heading(c.rotation_to(i, None, false), c.rotation_to(i, None, true))
            })
        };
        // Each strafe beside a twin going straight ahead or straight back, whose legs keep the
        // same stride: the strafer's hips turn toward the travel, and the chest turns back.
        for (travel, straight, turn) in [
            (45., 0., 45.),
            (-45., 0., -45.),
            (90., 0., 90.),
            (-90., 0., -90.),
            (135., 180., -45.),
            (-135., 180., 45.),
        ] {
            let speed = if straight == 180. { 3.6 } else { 6. };
            let ((mut c, u), (mut twin, v)) =
                (traveller(speed, travel), traveller(speed, straight));
            let mut capped = false;
            for frame in 0..60 {
                c.update(&u, 1. / 60.);
                twin.update(&v, 1. / 60.);
                let [hips, chest] = facing(&c);
                let [twin_hips, twin_chest] = facing(&twin);
                assert!(
                    wrap(hips - twin_hips - f32::to_radians(turn)).abs() < 1e-3,
                    "{travel}° at frame {frame}: hips {}° round from the twin's",
                    wrap(hips - twin_hips).to_degrees()
                );
                // The chest faces where the twin's does, unless that twists it further round from
                // the hips than MAX_TWIST (or the clip's own twist).
                let own = wrap(twin_chest - twin_hips);
                let wanted = wrap(twin_chest - hips);
                let twist = wanted.clamp(own.min(-MAX_TWIST), own.max(MAX_TWIST));
                assert!(
                    wrap(chest - hips - twist).abs() < 1e-3,
                    "{travel}° at frame {frame}: twist {}° for {}°",
                    wrap(chest - hips).to_degrees(),
                    twist.to_degrees()
                );
                capped |= twist != wanted;
            }
            // Square to the facing, the hips turn further round than the chest may follow.
            assert!(
                capped || travel.abs() != 90.,
                "{travel}°: the twist is capped"
            );
        }
    }
    /// Turns a standing character on the spot for `frames` frames, the sim's yaw at `yaw(t)`.
    /// Returns each frame's body yaw, the chest's heading in the world, and the ankles there.
    fn turning(
        c: &mut Character,
        u: &mut UnitState,
        frames: usize,
        yaw: impl Fn(f32) -> f32,
    ) -> Vec<(f32, f32, [Vec3; 2])> {
        let [chest, left, right] = ["spine_03", "foot_l", "foot_r"].map(|n| c.names[n]);
        let rest = c.rotation_to(chest, None, true);
        (0..frames)
            .map(|k| {
                u.yaw = yaw(k as f32 / 60.);
                u.pose_time += 1. / 60.;
                c.update(u, 1. / 60.);
                let body = c.body_yaw();
                let root = Mat4::from_rotation_translation(Quat::from_rotation_y(body), u.position);
                let heading = body + heading(c.rotation_to(chest, None, false), rest);
                let ankles =
                    [left, right].map(|i| root.transform_point3(c.world[i].w_axis.truncate()));
                (body, heading, ankles)
            })
            .collect()
    }
    #[test]
    fn turning_on_the_spot_the_feet_stay_put_while_the_chest_takes_the_turn() {
        // A twin that stands still shows where the idle has the feet and chest each frame.
        let [(mut c, mut u, _), (mut twin, mut v, _)] = [fighter(0.), fighter(0.)];
        for w in [&mut u, &mut v] {
            w.position = Vec3::new(3., 0., -2.);
        }
        turning(&mut c, &mut u, 30, |_| 0.3);
        turning(&mut twin, &mut v, 30, |_| 0.3);
        // The camera swings 50° left over a third of a second, and holds a moment.
        let frames = turning(&mut c, &mut u, 36, |t| 0.3 + 0.87 * (t / 0.33).min(1.));
        let still = turning(&mut twin, &mut v, 36, |_| 0.3);
        for (k, (now, idle)) in frames.iter().zip(&still).enumerate() {
            assert!(
                (now.0 - 0.3).abs() < 1e-5,
                "frame {k}: the body turned to {}",
                now.0
            );
            for f in 0..2 {
                let moved = now.2[f].distance(idle.2[f]);
                assert!(
                    moved < 1e-4,
                    "frame {k}: a foot moved {:.1} mm",
                    moved * 1000.
                );
            }
        }
        // The chest came round with the aim.
        let turned = wrap(frames[35].1 - still[35].1);
        assert!(
            (turned - 0.87).abs() < 0.01,
            "the chest turned {}°",
            turned.to_degrees()
        );
    }
    #[test]
    fn past_the_limit_the_feet_step_round_and_land_where_the_clip_stands_them() {
        let (mut c, mut u, _) = fighter(0.);
        // The feet go back to the clip through the blender, as in the game.
        c.blender.set_method(anim::Method::Inertialize);
        turning(&mut c, &mut u, 30, |_| 0.3);
        // 115° left at once, as when a target is picked behind.
        let target = 0.3 + 2.;
        let frames = turning(&mut c, &mut u, 150, |_| target);
        let low = [0, 1].map(|f| frames.iter().map(|x| x.2[f].y).fold(f32::MAX, f32::min));
        let mut steps = [0; 2];
        for pair in frames.windows(2) {
            let up = |x: &(f32, f32, [Vec3; 2]), f: usize| x.2[f].y > low[f] + 0.005;
            assert!(
                !(up(&pair[1], 0) && up(&pair[1], 1)),
                "both feet in the air"
            );
            for (f, taken) in steps.iter_mut().enumerate() {
                if !up(&pair[0], f) && !up(&pair[1], f) {
                    let slid = (pair[1].2[f] - pair[0].2[f]).with_y(0.).length();
                    assert!(slid < 0.002, "a planted foot slid {:.1} mm", slid * 1000.);
                }
                *taken += (!up(&pair[0], f) && up(&pair[1], f)) as usize;
            }
        }
        assert!(steps[0] >= 1 && steps[1] >= 1, "steps {steps:?}");
        // The body faces the sim's yaw, and the feet stand where the idle has them.
        let (body, _, feet) = frames[149];
        assert!(
            wrap(body - target).abs() < 0.01,
            "body at {}°",
            body.to_degrees()
        );
        assert!(c.steps.is_none(), "the feet are back on the clip");
        // A twin standing at that yaw from the start, on the same frame of the idle.
        let (mut twin, mut v, _) = fighter(0.);
        v.yaw = body;
        v.pose_time = u.pose_time - 1. / 60.;
        let clip = turning(&mut twin, &mut v, 1, |_| body)[0].2;
        for f in 0..2 {
            let off = feet[f].distance(clip[f]);
            assert!(
                off < 0.003,
                "a foot ended {:.1} cm from the idle's",
                off * 100.
            );
        }
    }
    #[test]
    fn an_attack_turns_the_body_to_face_where_it_strikes() {
        let (mut c, mut u, mut fight) = fighter(0.);
        u.yaw = 0.3;
        for _ in 0..30 {
            c.update_combat(&u, Some(&fight), 1. / 60.);
        }
        // 50° left: the chest takes it, the feet stay.
        u.yaw = 1.2;
        for _ in 0..20 {
            c.update_combat(&u, Some(&fight), 1. / 60.);
        }
        assert!((c.body_yaw() - 0.3).abs() < 1e-5);
        assert!(c.turn.offset() > 0.8);
        // A slash owns the upper body, so the body comes round to face it and the twist goes.
        u.pose = Pose::Attack;
        fight.action = Action::Slash;
        for frame in 0..24 {
            fight.action_time = frame as f32 / 60.;
            u.pose_time = fight.action_time;
            c.update_combat(&u, Some(&fight), 1. / 60.);
        }
        assert!(wrap(c.body_yaw() - 1.2).abs() < 0.02, "{}", c.body_yaw());
        assert!(c.turn.offset().abs() < 0.02);
    }
    /// Animates `c` at `tier` for `frames` frames from `frame`, running on with `u`; returns
    /// where bone `bone` is each frame, in the model's space, and which frames rebuilt.
    fn animated(
        c: &mut Character,
        u: &mut UnitState,
        fight: &UnitCombat,
        tier: anim::Tier,
        frames: std::ops::Range<u64>,
        bone: &str,
    ) -> Vec<(Vec3, bool)> {
        let i = c.names[bone];
        c.set_tier(tier);
        frames
            .map(|frame| {
                u.pose_time += 1. / 60.;
                let rebuilt = c.animate(u, Some(fight), 1. / 60., frame);
                (c.world[i].w_axis.truncate(), rebuilt)
            })
            .collect()
    }
    #[test]
    fn half_rate_blends_the_frames_between_a_rebuild_behind_and_quarter_rate_holds() {
        let (mut full, mut u, fight) = fighter(6.);
        let [(mut half, mut v, _), (mut quarter, mut w, _)] = [fighter(6.), fighter(6.)];
        let full = animated(
            &mut full,
            &mut u,
            &fight,
            anim::Tier::Full,
            0..120,
            "hand_r",
        );
        let half = animated(
            &mut half,
            &mut v,
            &fight,
            anim::Tier::Half,
            0..120,
            "hand_r",
        );
        let quarter = animated(
            &mut quarter,
            &mut w,
            &fight,
            anim::Tier::Quarter,
            0..120,
            "hand_r",
        );
        // Every other frame and every fourth, and the first.
        let rebuilds = |frames: &[(Vec3, bool)]| frames.iter().filter(|f| f.1).count();
        assert_eq!((rebuilds(&half), rebuilds(&quarter)), (61, 31));
        // Running, the hand swings through every frame at half rate, a rebuild (two frames)
        // behind; it never sits still or jumps.
        let fastest = |frames: &[(Vec3, bool)]| {
            let steps = frames[20..].windows(2).map(|p| p[0].0.distance(p[1].0));
            steps.fold((f32::MAX, 0f32), |(lo, hi), d| (lo.min(d), hi.max(d)))
        };
        let (_, full_most) = fastest(&full);
        let (half_least, half_most) = fastest(&half);
        assert!(
            half_most < full_most * 1.25,
            "{half_most} against {full_most}"
        );
        assert!(half_least > 1e-5, "the hand stopped for a frame");
        for k in 20..120 {
            let behind = half[k].0.distance(full[k - 2].0);
            assert!(behind < 0.01, "frame {k}: {:.1} cm off", behind * 100.);
        }
        // At quarter rate it holds between rebuilds.
        for p in quarter.windows(2).filter(|p| !p[1].1) {
            assert_eq!(p[0].0, p[1].0);
        }
    }
    #[test]
    fn a_new_character_shows_its_pose_on_its_first_frame_at_any_tier() {
        let (mut full, mut u, fight) = fighter(6.);
        let posed = animated(&mut full, &mut u, &fight, anim::Tier::Full, 0..1, "hand_r");
        for tier in [anim::Tier::Half, anim::Tier::Quarter] {
            // Unit 1 isn't due on frame 0 at either rate.
            assert!(!tier.due(0, 1));
            let (mut c, mut v, _) = fighter(6.);
            let shown = animated(&mut c, &mut v, &fight, tier, 0..1, "hand_r");
            assert!(shown[0].1, "{tier:?} didn't build its first pose");
            let off = shown[0].0.distance(posed[0].0);
            assert!(off < 1e-5, "{tier:?}: the hand {off} m from its pose");
        }
    }
    #[test]
    fn below_full_detail_the_cape_fades_to_its_pose_and_the_feet_dont_step() {
        let (_, mut u, fight) = fighter(6.);
        std::sync::Arc::make_mut(&mut u.look)
            .gear
            .push(("back".into(), "cape.traveller".into()));
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let mut c = Character::load(&cat, &mut ModelCache::default(), &u.look).unwrap();
        c.blender.set_method(anim::Method::Off);
        let tip = "cape_2_4";
        let swinging = animated(&mut c, &mut u, &fight, anim::Tier::Full, 0..90, tip);
        let fading = animated(&mut c, &mut u, &fight, anim::Tier::Half, 90..150, tip);
        // The cape eases down to the pose over half a second, its tip going no more than a
        // tenth of the way in any frame.
        let most = |frames: &[(Vec3, bool)]| {
            let steps = frames.windows(2).map(|p| p[0].0.distance(p[1].0));
            steps.fold(swinging[89].0.distance(frames[0].0), f32::max)
        };
        let far = swinging[89].0.distance(fading[59].0);
        let fade = most(&fading);
        assert!(far > 0.3 && fade < far * 0.1, "{fade} of {far}");
        // Then the springs are off: the cape bones are as the pose has them.
        for (i, n) in c
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.name.starts_with("cape_"))
        {
            assert_eq!(c.local[i].rotation, n.rest.rotation, "{}", n.name);
        }
        // Standing and turning, the body follows the sim's yaw on its spring and the feet don't
        // step round.
        u.velocity = Vec3::ZERO;
        u.pose = Pose::Idle;
        u.yaw = 2.;
        for frame in 150..210 {
            u.pose_time += 1. / 60.;
            c.animate(&u, Some(&fight), 1. / 60., frame);
            assert!(c.steps.is_none());
        }
        assert!(wrap(c.body_yaw() - 2.).abs() < 0.05, "{}", c.body_yaw());
    }
    /// How clips were applied before #204, looking each track's bone up by name.
    fn apply_by_name(c: &mut Character, name: &str, t: f32, looping: bool, bones: Bones) {
        let clip = c.clips.clips[name].clone();
        let t = if looping {
            t.rem_euclid(clip.duration.max(0.001))
        } else {
            t.clamp(0., clip.duration)
        };
        let upper: Vec<bool> = (0..c.nodes.len())
            .map(|mut i| {
                loop {
                    if c.nodes[i].name == "spine_01" {
                        return c.humanoid;
                    }
                    match c.nodes[i].parent {
                        Some(p) => i = p,
                        None => return false,
                    }
                }
            })
            .collect();
        for track in &clip.tracks {
            let Some(&i) = c.names.get(&track.node) else {
                continue;
            };
            if bones != Bones::All && upper[i] != (bones == Bones::Upper) {
                continue;
            }
            let v = track.sample(t);
            match track.property {
                Property::Rotation => c.local[i].rotation = Quat::from_vec4(v),
                Property::Translation if !c.humanoid => c.local[i].translation = v.truncate(),
                Property::Translation if track.node == "pelvis" => {
                    c.local[i].translation = c.nodes[i].rest.translation
                        + (v.truncate() - c.reference_pelvis) * c.pelvis_ratio
                }
                Property::Scale if !c.humanoid => c.local[i].scale = v.truncate(),
                _ => {}
            }
        }
    }
    #[test]
    fn bound_clips_pose_exactly_as_looking_bones_up_by_name() {
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let mut cache = ModelCache::default();
        let knight = Look {
            art: "male".into(),
            drawn: true,
            gear: vec![
                ("main_hand".into(), "sword.arming".into()),
                ("off_hand".into(), "shield.round".into()),
            ],
            ..Default::default()
        };
        let wolf = Look {
            art: "wolf.grey".into(),
            ..Default::default()
        };
        for look in [knight, wolf] {
            let mut c = Character::load(&cat, &mut cache, &look).unwrap();
            // LOD 2 swaps in a beast's own LOD clips, bound to the skeleton it was built on.
            for lod in [0, 2] {
                c.set_lod(&cat, &mut cache, lod).unwrap();
                // Off the rest pose, so a bone that one way sets and the other doesn't shows.
                let start: Vec<Transform> = c
                    .local
                    .iter()
                    .enumerate()
                    .map(|(i, l)| Transform {
                        translation: l.translation + Vec3::splat(0.01 * i as f32),
                        rotation: (Quat::from_rotation_x(0.1 * i as f32) * l.rotation).normalize(),
                        scale: Vec3::splat(1.5),
                    })
                    .collect();
                let mut names: Vec<String> = c.clips.clips.keys().cloned().collect();
                names.sort();
                for name in &names {
                    let duration = c.clips.clips[name].duration;
                    for bones in [Bones::All, Bones::Upper, Bones::Lower] {
                        for (at, looping) in [(0., false), (0.37, true), (1.6, true), (1.6, false)]
                        {
                            let t = at * duration;
                            c.local.clone_from(&start);
                            c.apply_clip(name, t, looping, bones);
                            let bound = c.local.clone();
                            c.local.clone_from(&start);
                            apply_by_name(&mut c, name, t, looping, bones);
                            for (i, (a, b)) in bound.iter().zip(&c.local).enumerate() {
                                assert!(
                                    a.rotation == b.rotation
                                        && a.translation == b.translation
                                        && a.scale == b.scale,
                                    "{} lod {lod} {name} at {t}: bone {} differs",
                                    look.art,
                                    c.nodes[i].name
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
