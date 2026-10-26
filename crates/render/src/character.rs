//! Catalogue assembly and animation. The workshop is the reference, not filenames or rig order.
use crate::model::*;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec2, Vec3, Vec3Swizzles};
use protocol::{
    Action, ForcedKind, ForcedMove, HitWeight, Look, Pose, Traversal, UnitCombat, UnitState,
};
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
    /// Whether the body leans with its acceleration (#217); off to compare.
    pub leaning: bool,
    /// Whether blows tip the body and hitstops hold its poses (#218); off to compare with the
    /// hit clip playing in full.
    pub flinching: bool,
    /// Whether rolls, leaps, charges and climbs play by how far the body has come through them,
    /// and swimmers swim (#213), and whether climbers' clips play by how far they have come and
    /// slid, with their hands on the hold and feet on the wall (#248); off to compare with the
    /// clips by the clock.
    pub traversing: bool,
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
    /// Where it is on a wall this frame (#248); the shimmy loop's phase, by the distance slid
    /// along the wall; how far it's moved, up and in toward the wall, for its hands to reach the
    /// hold; and how firmly each hand holds it, by `Side`.
    on_wall: Option<OnWall>,
    shimmy: f32,
    raised: Vec3,
    hold_grip: [f32; 2],
    /// How far this frame moves to reach a hold from its hang (#248), and the client's holds.
    hang_reach: Vec3,
    holds: Option<HoldAt>,
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
    /// Where the character looks (#212), in the world, as the client last said; and the spine,
    /// neck and head following it.
    gaze_at: Option<Vec3>,
    gaze: anim::Gaze,
    /// How the body leans with its acceleration (#217), and the tilt it took this frame.
    lean: anim::Lean,
    leaned: Vec2,
    /// Its flights and touchdowns, and the pelvis's dip on landing (#217).
    landing: anim::Landing,
    /// Seconds into `Jump_Start`, and on into `Jump_Loop`, while in the air (#217).
    air: Option<f32>,
    /// Seconds into `Jump_Land` after a fall that hurt, played in full while standing (#217).
    hard_landing: Option<f32>,
    /// Flinches away from blows, and the hitstop that holds the pose clocks as they land (#218).
    flinch: anim::Flinch,
    /// Whether the blow the sim's `Hit` pose is for caught it in the air, where it plays its hit
    /// in full as before rather than flinching.
    hit_in_air: bool,
    /// How far through the move it is carried through the body has come (#213).
    progress: anim::Progress,
    /// The clip of the move the body is carried through, or was last, and how far into it (#213).
    carried: Option<Carried>,
    /// Where the climb under way goes over its edge, which the hands take hold of, by the climb's
    /// start, and how firmly each hand holds it, by `Side`.
    edge: Option<(f64, Vec3)>,
    grip: [f32; 2],
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
/// The hold a body hanging at a point hangs from (#248), from the client's copy of the world's
/// holds: the one `sim::climbing::hang_point` would hang it from there. `None` where there's none.
pub type HoldAt = Arc<dyn Fn(Vec3) -> Option<world::Hold> + Send + Sync>;

/// A climbing clip under way (#247): the sim's move onto, between or off holds that it plays,
/// what the body does in it, and how far through the move the body has come (#248).
struct ClimbMove {
    clip: &'static str,
    wall: anim::climb::Wall,
    progress: anim::Progress,
}
/// Where a body is on a wall this frame (#248): what it's doing and how far through, and where it
/// hangs, or where its move goes from and to, which find the holds its hands may be on.
#[derive(Clone, Copy)]
struct OnWall {
    wall: anim::climb::Wall,
    k: f32,
    at: [Vec3; 2],
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
/// A move's clip and how far into it (#213). Through the move it plays the moment that matches how
/// far the body has come; once the move is over, the rest of it plays on by the clock until it ends
/// or something else plays: standing up out of a roll, a dash's blow and recovery.
#[derive(Clone, Copy)]
struct Carried {
    /// When the move began, in sim time (as `ForcedMove::start`).
    start: f64,
    clip: &'static str,
    /// Seconds into the clip, and where its stretch covering the move ends.
    time: f32,
    end: f32,
    /// The way a roll went, which the body faces until it has stood up out of it.
    heading: Option<f32>,
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
/// A point closer to the head than this, in metres, isn't looked at (#212): it would swing the
/// head about for nothing.
const NEAREST_GAZE: f32 = 0.5;
/// The humanoid jump (#217). `Jump_Start` crouches for its first two frames and pushes off, but
/// the sim's body leaves the ground at once, so the clip starts here, the legs straight...
const TAKEOFF: f32 = 0.07;
/// ...and the legs tuck up highest here, which comes at the top of the jump.
const TUCK: f32 = 0.3;
/// How much of a lean the pelvis takes, tipping the whole body about the ground beneath it; the
/// spine bends the rest of the way (#217).
const LEAN_BODY: f32 = 0.6;
/// The humanoid locomotion loops, slowest first, which blend by speed (#110).
const LOOPS: [&str; 3] = ["Walk_Loop", "Jog_Fwd_Loop", "Sprint_Loop"];
/// How far either side of a climb's line the hands take hold of its edge, in metres (#213).
const HAND_SPREAD: f32 = 0.25;
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
/// Whether `u` is being thrown by a blow (#192's traversal state): a knockback plays its clip.
fn thrown(u: &UnitState) -> bool {
    matches!(u.traversal, Traversal::Forced(m) if m.kind == ForcedKind::Knockback)
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
            leaning: true,
            flinching: true,
            traversing: true,
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
            on_wall: None,
            shimmy: 0.,
            raised: Vec3::ZERO,
            hold_grip: [0.; 2],
            hang_reach: Vec3::ZERO,
            holds: None,
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
            gaze_at: None,
            gaze: anim::Gaze::default(),
            lean: anim::Lean::default(),
            leaned: Vec2::ZERO,
            landing: anim::Landing::default(),
            air: None,
            hard_landing: None,
            flinch: anim::Flinch::default(),
            hit_in_air: false,
            progress: anim::Progress::default(),
            carried: None,
            edge: None,
            grip: [0.; 2],
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
        c.hang_reach = *set.reach.get_or_init(|| c.measure_reach());
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
        // Listed now, so that the first landing (#217) or slope, where the feet first reach for
        // the ground and the pelvis first drops, allocates nothing.
        c.find_children();
        if let Some([pelvis, ..]) = c.spine_bones {
            c.list_on(pelvis);
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
    /// Puts on `look` in place (#239), as when a character in view changes gear or clothes. The
    /// clip clocks, the stride, the blender, the feet, the turn, the gaze and the cape springs
    /// carry on, where building the character again would start them all over: the cape snapping
    /// back to rest and whatever the new gear plays differently cutting in. Bones carry over by
    /// name, so gear that brings bones of its own or takes them away leaves the rest moving, and a
    /// cape's springs keep their swing on every bone the new look still has. The change counts as
    /// a switch, so the new gear's clips and grips blend in. Another body (another art id) has
    /// nothing to carry on and starts afresh.
    pub fn set_look(
        &mut self,
        catalog: &assets::Catalog,
        cache: &mut ModelCache,
        look: &Look,
    ) -> Result<()> {
        let mut next = Self::load_lod(catalog, cache, look, self.lod)?;
        if look.art != self.look.art {
            (next.partial_updates, next.foot_placement) =
                (self.partial_updates, self.foot_placement);
            (next.leaning, next.flinching, next.tier) = (self.leaning, self.flinching, self.tier);
            next.traversing = self.traversing;
            (next.ground, next.holds) = (self.ground.take(), self.holds.take());
            next.blender.set_method(self.blender.method());
            *self = next;
            return Ok(());
        }
        // Where each of the new skeleton's bones was in the old one, if it was there.
        let was: Vec<_> = next
            .nodes
            .iter()
            .map(|n| self.names.get(&n.name).copied())
            .collect();
        // The bones that carry on as they were. One that rests elsewhere now belongs to other
        // geometry (another cape on bones of the same names) and starts from its rest, as a new
        // bone does.
        let kept: Vec<_> = (0..was.len())
            .map(|i| {
                was[i].filter(|&j| {
                    let (old, new) = (self.nodes[j].rest, next.nodes[i].rest);
                    (old.translation, old.rotation, old.scale)
                        == (new.translation, new.rotation, new.scale)
                })
            })
            .collect();
        let same = kept.len() == self.nodes.len() && (0..kept.len()).all(|i| kept[i] == Some(i));
        for (i, &kept) in kept.iter().enumerate() {
            if let Some(j) = kept {
                next.local[i] = self.local[j];
            }
        }
        if !same {
            self.blender.remap(&next.local, |i| kept[i]);
            // At half rate, the blend between rebuilds starts again from the next.
            self.between = None;
        }
        // Springs take up the swing on every bone of the same name, even another cape's.
        for springs in &mut next.springs {
            springs.carry_on(&self.springs, |bone| was[bone]);
        }
        // The new look's skeleton, geometry, gear and the clips that go with them.
        (self.parts, self.nodes, self.names, self.local) =
            (next.parts, next.nodes, next.names, next.local);
        (self.order, self.children, self.dropped) = (next.order, next.children, next.dropped);
        (self.clips, self.clip_set, self.skeleton) = (next.clips, next.clip_set, next.skeleton);
        (self.clip_meta, self.gaits, self.idle) = (next.clip_meta, next.gaits, next.idle);
        self.hang_reach = next.hang_reach;
        (self.main, self.main_clips) = (next.main, next.main_clips);
        (self.off_hand_clips, self.main_sockets) = (next.off_hand_clips, next.main_sockets);
        (self.leg_bones, self.ball_bones) = (next.leg_bones, next.ball_bones);
        (self.spine_bones, self.head_bones, self.arm_bones) =
            (next.spine_bones, next.head_bones, next.arm_bones);
        (self.springs, self.look) = (next.springs, next.look);
        // Other levels of detail kept the old look's geometry.
        self.lod_parts.clear();
        self.grab_offset = None;
        if !same {
            // Drawn as it stands until the next rebuild, which at a lower tier may be frames away.
            self.world.resize(self.nodes.len(), Mat4::IDENTITY);
            self.pose_world();
        }
        self.forced_switch = Some(anim::Switch::Gear);
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
        let carried = self.carried_by(u);
        // Rushing at a target (#213), the main item's dash, whatever the blow: its lunge carries
        // the body there, and its blow lands as it arrives.
        if carried.is_some_and(|m| m.kind == ForcedKind::Charge)
            && let Some(dash) = self.main_clips.get("dash")
        {
            return Some(dash);
        }
        let pose = self.shown_pose(u);
        if !matches!(
            pose,
            Pose::Attack | Pose::Cast | Pose::Hit | Pose::Jump | Pose::Run
        ) {
            return None;
        }
        // The sim's pose for a leap or a charge plays its action only while the body is carried
        // through it (#213), not on a jump or a run after.
        let stale =
            |kind| self.traversing && self.humanoid && carried.is_none_or(|m| m.kind != kind);
        if pose == Pose::Run && (c.action != Action::Charge || stale(ForcedKind::Charge))
            || pose == Pose::Jump && (c.action != Action::Leap || stale(ForcedKind::Leap))
            || pose == Pose::Hit && c.action != Action::Stagger
        {
            return None;
        }
        // Out of a leap or a charge (#213), its clip plays on while the pose of the action that
        // began it holds: a new action ends it.
        if carried.is_none()
            && let Some(c) = self.carried
        {
            return Some(c.clip);
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
            // A hop back (#213) springs off without lunging.
            Action::Leap
                if carried.is_some_and(|m| {
                    (m.to - m.from)
                        .xz()
                        .dot(Vec2::new(u.yaw.sin(), u.yaw.cos()))
                        < 0.
                }) =>
            {
                "NinjaJump_Start"
            }
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
        let pose = self.shown_pose(u);
        if !self.humanoid {
            let role = match pose {
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
        match pose {
            Pose::Idle if self.hard_landing.is_some() => "Jump_Land",
            // Afloat (#213), the swim loops.
            Pose::Idle if self.swimming(u) => "Swim_Idle_Loop",
            Pose::Walk | Pose::Run if self.swimming(u) => "Swim_Fwd_Loop",
            Pose::Idle => match self.carried {
                // Out of a roll or a dash (#213), the rest of its clip.
                Some(c) => c.clip,
                None if u.look.drawn => self.main_clips.get("idle").unwrap_or(self.idle),
                None => self.idle,
            },
            Pose::Walk | Pose::Run => locomotion_clip(pose, u.velocity.xz().length()),
            Pose::Jump if self.pulling_up(u) => "ClimbUp_1m",
            // One timeline through the air (#217): the take-off, then the loop.
            Pose::Jump | Pose::Fall => match self.air {
                Some(t) if t < self.clip_duration("Jump_Start") => "Jump_Start",
                _ => "Jump_Loop",
            },
            // Hanging borrows the falling loop until the climbing clips land (#247).
            Pose::Hang => "Jump_Loop",
            Pose::Dodge => "Roll",
            Pose::Attack | Pose::Cast => {
                let attacks = self.main_clips.list("attacks");
                match attacks.get(self.attack % attacks.len().max(1)) {
                    Some(&attack) => attack,
                    None if pose == Pose::Cast => "Spell_Simple_Shoot",
                    None => "Sword_Regular_A",
                }
            }
            Pose::Hit if thrown(u) => "Hit_Knockback",
            Pose::Hit => "Hit_Chest",
            Pose::Dead => "Death01",
        }
    }
    /// What a humanoid on a wall plays (#247; docs/CONTRACT.md, "Poses to animation"), with the
    /// clip time and whether it loops: the clip for the sim's move onto, between or off holds, at
    /// the share of the move the body has come (#248), or else the hang, shimmying whichever way
    /// the body slides along the wall, by how far it has slid. `None` off the wall.
    fn climbing(&mut self, u: &UnitState, dt: f32) -> Option<(&'static str, f32, bool)> {
        use anim::climb::Wall;
        let (last, last_pose) = (self.last_traversal, self.last_pose);
        self.last_traversal = u.traversal;
        self.on_wall = None;
        let (s, c) = u.yaw.sin_cos();
        let right = Vec3::new(-c, 0., s);
        match u.traversal {
            Traversal::Hanging => {
                self.climb = None;
                self.on_wall = Some(OnWall {
                    wall: Wall::Hanging,
                    k: 1.,
                    at: [u.position; 2],
                });
                let side = u.velocity.dot(right);
                if side.abs() > 0.3 {
                    self.shimmied_right = side > 0.;
                }
                let clip = if side > 0.3 {
                    "Hang_Shimmy_R_Loop"
                } else if side < -0.3 {
                    "Hang_Shimmy_L_Loop"
                } else {
                    return Some(("Hang_Idle_Loop", u.pose_time, true));
                };
                // The shimmy loops are made sliding at the sim's shimmy speed with the hands
                // still on the hold, so played by the distance slid they keep still at any speed.
                if !self.traversing {
                    return Some((clip, u.pose_time, true));
                }
                let slid = side.abs() * dt / anim::climb::SHIMMY_STRIDE;
                self.shimmy = (self.shimmy + slid).fract();
                Some((clip, self.shimmy * self.clip_duration(clip), true))
            }
            Traversal::Forced(f) => {
                let new = self
                    .climb
                    .as_ref()
                    .is_none_or(|m| m.progress.start() != Some(f.start));
                if new {
                    // Which move this is, from how the body got here: a new move is decided
                    // once, on its first frame.
                    let from_hang = matches!(last, Traversal::Hanging)
                        || matches!(last, Traversal::Forced(g) if matches!(g.kind, ForcedKind::Hold | ForcedKind::Corner));
                    let d = f.to - f.from;
                    let (clip, wall) = match f.kind {
                        ForcedKind::Climb if from_hang => ("Hang_ClimbUp", Wall::ClimbUp),
                        // Up or down to the next hold, or across to one (a side eject's longer
                        // hop until it has a clip of its own).
                        ForcedKind::Hold if from_hang => {
                            let across = d.dot(right);
                            let clip = if d.y > 0.3 && d.y > across.abs() {
                                "Hang_Climb_Up"
                            } else if d.y < -0.3 && -d.y > across.abs() {
                                "Hang_Climb_Down"
                            } else if across > 0. {
                                "Hang_Hop_R"
                            } else {
                                "Hang_Hop_L"
                            };
                            (clip, Wall::Between)
                        }
                        ForcedKind::Corner if self.shimmied_right => ("Hang_Hop_R", Wall::Between),
                        ForcedKind::Corner => ("Hang_Hop_L", Wall::Between),
                        ForcedKind::Hold if matches!(last_pose, Pose::Jump | Pose::Fall) => {
                            ("Hang_Catch", Wall::Catch)
                        }
                        ForcedKind::Hold => ("Hang_Drop", Wall::Drop),
                        _ => {
                            self.climb = None;
                            return None;
                        }
                    };
                    self.climb = Some(ClimbMove {
                        clip,
                        wall,
                        progress: anim::Progress::default(),
                    });
                }
                let m = self.climb.as_mut()?;
                // Each clip is made on the sim's path for its move, `k` of the clip `k` of the
                // way along it, so it plays at the share the body has come; where the path
                // stands still, by the clock. A wrap round a corner swings round it instead
                // (`sim::climbing::corner_carry`), easing out as a hop across does, so it plays
                // by the clock throughout.
                let share = (self.traversing && f.kind != ForcedKind::Corner)
                    .then(|| anim::climb::progress(f.from, f.to, u.position))
                    .flatten();
                let k = m.progress.update(f.start, f.duration, share, dt);
                let (clip, wall) = (m.clip, m.wall);
                self.on_wall = Some(OnWall {
                    wall,
                    k,
                    at: [f.from, f.to],
                });
                Some((clip, k * self.clip_duration(clip), false))
            }
            _ => {
                self.climb = None;
                None
            }
        }
    }
    /// The pose the body shows for the sim's (#218). A blow it flinches from leaves it doing what
    /// it was: walking or running by the sim's own speeds, carried on through a move, or hanging
    /// on a wall (#247). A stagger or a knockback, a blow that caught it in the air, or one on a
    /// body that can't flinch still plays its hit in full.
    fn shown_pose(&self, u: &UnitState) -> Pose {
        if u.pose != Pose::Hit
            || !self.flinches()
            || self.hit_in_air
            || thrown(u)
            || self
                .combat
                .as_ref()
                .is_some_and(|c| c.action == Action::Stagger)
        {
            return u.pose;
        }
        // The sim's pose for what the body was doing (`update_poses`), but for the blow.
        match u.traversal {
            Traversal::Hanging => return Pose::Hang,
            Traversal::Forced(m) => {
                return match m.kind {
                    ForcedKind::Roll => Pose::Dodge,
                    ForcedKind::Charge => Pose::Run,
                    ForcedKind::Hold | ForcedKind::Corner => Pose::Hang,
                    ForcedKind::Knockback => Pose::Hit,
                    ForcedKind::Leap
                    | ForcedKind::Climb
                    | ForcedKind::Mantle
                    | ForcedKind::Vault
                    | ForcedKind::WallRun => Pose::Jump,
                };
            }
            _ => {}
        }
        let speed = u.velocity.xz().length();
        if speed > 3.2 {
            Pose::Run
        } else if speed > 0.3 {
            Pose::Walk
        } else {
            Pose::Idle
        }
    }
    /// Whether blows tip this body (#218): a humanoid, its spine, neck and head found.
    fn flinches(&self) -> bool {
        self.flinching
            && self.humanoid
            && self.spine_bones.is_some()
            && self.head_bones.iter().all(Option::is_some)
    }
    /// The move `u` is carried through, if this body plays it by how far it has come (#213): a
    /// roll, a leap, a charge, a climb, a mantle or a vault. A knockback plays its hit by the
    /// clock, and the climbing clips play the moves onto holds (#247).
    fn carried_by(&self, u: &UnitState) -> Option<ForcedMove> {
        match u.traversal {
            Traversal::Forced(m) if self.traversing && self.humanoid => {
                (!matches!(m.kind, ForcedKind::Knockback | ForcedKind::Hold)).then_some(m)
            }
            _ => None,
        }
    }
    /// Whether `u` pulls up over a ledge (#213), from the ground or caught in a jump (a mantle):
    /// too high to step up onto, it plays a climb, and the hands take hold of the edge. Lower ones
    /// hop up.
    fn pulling_up(&self, u: &UnitState) -> bool {
        self.carried_by(u).is_some_and(|m| {
            matches!(m.kind, ForcedKind::Climb | ForcedKind::Mantle)
                && m.to.y - m.from.y >= anim::traverse::STEP_UP
        })
    }
    /// Whether `u` is afloat (#213), where it plays the swim loops.
    fn swimming(&self, u: &UnitState) -> bool {
        self.traversing && self.humanoid && u.traversal == Traversal::Swimming
    }
    /// The locomotion pose the legs keep while a humanoid attacks or casts on the move, by the
    /// sim's own speed thresholds (`update_poses`). `None` plays the action on the whole body, as
    /// it does carried through a move or afloat (#213).
    fn moving_legs(&self, u: &UnitState) -> Option<Pose> {
        let kneeling = self
            .combat
            .as_ref()
            .is_some_and(|c| c.downed || c.action == Action::Revive);
        if !self.humanoid
            || kneeling
            || self.rally_time.is_some()
            || !matches!(self.shown_pose(u), Pose::Attack | Pose::Cast)
            || self.carried_by(u).is_some()
            || self.swimming(u)
        {
            return None;
        }
        let speed = u.velocity.xz().length();
        (speed > 0.3).then_some(if speed > 3.2 { Pose::Run } else { Pose::Walk })
    }
    /// How long clip `name` lasts, in seconds; 0 if there's no such clip.
    fn clip_duration(&self, name: &str) -> f32 {
        self.clips.clips.get(name).map_or(0., |a| a.duration)
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
    /// Turns the spine, neck and head toward where the character looks (#212), `free` of the way
    /// (0 fades the gaze out). `anim::Gaze` says how far each bone turns, round the vertical and
    /// about the line across the shoulders, from where the upper body faces after turning on the
    /// spot (#211). With a weapon drawn the spine aims, taking the arms and the weapon with it;
    /// otherwise mostly the head turns.
    fn follow_gaze(&mut self, u: &UnitState, body: f32, free: f32, dt: f32) {
        let (Some([_, low, mid, chest]), [Some(neck), Some(head)]) =
            (self.spine_bones, self.head_bones)
        else {
            return;
        };
        if free <= 0. && self.gaze.faded_out() {
            return;
        }
        let bones = [low, mid, chest, neck, head];
        // The point from the head, where it was last frame (near enough), in the model's frame.
        let facing = self.turn.offset();
        let from = self.world[head].w_axis.truncate();
        let want = self.gaze_at.and_then(|at| {
            let to = Quat::from_rotation_y(-body) * (at - u.position) - from;
            let flat = to.xz().length();
            (to.length() > NEAREST_GAZE).then(|| anim::Aim {
                // Right overhead or underfoot, it looks the way it faces.
                yaw: if flat > 0.3 {
                    wrap(to.x.atan2(to.z) - facing)
                } else {
                    0.
                },
                // Less however far the body leans forward (#217), so the aim holds.
                pitch: to.y.atan2(flat) + self.leaned.dot(Vec2::new(facing.sin(), facing.cos())),
                aiming: u.look.drawn,
            })
        });
        let turns = self.gaze.update(want, free, dt);
        if turns == anim::Turns::default() {
            return;
        }
        // Each bone turns round the vertical, then pitches about the line across the way it now
        // faces, so the head ends up facing round and up by all of it.
        let (mut above, mut yaw) = (Quat::IDENTITY, facing);
        for (i, b) in bones.into_iter().enumerate() {
            if i == 0 || self.nodes[b].parent != Some(bones[i - 1]) {
                above = self.nodes[b]
                    .parent
                    .map_or(Quat::IDENTITY, |p| self.rotation_to(p, None, false));
            }
            yaw += turns.yaw[i];
            let across = Quat::from_rotation_y(yaw) * Vec3::X;
            let turn = Quat::from_axis_angle(across, -turns.pitch[i])
                * Quat::from_rotation_y(turns.yaw[i]);
            let local = &mut self.local[b];
            local.rotation = (above.inverse() * turn * above * local.rotation).normalize();
            above *= local.rotation;
        }
    }
    /// Leans the body by `tilt` (#217), toward +x and +z in the model's frame, in radians: the
    /// pelvis tips about the ground beneath it, taking the whole body with it, and the spine bends
    /// the rest of the way, a third at each bone.
    fn lean_body(&mut self, tilt: Vec2) {
        let Some([pelvis, low, mid, chest]) = self.spine_bones else {
            return;
        };
        if tilt.length() < 1e-6 {
            return;
        }
        let spine = tilt * ((1. - LEAN_BODY) / 3.);
        self.tip(&[
            (pelvis, tilt * LEAN_BODY),
            (low, spine),
            (mid, spine),
            (chest, spine),
        ]);
    }
    /// Raises the pelvis, and the whole body on it, by `height` in the model's frame.
    fn raise(&mut self, height: f32) {
        let Some([pelvis, ..]) = self.spine_bones else {
            return;
        };
        let above = self.nodes[pelvis]
            .parent
            .map_or(Quat::IDENTITY, |p| self.rotation_to(p, None, false));
        self.local[pelvis].translation += above.inverse() * Vec3::Y * height;
    }
    /// Bends the body as a flinch has it (#218): the pelvis, the spine, the neck and the head.
    fn flinch_body(&mut self, bend: &anim::Bend) {
        let (Some([pelvis, low, mid, chest]), [Some(neck), Some(head)]) =
            (self.spine_bones, self.head_bones)
        else {
            return;
        };
        self.tip(&[
            (pelvis, bend.pelvis),
            (low, bend.spine[0]),
            (mid, bend.spine[1]),
            (chest, bend.spine[2]),
            (neck, bend.neck),
            (head, bend.head),
        ]);
    }
    /// Tips each of `bones`, a chain from the pelvis up, on top of its parent by its tilt: toward
    /// +x and +z in the model's frame, in radians. The pelvis tips about the ground beneath it,
    /// taking the whole body with it.
    fn tip(&mut self, bones: &[(usize, Vec2)]) {
        let mut above = Quat::IDENTITY;
        // The lean tips the spine's bones alike, and one turn does for them all.
        let mut last = (Vec2::ZERO, Quat::IDENTITY);
        for (i, &(b, tilt)) in bones.iter().enumerate() {
            if i == 0 || self.nodes[b].parent != Some(bones[i - 1].0) {
                above = self.nodes[b]
                    .parent
                    .map_or(Quat::IDENTITY, |p| self.rotation_to(p, None, false));
            }
            if tilt != last.0 {
                let angle = tilt.length();
                let turn = if angle > 1e-6 {
                    Quat::from_axis_angle(Vec3::new(tilt.y, 0., -tilt.x) / angle, angle)
                } else {
                    Quat::IDENTITY
                };
                last = (tilt, turn);
            }
            let turn = last.1;
            if turn != Quat::IDENTITY {
                if i == 0 {
                    // Round the point on the ground beneath the pelvis, so the feet stay about
                    // where they were.
                    let [at] = self.chain([b]);
                    let moved = turn * Vec3::Y * at.y - Vec3::Y * at.y;
                    self.local[b].translation += above.inverse() * moved;
                }
                let local = &mut self.local[b];
                local.rotation = (above.inverse() * turn * above * local.rotation).normalize();
            }
            above *= self.local[b].rotation;
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
        // The pelvis dips on landing too (#217), the legs bending to keep the feet down.
        let drop = Vec3::Y * (placed.drop + self.landing.dip());
        if drop.y != 0. {
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
        self.list_on(i);
        let by = by.extend(0.);
        for &n in &self.dropped.1 {
            self.world[n].w_axis += by;
        }
    }
    /// Lists bone `i` and everything on it, parents first, for [`Character::shift`]: once (again
    /// if a piece brings bones), since a flat list is quicker to go through than the tree, and the
    /// pelvis carries nearly the whole skeleton.
    fn list_on(&mut self, i: usize) {
        let (count, on) = &mut self.dropped;
        if *count == self.nodes.len() && on.first() == Some(&i) {
            return;
        }
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
    /// How far this frame must move, up and in toward the wall, for its hands to reach a hold
    /// (#248), from its wrists in its own hang: `Hang_Idle_Loop`'s first frame. Nothing without the
    /// clip or the arms.
    fn measure_reach(&mut self) -> Vec3 {
        let wrists = self.arm_bones.map(|arm| arm.map(|[_, _, hand, _]| hand));
        let [Some(left), Some(right)] = wrists else {
            return Vec3::ZERO;
        };
        if !self.clip_set.clips.contains_key("Hang_Idle_Loop") {
            return Vec3::ZERO;
        }
        self.world.resize(self.nodes.len(), Mat4::IDENTITY);
        for (n, l) in self.nodes.iter().zip(&mut self.local) {
            *l = n.rest;
        }
        self.apply_clip("Hang_Idle_Loop", 0., true, Bones::All);
        self.pose_world();
        anim::climb::reach([left, right].map(|i| self.world[i].w_axis.truncate()))
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
    /// A blow landed on this character (#218), travelling over the ground along yaw `way` (0
    /// toward +z, a quarter turn toward +x, as `Event::Hit` sends it), as hard as `weight`: the
    /// body tips away from it, if it can, and its pose holds for `hitstop` seconds.
    pub fn hit(&mut self, way: f32, weight: HitWeight, hitstop: f32) {
        if !self.flinching {
            return;
        }
        if !self.flinches() {
            self.flinch.hold(hitstop);
            return;
        }
        use anim::flinch::{BIG, BLOCKED, CRITICAL, HEAVY, LIGHT, PARRIED};
        let (size, heavy) = match weight {
            HitWeight::Light => (LIGHT, false),
            HitWeight::Heavy => (HEAVY, true),
            HitWeight::Critical => (CRITICAL, true),
            HitWeight::Big => (BIG, true),
            HitWeight::Blocked => (BLOCKED, false),
            HitWeight::Parried => (PARRIED, false),
        };
        // Into the frame of the body as it is drawn.
        let way = way - self.turn.yaw().unwrap_or(0.);
        let blow = anim::Blow {
            way: Vec2::new(way.sin(), way.cos()),
            size,
            heavy,
        };
        self.flinch.hit(blow, hitstop);
    }
    /// This character's own blow landed (#218): its pose holds for `hitstop` seconds.
    pub fn landed(&mut self, hitstop: f32) {
        if self.flinching {
            self.flinch.hold(hitstop);
        }
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
                // Out of a move, what was left of its clip gives way to the action (#213).
                if !matches!(u.traversal, Traversal::Forced(_)) {
                    self.carried = None;
                }
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
        // Blows (#218): the hitstop holds the pose clocks, so they move on by `pose_dt`, and the
        // flinch bends the body over everything else. The sim never pauses, so whatever follows
        // the body through the world (the stride over the ground covered, the turn, the lean, the
        // feet, the capes, the blender) keeps to `dt`.
        let (pose_dt, bend) = self.flinch.update(dt);
        if u.pose == Pose::Hit && self.last_pose != Pose::Hit {
            self.hit_in_air = self.air.is_some();
        }
        let pose = self.shown_pose(u);
        if let Some(t) = &mut self.rally_time {
            *t += pose_dt;
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
            pose,
            Pose::Attack | Pose::Cast | Pose::Dodge | Pose::Hit | Pose::Dead
        ) {
            self.transition = None;
        }
        if let Some((name, time)) = &mut self.transition {
            *time += pose_dt;
            if *time >= self.clips.clips.get(*name).map_or(0., |a| a.duration) {
                self.transition = None;
            }
        }
        // Weight (#217). A landing that hurt plays through standing still, and ends on moving.
        if let Some(t) = &mut self.hard_landing {
            *t += pose_dt;
            if *t >= self.clips.clips.get("Jump_Land").map_or(0., |a| a.duration)
                || pose != Pose::Idle
            {
                self.hard_landing = None;
            }
        }
        // On a wall (#247), the climbing clips play: the hang, or the sim's move onto, between or
        // off holds. A climb up from a hang isn't in the air.
        let climbing = if self.humanoid {
            self.climbing(u, dt)
        } else {
            None
        };
        // A move the body is carried through (#213), unless it's on a wall, where the climbing
        // clips play it.
        let forced = self.carried_by(u).filter(|_| climbing.is_none());
        let pulling_up = forced.is_some() && self.pulling_up(u);
        // In the air, one timeline runs from take-off through `Jump_Start` and on into
        // `Jump_Loop`: the push-off's crouch left out, and the legs tucked highest at the top of
        // the jump. Off an edge, it falls straight into the loop. A climb (#213) isn't in the air.
        let airborne = matches!(pose, Pose::Jump | Pose::Fall) && climbing.is_none() && !pulling_up;
        let took_off = airborne && pose == Pose::Jump && self.last_pose != Pose::Jump;
        self.air = match self.air.filter(|_| airborne && !took_off) {
            Some(t) => {
                let rate = if u.velocity.y > 0. && t < TUCK {
                    ((TUCK - t) * anim::land::GRAVITY / u.velocity.y).clamp(0.25, 2.)
                } else {
                    1.
                };
                Some(t + pose_dt * rate)
            }
            None if took_off => Some(TAKEOFF),
            None if airborne => Some(self.clip_duration("Jump_Start")),
            None => None,
        };
        // Touching down, the pelvis dips as far as the fall was fast, or after a fall that hurt,
        // a body standing still plays its landing in full. Catching a hold isn't touching down,
        // and water breaks a fall.
        if let Some(down) = self
            .landing
            .update(airborne, u.position.y, u.velocity.y, dt)
            && climbing.is_none()
        {
            if self.humanoid
                && down.fell > anim::land::SAFE_FALL
                && pose == Pose::Idle
                && !self.swimming(u)
                && self.clips.clips.contains_key("Jump_Land")
            {
                self.hard_landing = Some(0.);
            } else {
                self.landing.dip_for(down.speed);
            }
        }
        // A move's clip (#213). Out of the move, the rest of it plays on by the clock, until it
        // ends or something else plays; a new move starts afresh.
        let new_move = forced.is_some_and(|m| self.progress.start() != Some(m.start));
        self.carried = self.carried.and_then(|mut c| match forced {
            Some(m) => (m.start == c.start).then_some(c),
            None => {
                c.time = c.time.max(c.end) + pose_dt;
                let goes_on = match self.action_clip(u) {
                    // The pose of the action that began it holds the clip, to its end.
                    Some(clip) => clip == c.clip,
                    None => {
                        matches!(pose, Pose::Idle | Pose::Walk | Pose::Run)
                            && c.time < self.clip_duration(c.clip)
                    }
                };
                (goes_on && self.hard_landing.is_none() && !self.swimming(u)).then_some(c)
            }
        });
        let name = match climbing {
            Some((clip, _, _)) => clip,
            None => self.clip_for(u),
        };
        let speed = u.velocity.xz().length();
        let moving = self.moving_legs(u);
        let legs = moving.map(|pose| locomotion_clip(pose, speed));
        let acting = self.action_clip(u).is_some();
        // The rest of a move's clip, playing on after it.
        let after = self.carried.filter(|_| forced.is_none());
        let carrying_on = after.is_some_and(|c| c.clip == name);
        // Afloat, the swim loops play at the surface (#213).
        let swimming = matches!(name, "Swim_Idle_Loop" | "Swim_Fwd_Loop");
        let busy = acting
            || carrying_on
            || matches!(
                pose,
                Pose::Attack | Pose::Cast | Pose::Dodge | Pose::Hit | Pose::Dead
            );
        // Any change in what plays is a switch for the blender, even one that looks continuous:
        // the sim's pose changing under a flinch restarts its clock, which the blender smooths. A
        // move's clip playing on after it doesn't restart, but the next move's does (#213).
        let mut switch = self.forced_switch.take();
        if name != self.last_clip
            || legs != self.last_legs
            || (u.pose != self.last_pose && !carrying_on)
            || new_move
        {
            switch = Some(switch_kind(self.last_pose, self.last_busy, pose, busy));
            self.last_clip = name;
            self.last_legs = legs;
        }
        self.last_pose = u.pose;
        self.last_busy = busy;
        // Clips timed by the sim's clocks run behind them by however long hitstops held them,
        // until a switch lets them catch up.
        if switch.is_some() {
            self.flinch.switched();
        }
        let behind = self.flinch.behind();
        let pose_time = (u.pose_time - behind).max(0.);
        let action_time = |c: &Fight| (c.action_time - behind).max(0.);
        for (n, l) in self.nodes.iter().zip(&mut self.local) {
            *l = n.rest;
        }
        let locomotion = matches!(pose, Pose::Walk | Pose::Run);
        let walking = locomotion && !acting;
        let looping = !carrying_on
            && match pose {
                Pose::Idle => name != "Jump_Land",
                Pose::Walk | Pose::Run => true,
                Pose::Fall => name != "Jump_Start",
                _ => name == "Jump_Loop" || LOOPS.contains(&name),
            }
            || self.combat.as_ref().is_some_and(|c| {
                matches!(c.action, Action::Channel | Action::Revive)
                    || c.action == Action::Spin && c.cast.is_some_and(|(_, channel)| channel)
            });
        let rate = match pose {
            Pose::Walk | Pose::Run if swimming => {
                (speed / anim::traverse::SWIM_SPEED).clamp(0.5, 1.5)
            }
            Pose::Walk | Pose::Run => self.stride_rate(pose, speed),
            Pose::Dodge => self.clips.clips.get(name).map_or(1., |a| a.duration / 0.45),
            Pose::Attack => self.clips.clips.get(name).map_or(1., |a| a.duration / 0.5),
            _ => 1.,
        };
        // The body turns on the spot behind the sim's yaw (#211): standing, the upper body takes
        // up the turn until the feet step round; moving or acting, the body follows on a spring.
        // Below full detail (#215) the feet don't step: the body follows on the spring. Carried
        // through a roll or a climb (#213) it faces the way it goes, and after a roll until it has
        // stood up, then turns back.
        let full = self.tier == anim::Tier::Full;
        let stance = if full && self.humanoid && pose == Pose::Idle && !busy && !swimming {
            anim::Stance::Standing
        } else {
            anim::Stance::Moving
        };
        let way = match forced {
            Some(m)
                if matches!(
                    m.kind,
                    ForcedKind::Roll | ForcedKind::Climb | ForcedKind::Mantle | ForcedKind::Vault
                ) =>
            {
                anim::traverse::heading(m.from, m.to)
            }
            Some(_) => None,
            None => after
                .filter(|_| carrying_on && pose == Pose::Idle)
                .and_then(|c| c.heading),
        };
        let body = match way {
            Some(way) => self.turn.update(way, anim::Stance::Carried, dt),
            None => self.turn.update(u.yaw, stance, dt),
        };
        // The humanoid loops follow the body (#110): blended by speed, the hips turned toward
        // where it travels. One stride phase runs through attacks on the move and every change of
        // speed or direction, so the feet never restart. Afloat, the strokes play by speed.
        let striding = (walking || moving.is_some()) && !swimming;
        let step = match self.gaits.as_ref().filter(|_| striding) {
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
        } else if let (Some(t), "Jump_Start" | "Jump_Loop") = (self.air, name) {
            if name == "Jump_Loop" {
                t - self.clip_duration("Jump_Start")
            } else {
                t
            }
        } else if let (Some(t), "Jump_Land") = (self.hard_landing, name) {
            t
        } else if pose == Pose::Idle && !acting {
            // Crowds out of step (#217): each unit's idle starts at its own phase and pace.
            anim::idle::time(u.id, pose_time, self.clip_duration(name))
        } else {
            pose_time * rate
        };
        if let Some(c) = &self.combat {
            if c.downed {
                clip_time = self.clips.clips.get(name).map_or(0., |a| a.duration);
            } else if let Some(t) = self.rally_time {
                clip_time = t;
            } else if let Some(windup) = self.windup.filter(|_| pose == Pose::Attack) {
                // The blow lands on the clip's `hit` marker just as the wind-up ends.
                let duration = self.clips.clips.get(name).map_or(1., |a| a.duration);
                let hit = self
                    .clip_meta
                    .get(name)
                    .and_then(|m| m.event("hit"))
                    .unwrap_or(duration * 0.5);
                clip_time = swing_time(action_time(c), windup, hit, duration);
            } else if acting {
                clip_time = if c.action == Action::Parry {
                    action_time(c) * 2.
                } else if pose == Pose::Attack {
                    action_time(c) * rate
                } else {
                    action_time(c)
                };
            }
        }
        // Traversal (#213): through a move, a clip with a `travel` curve plays the moment that
        // covers as much of the move's path as the body has, so it reaches the end of that stretch
        // as the move ends; after the move, the rest plays on by the clock.
        if let Some(m) = forced {
            let path = match m.kind {
                ForcedKind::Climb | ForcedKind::Mantle => anim::Path::UpAndOver,
                _ => anim::Path::Along,
            };
            let share = anim::traverse::share(path, m.from, m.to, u.position);
            let share = self.progress.update(m.start, m.duration, share, dt);
            let travel = self
                .clip_meta
                .get(name)
                .and_then(|c| c.curves.get("travel"));
            self.carried = travel.map(|keys| {
                clip_time = anim::traverse::clip_time(keys, share);
                Carried {
                    start: m.start,
                    clip: name,
                    time: clip_time,
                    end: anim::traverse::clip_time(keys, 1.),
                    heading: anim::traverse::heading(m.from, m.to)
                        .filter(|_| m.kind == ForcedKind::Roll),
                }
            });
        } else if let Some(c) = after.filter(|_| carrying_on) {
            clip_time = c.time;
        }
        // On a wall (#247), the climbing clip's own time.
        let (clip_time, looping) =
            climbing.map_or((clip_time, looping), |(_, time, looping)| (time, looping));
        match step.filter(|_| walking) {
            Some(step) => self.apply_step(step, Bones::All),
            None => self.apply_clip(name, clip_time, looping, Bones::All),
        }
        if swimming {
            // The swim clips have the surface at their origin; the sim floats the body lower.
            self.raise(anim::traverse::SWIM_FLOAT);
        }
        // Falling, the legs reach for the ground by when the body will touch down (#217): into
        // `Jump_Land`'s first frame, its feet just meeting the ground.
        if airborne
            && self.humanoid
            && !acting
            && let Some(ground) = &self.ground
        {
            let height = u.position.y - ground(u.position.x, u.position.z);
            let reach = self.landing.reach(height, u.velocity.y, dt);
            if reach > 0. {
                self.mix_clip("Jump_Land", 0., false, Bones::All, reach);
            }
        }
        let aim = legs.and_then(|legs| self.walk_under(legs, step));
        let mut upper = None;
        if locomotion
            && u.look.drawn
            && self.humanoid
            && !swimming
            && let Some(idle) = self.main_clips.get("idle")
        {
            let t = anim::idle::time(u.id, pose_time, self.clip_duration(idle));
            self.apply_clip(idle, t, true, Bones::Upper);
            upper = Some(idle);
        }
        // Out of a roll or a dash on the move (#213), the rest of its clip on the upper body.
        if let Some(c) = after.filter(|_| locomotion && !acting && !swimming) {
            self.apply_clip(c.clip, c.time, false, Bones::Upper);
            upper = Some(c.clip);
        }
        if self.humanoid && self.combat.as_ref().is_some_and(|c| c.guarding) {
            let c = self.combat.as_ref().unwrap();
            let parrying = c.action == Action::Parry && action_time(c) < 0.25;
            let time = if parrying {
                action_time(c) * 2.
            } else {
                action_time(c)
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
        // The body leans with its acceleration (#217): forward setting off, back stopping, and
        // into turns. Not in the air, on a wall, carried through a move, afloat or down, nor below
        // half detail.
        let down = self.combat.as_ref().is_some_and(|c| c.downed);
        let upright = airborne
            || climbing.is_some()
            || forced.is_some()
            || swimming
            || matches!(pose, Pose::Dodge | Pose::Dead)
            || down
            || self.rally_time.is_some()
            || self.tier == anim::Tier::Quarter;
        let free = if upright || !self.leaning { 0. } else { 1. };
        self.leaned = self.lean.update(u.velocity.xz(), body, free, dt);
        self.lean_body(self.leaned);
        // The spine, neck and head follow where it looks (#212). A swing's clip keeps the head,
        // but a shot or a bolt is aimed, and below half detail nobody looks about.
        let shooting = matches!(pose, Pose::Attack | Pose::Cast)
            && self.combat.as_ref().is_some_and(|c| {
                !c.downed && matches!(c.action, Action::Shoot | Action::CastBolt | Action::Channel)
            });
        let held =
            busy && !shooting || self.rally_time.is_some() || self.tier == anim::Tier::Quarter;
        self.follow_gaze(u, body, if held { 0. } else { 1. }, dt);
        self.pose_world();
        self.drawn_now = u.look.drawn;
        self.main_offset = 0.;
        let (active, t) = transition.unwrap_or((name, clip_time));
        self.perform(active, t);
        // Last, so a switch also smooths the hand reaching and gripping that `perform` adds.
        self.blender.update(&mut self.local, switch, dt);
        // Then the flinch, over everything, so the blender never takes it for a switch to smooth.
        // Below half detail, it doesn't show.
        let flinched = bend != anim::Bend::default() && self.tier != anim::Tier::Quarter;
        if flinched {
            self.flinch_body(&bend);
        }
        if self.blender.blending() || flinched {
            self.pose_world();
        }
        // Climbing (#213), the hands grab the edge while they reach it: after the blender, which
        // would only bring them there once the sim had lifted the body out of reach.
        if let Some(m) = forced.filter(|_| pulling_up) {
            self.grip_edge(m, u.position, body, dt);
        }
        // On a wall (#248), the body hangs from the hold at its own arms' reach, the hands close
        // on the hold and the feet brace on the wall below.
        if self.humanoid {
            self.hang_from_hold(u.position, body, dt);
        }
        // Then the feet go on the ground (#214), so that nothing smooths them off it: at full
        // detail, standing, walking or acting on it, the feet planted in the stride held still.
        // Not on a wall (#247), even taking a blow there, carried through a move, nor afloat (#213).
        let kneeling = self
            .combat
            .as_ref()
            .is_some_and(|c| c.downed || c.action == Action::Revive);
        let grounded = full
            && self.humanoid
            && !kneeling
            && self.rally_time.is_none()
            && climbing.is_none()
            && forced.is_none()
            && !swimming
            && matches!(
                pose,
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
    /// Hangs the character on walls from the holds `at` finds (#248). Without them, the hands and
    /// feet on a wall go where the clips put them.
    pub fn set_holds(&mut self, at: HoldAt) {
        self.holds = Some(at);
    }
    /// Has the character look at `at`, a point in the world, from its next update (#212): where
    /// the crosshair points, its target, or someone stopping by. `None` looks ahead.
    pub fn look_at(&mut self, at: Option<Vec3>) {
        self.gaze_at = at;
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
        // The shoulder turns the elbow to its place and the elbow the wrist to the goal, `weight`
        // of the way, with the arm's world matrices rebuilt once, at the end: everything on the
        // forearm turns with both, the rest of the arm with the shoulder alone.
        let turn = |from: Vec3, to: Vec3| {
            if from.length_squared() < 1e-10 || to.length_squared() < 1e-10 {
                return Quat::IDENTITY;
            }
            Quat::IDENTITY.slerp(
                Quat::from_rotation_arc(from.normalize(), to.normalize()),
                weight,
            )
        };
        let rotation = |i: usize| self.world[i].to_scale_rotation_translation().1;
        let parent = |i: usize| self.nodes[i].parent.map_or(Quat::IDENTITY, rotation);
        let upper = turn(elbow - shoulder, new_elbow - shoulder);
        let (elbow_now, wrist_now) = (
            shoulder + upper * (elbow - shoulder),
            shoulder + upper * (wrist - shoulder),
        );
        let lower = turn(wrist_now - elbow_now, goal - elbow_now) * upper;
        let (above_a, turned_a) = (parent(a), upper * rotation(a));
        let (above_b, turned_b) = (upper * parent(b), lower * rotation(b));
        let (above_h, now) = (lower * parent(h), lower * rotation(h));
        let target_q = want.to_scale_rotation_translation().1;
        self.local[a].rotation = (above_a.inverse() * turned_a).normalize();
        self.local[b].rotation = (above_b.inverse() * turned_b).normalize();
        self.local[h].rotation = (above_h.inverse() * now.slerp(target_q, weight)).normalize();
        self.update_chain(a);
    }
    /// Puts the hands on the edge of the climb `m` (#213), the body at `position` turned `body`,
    /// `dt` seconds on: palms down on the top a shoulder's width apart, fingers over the edge, as
    /// firmly as `anim::traverse::grip` has them hold it, while they can reach it.
    fn grip_edge(&mut self, m: ForcedMove, position: Vec3, body: f32, dt: f32) {
        let edge = match self.edge {
            Some((start, edge)) if start == m.start => edge,
            _ => {
                let edge = match &self.ground {
                    Some(ground) => anim::traverse::edge(m.from, m.to, |x, z| ground(x, z)),
                    None => anim::traverse::edge(m.from, m.to, |_, _| m.from.y),
                };
                self.edge = Some((m.start, edge));
                self.grip = [0.; 2];
                edge
            }
        };
        let elapsed = self.progress.elapsed();
        let weight = anim::traverse::grip(edge, m.from, m.to, position, elapsed);
        if weight <= 0. && self.grip == [0.; 2] {
            return;
        }
        // Into the model's frame: the edge, the way over it, and across it toward the left hand.
        let to_model = Quat::from_rotation_y(-body);
        let way = to_model * (m.to - m.from).with_y(0.).normalize_or(Vec3::Z);
        let across = Vec3::new(way.z, 0., -way.x);
        let edge = to_model * (edge - position);
        const KNUCKLES: [[&str; 2]; 2] =
            [["index_01_l", "pinky_01_l"], ["index_01_r", "pinky_01_r"]];
        for (side, toward) in [(Side::Left, 1.), (Side::Right, -1.)] {
            let Some([shoulder, elbow, hand, socket]) = self.arm_bones[side as usize] else {
                continue;
            };
            let at = |i: usize| self.world[i].w_axis.truncate();
            let socket_now = self.world[socket].to_scale_rotation_translation().1;
            // The turn that puts the palm down, then the fingers over the edge, from the knuckles
            // and the wrist. Without the knuckles, the hand keeps the clip's turn.
            let turn = match KNUCKLES[side as usize].map(|n| self.names.get(n).copied()) {
                [Some(index), Some(pinky)] => {
                    let (wrist, index, pinky) = (at(hand), at(index), at(pinky));
                    // Out of the palm: the knuckles run the other way round on the left hand.
                    let palm = (index - wrist).cross(pinky - wrist) * -toward;
                    let down = Quat::from_rotation_arc(palm.normalize_or(Vec3::NEG_Y), Vec3::NEG_Y);
                    let fingers = down * ((index + pinky) * 0.5 - wrist);
                    let over = way.x.atan2(way.z) - fingers.x.atan2(fingers.z);
                    Quat::from_rotation_y(over) * down
                }
                _ => Quat::IDENTITY,
            };
            let target = Mat4::from_rotation_translation(
                turn * socket_now,
                edge + across * (toward * HAND_SPREAD) + way * 0.05,
            );
            // Held while the wrist can reach its place on the edge, eased off once it can't.
            let wrist = (target * self.local[socket].matrix().inverse())
                .w_axis
                .truncate();
            let arm = at(shoulder).distance(at(elbow)) + at(elbow).distance(at(hand));
            let reach = anim::traverse::within_reach(at(shoulder).distance(wrist), arm);
            let held = &mut self.grip[side as usize];
            *held = anim::traverse::ease_hold(*held, weight * reach, dt);
            let held = *held;
            self.arm_to(side, target, held);
        }
    }
    /// Hangs a body on a wall from its hold (#248), `position` turned `body`, `dt` seconds on:
    /// moved up or down and in or out so its hands meet the hold where the clips put them (eased
    /// in and out), by its own reach and by how far short of a full hang the hold is (the sim
    /// stands a body on the ground under a low one); then, at full and half detail, each hand
    /// closes on the hold it's near and the feet brace on the wall below, with two-bone IK.
    fn hang_from_hold(&mut self, position: Vec3, body: f32, dt: f32) {
        use anim::climb::{self, Wall};
        // Without traversal, the hands and feet go where the clips put them.
        let on_wall = self.on_wall.filter(|_| self.traversing);
        // Off the wall, once the body has come back down from its raise, nothing moves.
        if on_wall.is_none() && self.raised == Vec3::ZERO {
            self.hold_grip = [0.; 2];
            return;
        }
        // The holds the hands may be on, in the model's frame: the one the body hangs from, or
        // the ones its move goes from and to.
        let to_model = Quat::from_rotation_y(-body);
        let edge = |h: world::Hold| climb::Edge {
            a: to_model * (h.a - position),
            b: to_model * (h.b - position),
            out: to_model * Vec3::new(h.out.x, 0., h.out.y),
            braced: h.braced,
        };
        let edges = match (on_wall, &self.holds) {
            (Some(OnWall { at: [from, to], .. }), Some(holds)) => [
                holds(from).map(edge),
                if to == from {
                    None
                } else {
                    holds(to).map(edge)
                },
            ],
            _ => [None; 2],
        };
        let want = on_wall.map_or(Vec3::ZERO, |w| {
            let [from, to] = w.at;
            // How far the body moves hanging at each end of the move, from the hold there: up, and
            // in toward the wall (ahead, where there's no hold to say).
            let raise = |p: Vec3, e: Option<climb::Edge>| {
                e.map_or(self.hang_reach, |e| {
                    let p = to_model * (p - position);
                    let by = climb::raise(self.hang_reach, e.nearest(p).y - p.y, e.out_from(p));
                    Vec3::Y * by.y - e.out * by.z
                })
            };
            let (start, end) = (
                raise(from, edges[0]),
                raise(to, if to == from { edges[0] } else { edges[1] }),
            );
            let raised = match w.wall {
                Wall::Hanging | Wall::ClimbUp => start,
                Wall::Drop | Wall::Catch => end,
                Wall::Between => start + (end - start) * w.k,
            };
            raised * climb::hanging(w.wall, w.k)
        });
        self.raised = climb::ease_raise(self.raised, want, dt);
        if self.raised != Vec3::ZERO
            && let Some([pelvis, ..]) = self.spine_bones
        {
            let by = self.raised;
            let above = self.nodes[pelvis]
                .parent
                .map_or(Quat::IDENTITY, |p| self.rotation_to(p, None, false));
            self.local[pelvis].translation += above.inverse() * by;
            self.shift(pelvis, by);
        }
        if on_wall.is_none() || self.tier == anim::Tier::Quarter || edges == [None; 2] {
            self.hold_grip = [0.; 2];
            return;
        }
        let at = |c: &Self, i: usize| c.world[i].w_axis.truncate();
        for side in [Side::Left, Side::Right] {
            let Some([shoulder, elbow, hand, socket]) = self.arm_bones[side as usize] else {
                continue;
            };
            let wrist = at(self, hand);
            let (goal, firm) = edges
                .iter()
                .flatten()
                .map(|e| climb::grip(e, wrist))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((wrist, 0.));
            let arm =
                at(self, shoulder).distance(at(self, elbow)) + at(self, elbow).distance(wrist);
            let reach = anim::traverse::within_reach(at(self, shoulder).distance(goal), arm);
            let held = &mut self.hold_grip[side as usize];
            *held = anim::traverse::ease_hold(*held, firm * reach, dt);
            let held = *held;
            // The hand goes where the wrist does, turned as the clip turns it.
            let target = Mat4::from_translation(goal - wrist) * self.world[socket];
            self.arm_to(side, target, held);
        }
        // The feet brace on the wall the body hangs from, or the nearer of the walls its move goes
        // between (round a corner, they're square to each other).
        let (Some(legs), Some(balls)) = (self.leg_bones, self.ball_bones) else {
            return;
        };
        for (leg, ball) in legs.into_iter().zip(balls) {
            let ball = at(self, ball);
            let (by, firm) = edges
                .iter()
                .flatten()
                .map(|e| climb::brace(e, ball))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap_or((Vec3::ZERO, 0.));
            if firm <= 0. {
                continue;
            }
            let joints = leg.map(|i| at(self, i));
            self.reach(
                leg,
                joints,
                joints[2] + by * firm,
                Quat::IDENTITY,
                Quat::IDENTITY,
            );
            self.update_chain(leg[0]);
        }
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
    /// A look with `gear`, a weapon drawn.
    fn geared(gear: &[(&str, &str)]) -> Look {
        Look {
            art: "male".into(),
            drawn: true,
            gear: gear.iter().map(|&(s, i)| (s.into(), i.into())).collect(),
            ..Default::default()
        }
    }
    #[test]
    fn gear_changes_carry_the_motion_on_instead_of_popping() {
        // Jogging with sword, shield and cape, a greatsword takes their place at frame 30 (the
        // same cape), another cape on bones of the same names goes on at 60, it comes off at 90
        // and the first goes back on at 120. A twin keeps the first look throughout, and a third
        // is built again at each change, as the client used to.
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let mut cache = ModelCache::default();
        let (sword, shield) = (("main_hand", "sword.arming"), ("off_hand", "shield.round"));
        let great = ("main_hand", "sword.great");
        let looks = [
            geared(&[sword, shield, ("back", "cape.traveller")]),
            geared(&[great, ("back", "cape.traveller")]),
            geared(&[great, ("back", "cape.ranger")]),
            geared(&[great]),
            geared(&[sword, shield, ("back", "cape.traveller")]),
        ];
        // Each frame's local rotations by bone name, and the stride's phase.
        type Shown = Vec<(HashMap<String, Quat>, f32)>;
        let mut play = |how: Option<bool>| -> Shown {
            let mut c = Character::load(&cat, &mut cache, &looks[0]).unwrap();
            // The blender's and the springs' own smoothness: the feet go on the ground after them
            // (#214), and carry on through a change untouched.
            c.foot_placement = false;
            let mut u = traveller(4.5, 0.).1;
            let mut shown = Vec::new();
            for frame in 0..150 {
                if frame % 30 == 0 && frame > 0 {
                    let look = &looks[frame / 30];
                    match how {
                        Some(true) => c.set_look(&cat, &mut cache, look).unwrap(),
                        Some(false) => {
                            c = Character::load(&cat, &mut cache, look).unwrap();
                            c.foot_placement = false;
                        }
                        None => {}
                    }
                }
                c.update(&u, 1. / 60.);
                if how == Some(true) && frame % 30 == 0 && frame > 0 {
                    assert!(
                        c.blender.blending(),
                        "frame {frame}: the change is a switch"
                    );
                }
                assert!(c.world.iter().all(|m| m.is_finite()));
                // Every part's bones are in the skeleton.
                assert!(!c.draws(u.position, c.body_yaw()).is_empty());
                u.position += u.velocity / 60.;
                let pose = c
                    .names
                    .iter()
                    .map(|(n, &i)| (n.clone(), c.local[i].rotation));
                shown.push((pose.collect(), c.stride.phase));
            }
            shown
        };
        let [swapped, twin, rebuilt] = [Some(true), None, Some(false)].map(&mut play);
        // How much more or less each bone in both poses turned on frame `at` than on the frame
        // before, at worst, over the bones `which` picks.
        let jolt = |shown: &Shown, at: usize, which: &dyn Fn(&str) -> bool| {
            let turn = |a: &Quat, b: &Quat| {
                let d = *b * a.inverse();
                2. * d.xyz().length().atan2(d.w.abs())
            };
            let (before, last, now) = (&shown[at - 2].0, &shown[at - 1].0, &shown[at].0);
            let common = before
                .keys()
                .filter(|n| last.contains_key(*n) && now.contains_key(*n));
            common
                .filter(|n| which(n))
                .map(|n| (turn(&last[n], &now[n]) - turn(&before[n], &last[n])).abs())
                .fold(0., f32::max)
        };
        let body = |n: &str| !n.starts_with("cape_");
        for at in [30, 60, 90, 120] {
            assert!(
                jolt(&swapped, at, &body) < 1e-4,
                "frame {at}: the body jolts {}",
                jolt(&swapped, at, &body)
            );
            let stride = |s: &Shown| (s[at].1 - s[at - 1].1).rem_euclid(1.);
            assert!(
                (stride(&swapped) - stride(&twin)).abs() < 1e-4,
                "frame {at}: the stride runs on"
            );
        }
        assert!(
            [30, 60, 90, 120]
                .iter()
                .any(|&at| jolt(&rebuilt, at, &body) > 0.5),
            "building it again pops"
        );
        // How far apart two capes hang on frame `at`, at worst.
        let apart = |a: &Shown, b: &Shown, at: usize| {
            a[at]
                .0
                .iter()
                .filter(|(n, _)| n.starts_with("cape_"))
                .map(|(n, q)| q.angle_between(b[at].0[n]))
                .fold(0., f32::max)
        };
        // Kept through the change, the cape swings on as the twin's does, where built again it
        // starts over from the pose.
        let kept = apart(&swapped, &twin, 30);
        assert!(kept < 0.02, "the cape strays {kept}");
        let built = apart(&rebuilt, &twin, 30);
        assert!(built > 0.1, "a new cape starts over {built}");
        // Another cape on the same bones takes up the swing rather than starting over.
        let other = apart(&swapped, &rebuilt, 60);
        assert!(other > 0.1, "another cape starts over {other}");
    }
    #[test]
    fn a_new_look_survives_detail_swaps_and_a_new_body_starts_afresh() {
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let mut cache = ModelCache::default();
        let caped = geared(&[("main_hand", "sword.arming"), ("back", "cape.traveller")]);
        let bare = geared(&[("main_hand", "sword.arming")]);
        let mut c = Character::load(&cat, &mut cache, &caped).unwrap();
        // Both levels of detail of the caped look get kept for swapping back.
        c.set_lod(&cat, &mut cache, 1).unwrap();
        c.set_lod(&cat, &mut cache, 0).unwrap();
        c.set_look(&cat, &mut cache, &bare).unwrap();
        c.set_lod(&cat, &mut cache, 1).unwrap();
        let fresh = Character::load_lod(&cat, &mut cache, &bare, 1).unwrap();
        assert_eq!(c.parts.len(), fresh.parts.len(), "no cape comes back");
        assert!(c.springs.is_empty());
        c.foot_placement = false;
        c.set_ground(Arc::new(|_, _| 0.));
        c.set_holds(Arc::new(|_| None));
        let reach = c.hang_reach;
        let female = Look {
            art: "female".into(),
            ..bare.clone()
        };
        c.set_look(&cat, &mut cache, &female).unwrap();
        assert_eq!(c.look.art, "female");
        assert!(!c.foot_placement, "settings carry over to the new body");
        assert!(
            c.ground.is_some() && c.holds.is_some(),
            "and so do the ground and holds"
        );
        assert_ne!(
            c.hang_reach, reach,
            "the new body reaches for holds as it does"
        );
        let fresh = Character::load_lod(&cat, &mut cache, &female, 1).unwrap();
        assert_eq!(c.parts.len(), fresh.parts.len());
        assert_eq!(c.nodes.len(), fresh.nodes.len());
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
    /// A ranger with a hunting bow, drawn or on the back, standing facing +z. Switches cut.
    fn archer(drawn: bool) -> (Character, UnitState) {
        let (_, mut u, _) = fighter(0.);
        u.look = Look {
            art: "male".into(),
            drawn,
            gear: vec![
                ("main_hand".into(), "bow.hunting".into()),
                ("back".into(), "quiver.leather".into()),
            ],
            ..Default::default()
        }
        .into();
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let mut c = Character::load(&cat, &mut ModelCache::default(), &u.look).unwrap();
        c.blender.set_method(anim::Method::Off);
        (c, u)
    }
    /// How bone `i` of `c` is turned from the same bone of `twin`, posed the same way without
    /// the gaze (#212): the yaw (toward +x) and pitch (up) that turn takes the facing (+z) to, in
    /// degrees.
    fn gazed(c: &Character, twin: &Character, i: usize) -> (f32, f32) {
        let turn = Quat::from_mat4(&c.world[i]) * Quat::from_mat4(&twin.world[i]).inverse();
        let ahead = turn * Vec3::Z;
        (
            ahead.x.atan2(ahead.z).to_degrees(),
            ahead.y.asin().to_degrees(),
        )
    }
    #[test]
    fn a_drawn_bow_pitches_the_spine_with_the_aim_and_a_look_mostly_the_head() {
        // Looking at a point 30° up or down, 10 m ahead: the head ends up pitched all the way.
        // With the bow drawn the spine takes all of it (anim::look::AIM_PITCH), so the arms and
        // the bow point there too; with it on the back the chest takes a little (LOOK_PITCH).
        for drawn in [true, false] {
            for angle in [30f32, -30.] {
                let (mut c, mut u) = archer(drawn);
                let (mut twin, _) = archer(drawn);
                let [head, chest, hand] = ["Head", "spine_03", "hand_l"].map(|n| c.names[n]);
                c.update(&u, 1. / 60.);
                let eyes = c.world[head].w_axis.truncate();
                c.look_at(Some(
                    eyes + Vec3::new(0., 10. * angle.to_radians().tan(), 10.),
                ));
                for frame in 0..60 {
                    u.pose_time = frame as f32 / 60.;
                    c.update(&u, 1. / 60.);
                    twin.update(&u, 1. / 60.);
                }
                let share: f32 = if drawn {
                    1.
                } else {
                    anim::look::LOOK_PITCH[..3].iter().sum()
                };
                let (yaw, pitch) = gazed(&c, &twin, head);
                assert!(
                    (pitch - angle).abs() < 1.5 && yaw.abs() < 1.,
                    "drawn {drawn}, {angle}°: the head turned {yaw}° and pitched {pitch}°"
                );
                let (_, pitch) = gazed(&c, &twin, chest);
                assert!(
                    (pitch - angle * share).abs() < 1.5,
                    "drawn {drawn}, {angle}°: the chest pitched {pitch}°"
                );
                if drawn {
                    let (_, pitch) = gazed(&c, &twin, hand);
                    assert!(
                        (pitch - angle).abs() < 3.,
                        "{angle}°: the bow hand pitched {pitch}°"
                    );
                }
            }
        }
    }
    #[test]
    fn heads_turn_toward_a_point_within_limits_and_let_go_behind() {
        // A point at head height, round to the side: the head turns to it up to 70°, the chest
        // taking a fifth; behind, it looks ahead again.
        for (round, want) in [(60f32, 60f32), (-60., -60.), (100., 70.), (150., 0.)] {
            let (mut c, mut u) = archer(false);
            let (mut twin, _) = archer(false);
            let [head, chest] = ["Head", "spine_03"].map(|n| c.names[n]);
            c.update(&u, 1. / 60.);
            let eyes = c.world[head].w_axis.truncate();
            let way = round.to_radians();
            c.look_at(Some(eyes + Vec3::new(way.sin(), 0., way.cos()) * 5.));
            for frame in 0..90 {
                u.pose_time = frame as f32 / 60.;
                c.update(&u, 1. / 60.);
                twin.update(&u, 1. / 60.);
            }
            let (yaw, pitch) = gazed(&c, &twin, head);
            assert!(
                (yaw - want).abs() < 2. && pitch.abs() < 2.,
                "{round}°: the head turned {yaw}° and pitched {pitch}°"
            );
            let (yaw, _) = gazed(&c, &twin, chest);
            assert!(
                (yaw - want * 0.2).abs() < 1.,
                "{round}°: the chest turned {yaw}°"
            );
        }
    }
    #[test]
    fn a_swing_takes_the_head_back_and_a_shot_keeps_its_aim() {
        // Looking 45° round and 20° up, a swordsman slashes: the slash's clip has the head, so
        // the gaze fades out over the swing without jumping. An archer loosing a shot keeps aiming.
        let way = 45f32.to_radians();
        let point =
            |eyes: Vec3| eyes + Vec3::new(way.sin(), 20f32.to_radians().tan(), way.cos()) * 5.;
        let (mut c, mut u, mut fight) = fighter(0.);
        let (mut twin, ..) = fighter(0.);
        let head = c.names["Head"];
        c.update_combat(&u, Some(&fight), 1. / 60.);
        c.look_at(Some(point(c.world[head].w_axis.truncate())));
        for frame in 0..60 {
            u.pose_time = frame as f32 / 60.;
            c.update_combat(&u, Some(&fight), 1. / 60.);
            twin.update_combat(&u, Some(&fight), 1. / 60.);
        }
        let start = gazed(&c, &twin, head);
        assert!(
            (start.0 - 45.).abs() < 2. && (start.1 - 20.).abs() < 2.,
            "{start:?}"
        );
        u.pose = Pose::Attack;
        fight.action = Action::Slash;
        let mut last = start;
        for frame in 0..30 {
            let t = frame as f32 / 60.;
            u.pose_time = t;
            fight.action_time = t;
            c.update_combat(&u, Some(&fight), 1. / 60.);
            twin.update_combat(&u, Some(&fight), 1. / 60.);
            let now = gazed(&c, &twin, head);
            let step = (now.0 - last.0).hypot(now.1 - last.1);
            assert!(step < 5., "frame {frame}: the head jumped {step}°");
            last = now;
        }
        assert!(
            last.0.hypot(last.1) < 0.05 * start.0.hypot(start.1),
            "after the swing the gaze still turns the head {last:?}"
        );
        // The archer looses a shot: still aiming where they were.
        let (mut c, mut u) = archer(true);
        let (mut twin, _) = archer(true);
        let head = c.names["Head"];
        c.update_combat(&u, Some(&fight), 1. / 60.);
        c.look_at(Some(point(c.world[head].w_axis.truncate())));
        let mut fight = fight.clone();
        fight.action = Action::None;
        for frame in 0..60 {
            u.pose_time = frame as f32 / 60.;
            c.update_combat(&u, Some(&fight), 1. / 60.);
            twin.update_combat(&u, Some(&fight), 1. / 60.);
        }
        u.pose = Pose::Attack;
        fight.action = Action::Shoot;
        for frame in 0..30 {
            let t = frame as f32 / 60.;
            u.pose_time = t;
            fight.action_time = t;
            c.update_combat(&u, Some(&fight), 1. / 60.);
            twin.update_combat(&u, Some(&fight), 1. / 60.);
        }
        let (yaw, pitch) = gazed(&c, &twin, head);
        assert!(
            (yaw - 45.).abs() < 3. && (pitch - 20.).abs() < 3.,
            "loosing, the archer's head turned {yaw}° and pitched {pitch}°"
        );
    }
    /// The sim's jump on the flat (#217): up at 6.6 m/s under its gravity, `Jump` rising and
    /// `Fall` after, between a second standing before and a second standing after. Each frame's
    /// unit, and the frame it touches down on.
    fn jumping(u: &UnitState) -> (Vec<UnitState>, usize) {
        let mut u = u.clone();
        let mut frames = vec![];
        let mut since = 0;
        let mut pose = |u: &mut UnitState, now: Pose, frames: &Vec<UnitState>| {
            if now != u.pose {
                (u.pose, since) = (now, frames.len());
            }
            u.pose_time = (frames.len() - since) as f32 / 60.;
        };
        for _ in 0..60 {
            pose(&mut u, Pose::Idle, &frames);
            frames.push(u.clone());
        }
        for f in 1.. {
            let t = f as f32 / 60.;
            let y = 6.6 * t - 0.5 * anim::land::GRAVITY * t * t;
            if y <= 0. {
                break;
            }
            u.position.y = y;
            u.velocity.y = 6.6 - anim::land::GRAVITY * t;
            let now = if u.velocity.y > 0. {
                Pose::Jump
            } else {
                Pose::Fall
            };
            pose(&mut u, now, &frames);
            frames.push(u.clone());
        }
        let down = frames.len();
        (u.position.y, u.velocity.y) = (0., 0.);
        for _ in 0..60 {
            pose(&mut u, Pose::Idle, &frames);
            frames.push(u.clone());
        }
        (frames, down)
    }
    #[test]
    fn a_jump_takes_off_straight_tucks_at_the_top_and_lands_with_a_dip() {
        // On flat ground with the feet on it: the legs are straight leaving the ground (not in
        // Jump_Start's crouch), tuck highest at the top of the arc and reach the ground as it
        // comes; touching down at 6.6 m/s, the pelvis dips about 6.6 cm with the feet kept down.
        let (mut c, u, _) = fighter(0.);
        c.set_ground(Arc::new(|_, _| 0.));
        let [pelvis, ball_l, ball_r] = ["pelvis", "ball_l", "ball_r"].map(|n| c.names[n]);
        let (frames, down) = jumping(&u);
        let mut seen = vec![];
        for u in &frames {
            c.update(u, 1. / 60.);
            let at = |i: usize| c.world[i].w_axis.truncate() + u.position;
            seen.push([at(pelvis), at(ball_l), at(ball_r)].map(|p| p.y - u.position.y));
        }
        let first = seen[60][0];
        assert!(first > 0.8, "the pelvis took off {first} m up, crouched");
        let feet = |f: usize| seen[f][1].max(seen[f][2]);
        let tuck = (60..down)
            .max_by(|a, b| feet(*a).total_cmp(&feet(*b)))
            .unwrap();
        let top = 60 + 20;
        assert!(
            tuck.abs_diff(top) <= 3,
            "the feet tucked highest {tuck}, the top at {top}"
        );
        assert!(
            feet(down - 1) < 0.05,
            "the feet were {} up touching down",
            feet(down - 1)
        );
        // The pelvis in the world: down and back up without bouncing, the feet on the ground.
        let height = |f: usize| seen[f][0] + frames[f].position.y;
        let standing = height(frames.len() - 1);
        let low = (down..down + 30).map(height).fold(f32::MAX, f32::min);
        let dip = standing - low;
        assert!((0.045..0.085).contains(&dip), "dipped {dip} m");
        for (f, at) in seen.iter().enumerate().skip(down) {
            for foot in [1, 2] {
                assert!(at[foot] > -0.01, "frame {f}: a foot sank {}", at[foot]);
            }
        }
    }
    #[test]
    fn a_fall_that_hurts_lands_in_full_standing_and_dips_on_the_run() {
        // Off a 6 m ledge onto flat ground: standing, Jump_Land plays through (the pelvis down to
        // a squat); running, the stride goes on, the pelvis dipping the most it does below a twin
        // that ran on without falling.
        for speed in [0f32, 6.] {
            let (mut c, mut u, _) = fighter(speed);
            let (mut twin, mut v, _) = fighter(speed);
            for c in [&mut c, &mut twin] {
                c.set_ground(Arc::new(|_, _| 0.));
            }
            let pelvis = c.names["pelvis"];
            u.position.y = 6.;
            c.update(&u, 1. / 60.);
            twin.update(&v, 1. / 60.);
            u.pose = Pose::Fall;
            let mut frame = 0;
            while u.position.y > 0. {
                u.velocity.y -= anim::land::GRAVITY / 60.;
                u.position += u.velocity / 60.;
                u.pose_time = frame as f32 / 60.;
                c.update(&u, 1. / 60.);
                frame += 1;
            }
            (u.position, u.velocity.y) = (v.position, 0.);
            u.pose = v.pose;
            let mut below = vec![];
            for f in 0..60 {
                for (c, u) in [(&mut c, &mut u), (&mut twin, &mut v)] {
                    u.pose_time = f as f32 / 60.;
                    c.update(u, 1. / 60.);
                    u.position += u.velocity / 60.;
                }
                below.push(twin.world[pelvis].w_axis.y - c.world[pelvis].w_axis.y);
                if f == 5 {
                    let landing = c.last_clip == "Jump_Land";
                    assert_eq!(landing, speed == 0., "{speed} m/s: playing {}", c.last_clip);
                }
            }
            let most = below[..30].iter().copied().fold(0., f32::max);
            if speed == 0. {
                assert!(most > 0.25, "standing, the pelvis only went {most} m down");
            } else {
                let want = anim::land::MAX_DIP;
                assert!((most - want).abs() < 0.03, "running, it dipped {most} m");
                assert!(below[45..].iter().all(|d| d.abs() < 0.02), "{below:?}");
            }
        }
    }
    #[test]
    fn runners_lean_forward_setting_off_back_stopping_and_into_turns() {
        // Against a twin that doesn't lean: the head goes forward setting off and back stopping
        // (about 1.2 m for each radian of lean, within the caps), and to the inside of a turn.
        let (mut c, mut u, _) = fighter(0.);
        let (mut twin, ..) = fighter(0.);
        twin.leaning = false;
        let head = c.names["Head"];
        let mut ahead = vec![];
        for frame in 0..120 {
            let running = (30..75).contains(&frame);
            u.velocity.z = if running { 6. } else { 0. };
            u.pose = if running { Pose::Run } else { Pose::Idle };
            c.update(&u, 1. / 60.);
            twin.update(&u, 1. / 60.);
            u.position += u.velocity / 60.;
            ahead.push(c.world[head].w_axis.z - twin.world[head].w_axis.z);
        }
        let most = |r: std::ops::Range<usize>| {
            ahead[r]
                .iter()
                .copied()
                .fold(0f32, |a, b| if b.abs() > a.abs() { b } else { a })
        };
        let (off, stop) = (most(30..60), most(75..110));
        assert!(most(0..30) == 0., "it leaned standing");
        assert!(
            off > 0.08 && off < 1.3 * anim::lean::MAX_PITCH,
            "set off {off} m forward"
        );
        assert!(stop < -0.05, "stopped {stop} m back");
        // Round to the right (toward +x) at 6 m/s, 1.2 radians a second.
        let (mut c, mut u, _) = fighter(6.);
        let (mut twin, ..) = fighter(6.);
        twin.leaning = false;
        let mut inside = vec![];
        for frame in 0..120 {
            u.yaw = 1.2 * frame as f32 / 60.;
            u.velocity = Vec3::new(u.yaw.sin(), 0., u.yaw.cos()) * 6.;
            c.update(&u, 1. / 60.);
            twin.update(&u, 1. / 60.);
            u.position += u.velocity / 60.;
            inside.push(c.world[head].w_axis.x - twin.world[head].w_axis.x);
        }
        let banked = inside[60..].iter().sum::<f32>() / 60.;
        assert!(
            banked > 0.12 && banked < 1.3 * anim::lean::MAX_BANK,
            "leaned {banked} m into the turn"
        );
    }
    /// Which way a blow travelling along yaw `way` goes over the ground (x, z).
    fn along(way: f32) -> Vec2 {
        Vec2::new(way.sin(), way.cos())
    }
    #[test]
    fn a_blow_on_a_wall_or_mid_roll_flinches_over_what_the_body_was_doing() {
        // Struck hanging on a wall (#247) or rolling, the sim's `Hit` pose leaves the climbing
        // clip or the roll playing, not a walk or run by the body's speed, and the head still
        // goes the way the blow went.
        use std::f32::consts::PI;
        let (mut c, mut u, _) = fighter(0.);
        let (mut twin, mut still, _) = fighter(0.);
        for v in [&mut u, &mut still] {
            (v.position.y, v.pose, v.traversal) = (2., Pose::Hang, Traversal::Hanging);
        }
        let head = c.names["Head"];
        let mut most = 0f32;
        for frame in 0..40 {
            if frame == 10 {
                c.hit(PI, HitWeight::Light, 0.);
            }
            u.pose = if (10..25).contains(&frame) {
                Pose::Hit
            } else {
                Pose::Hang
            };
            c.update(&u, 1. / 60.);
            twin.update(&still, 1. / 60.);
            assert_eq!(c.last_clip, "Hang_Idle_Loop", "frame {frame}");
            let off = (c.world[head].w_axis - twin.world[head].w_axis).truncate();
            most = most.max(off.length());
        }
        assert!(most > 0.02, "on the wall, the head only went {most} m");
        let (mut c, mut u, _) = fighter(0.);
        let roll = protocol::ForcedMove {
            kind: ForcedKind::Roll,
            from: Vec3::ZERO,
            to: Vec3::Z * 5.,
            start: 1.,
            duration: 0.45,
        };
        u.traversal = Traversal::Forced(roll);
        for frame in 0..27 {
            if frame == 10 {
                c.hit(PI, HitWeight::Light, 0.);
            }
            let k = frame as f32 / 27.;
            (u.position, u.velocity) = (Vec3::Z * 5. * (1. - (1. - k) * (1. - k)), Vec3::Z * 11.);
            (u.pose, u.pose_time) = (
                if (10..25).contains(&frame) {
                    Pose::Hit
                } else {
                    Pose::Dodge
                },
                frame as f32 / 60.,
            );
            c.update(&u, 1. / 60.);
            assert_eq!(c.last_clip, "Roll", "frame {frame}");
        }
    }
    #[test]
    fn blows_tip_the_body_away_while_the_legs_jog_on() {
        // A jogger takes a light blow from behind, in front and either side in turn, beside a twin
        // who takes none. The sim's `Hit` pose lasts a quarter of a second, and used to stop the
        // legs for `Hit_Chest`: now the legs and the stride jog on as the twin's do, while the
        // head goes the way the blow went and comes back.
        use std::f32::consts::{FRAC_PI_2, PI};
        for way in [0., PI, FRAC_PI_2, -FRAC_PI_2] {
            let (mut c, mut u, _) = fighter(4.5);
            let (mut twin, mut jog, _) = fighter(4.5);
            let head = c.names["Head"];
            let mut offsets = vec![];
            for frame in 0..60 {
                if frame == 20 {
                    c.hit(way, HitWeight::Light, 0.);
                }
                u.pose = if (20..35).contains(&frame) {
                    Pose::Hit
                } else {
                    Pose::Run
                };
                c.update(&u, 1. / 60.);
                twin.update(&jog, 1. / 60.);
                for leg in ["thigh_l", "calf_l", "foot_r", "thigh_r"] {
                    let [a, b] = [&c, &twin].map(|c| c.local[c.names[leg]].rotation);
                    assert!(same(a, b), "frame {frame}: {leg} stopped jogging");
                }
                assert!((c.stride.phase - twin.stride.phase).abs() < 1e-6);
                offsets.push(
                    (c.world[head].w_axis - twin.world[head].w_axis)
                        .truncate()
                        .xz(),
                );
                for v in [&mut u, &mut jog] {
                    v.pose_time += 1. / 60.;
                    v.position += v.velocity / 60.;
                }
            }
            assert!(offsets[..20].iter().all(|o| *o == Vec2::ZERO));
            let peak = offsets.iter().copied().fold(Vec2::ZERO, |a, b| {
                if b.length() > a.length() { b } else { a }
            });
            assert!(
                peak.normalize().dot(along(way)) > 0.95,
                "from {way}: the head went {peak}"
            );
            assert!(
                peak.length() > 0.02 && peak.length() < 0.12,
                "from {way}: the head went {} m",
                peak.length()
            );
            assert!(
                offsets[50..].iter().all(|o| o.length() < 0.005),
                "from {way}: the head didn't come back"
            );
        }
        // A heavy blow from in front takes the hips back too, the body tipping about its feet.
        let (mut c, mut u, _) = fighter(0.);
        let (mut twin, mut still, _) = fighter(0.);
        let pelvis = c.spine_bones.unwrap()[0];
        let mut back = 0f32;
        for frame in 0..40 {
            if frame == 10 {
                c.hit(PI, HitWeight::Heavy, 0.);
            }
            u.pose = if (10..25).contains(&frame) {
                Pose::Hit
            } else {
                Pose::Idle
            };
            c.update(&u, 1. / 60.);
            twin.update(&still, 1. / 60.);
            back = back.min(c.world[pelvis].w_axis.z - twin.world[pelvis].w_axis.z);
            u.pose_time += 1. / 60.;
            still.pose_time += 1. / 60.;
        }
        assert!(back < -0.03, "the hips went {back} m back");
        // A hitstop as a blow lands on the jogger holds the upper body, but the legs keep up with
        // the ground it covers.
        let (mut c, mut u, _) = fighter(4.5);
        let (mut twin, mut jog, _) = fighter(4.5);
        for frame in 0..40 {
            if frame == 10 {
                c.hit(0., HitWeight::Big, 0.12);
            }
            u.pose = if (10..25).contains(&frame) {
                Pose::Hit
            } else {
                Pose::Run
            };
            c.update(&u, 1. / 60.);
            twin.update(&jog, 1. / 60.);
            assert!((c.stride.phase - twin.stride.phase).abs() < 1e-6);
            for v in [&mut u, &mut jog] {
                v.pose_time += 1. / 60.;
                v.position += v.velocity / 60.;
            }
        }
    }
    #[test]
    fn staggers_knockbacks_and_blows_in_the_air_still_play_their_clips() {
        let (mut c, mut u, mut fight) = fighter(0.);
        let mut standing = u.clone();
        standing.pose = Pose::Idle;
        u.pose = Pose::Hit;
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(
            c.last_clip,
            c.clip_for(&standing),
            "a blow it flinches from"
        );
        // Flinching off, to compare: the blow plays the hit clip in full and holds nothing.
        let (mut off, ..) = fighter(0.);
        off.flinching = false;
        off.hit(0., HitWeight::Heavy, 0.08);
        off.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(off.last_clip, "Hit_Chest");
        assert!(!off.flinch.holding());
        fight.action = Action::Stagger;
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Hit_Knockback", "a stagger");
        fight.action = Action::None;
        u.traversal = Traversal::Forced(protocol::ForcedMove {
            kind: ForcedKind::Knockback,
            from: Vec3::ZERO,
            to: Vec3::Z * 3.,
            start: 0.,
            duration: 0.4,
        });
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Hit_Knockback", "a knockback");
        // Caught in the air, it plays its hit in full as before.
        let (mut c, mut u, fight) = fighter(0.);
        u.pose = Pose::Jump;
        u.velocity.y = 6.;
        for _ in 0..5 {
            c.update_combat(&u, Some(&fight), 1. / 60.);
        }
        u.pose = Pose::Hit;
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Hit_Chest");
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Hit_Chest");
        // A beast can't flinch: it plays its own hit, and a blow on it only holds its pose.
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let wolf = Look {
            art: "wolf.grey".into(),
            ..Default::default()
        };
        let mut c = Character::load(&cat, &mut ModelCache::default(), &wolf).unwrap();
        u.look = wolf.into();
        u.velocity.y = 0.;
        u.pose = Pose::Hit;
        c.hit(0., HitWeight::Heavy, 0.05);
        c.update(&u, 1. / 60.);
        assert_eq!(Some(c.last_clip), c.role_clips.get("hit").copied());
        assert!(c.flinch.holding() && c.flinch.update(0.).1 == anim::Bend::default());
    }
    #[test]
    fn the_hitstop_holds_a_swing_then_lets_it_go_on_without_a_jump() {
        // A standing slash lands on frame 10 and holds for 80 ms; a twin's lands with none. The
        // sword arm holds still through the hitstop, swings on from where it held at its own
        // pace, behind the twin's, and when the swing ends both stand alike again.
        let (mut c, mut u, mut fight) = fighter(0.);
        let (mut twin, ..) = fighter(0.);
        u.pose = Pose::Attack;
        fight.action = Action::Slash;
        let arm = c.names["upperarm_r"];
        let (mut shown, mut twins) = (vec![], vec![]);
        for frame in 0..40 {
            if frame == 10 {
                c.landed(0.08);
            }
            if frame == 30 {
                u.pose = Pose::Idle;
                u.pose_time = 0.;
                fight.action = Action::None;
                fight.action_time = 0.;
            }
            c.update_combat(&u, Some(&fight), 1. / 60.);
            twin.update_combat(&u, Some(&fight), 1. / 60.);
            shown.push(c.local[arm].rotation);
            twins.push(twin.local[arm].rotation);
            fight.action_time += 1. / 60.;
            u.pose_time += 1. / 60.;
        }
        let turned = |a: Quat, b: Quat| a.angle_between(b);
        assert!(turned(twins[9], twins[14]) > 0.05, "the swing hardly moved");
        // Held where it was as the blow landed, until 80 ms had gone by...
        for f in 10..14 {
            assert!(same(shown[f], shown[9]), "frame {f} moved");
        }
        // ...then on, never faster than the swing itself goes.
        let fastest = (9..29)
            .map(|f| turned(twins[f], twins[f + 1]))
            .fold(0., f32::max);
        for f in 13..29 {
            let step = turned(shown[f], shown[f + 1]);
            assert!(step <= fastest + 1e-4, "frame {f}: jumped {step}");
        }
        assert!(!same(shown[20], twins[20]), "it caught up during the swing");
        for f in 30..40 {
            assert!(same(shown[f], twins[f]), "frame {f}: still behind");
        }
    }
    #[test]
    fn on_a_wall_the_body_neither_lands_nor_leans_nor_jumps() {
        // Falling onto a hold 2 m up (#247), shimmying along the wall and climbing up from it:
        // catching the hold dips nothing, the shimmy leans nothing, and the climb up isn't a jump
        // (no timeline in the air).
        let (mut c, mut u, _) = fighter(0.);
        c.set_ground(Arc::new(|_, _| 0.));
        let dt = 1. / 60.;
        (u.position.y, u.pose) = (3., Pose::Fall);
        for frame in 0..20 {
            u.velocity.y = -anim::land::GRAVITY * frame as f32 * dt;
            u.position.y += u.velocity.y * dt;
            c.update(&u, dt);
        }
        let caught = u.position;
        let hold = protocol::ForcedMove {
            kind: ForcedKind::Hold,
            from: caught,
            to: caught + Vec3::new(0., 0.2, 0.2),
            start: 1.,
            duration: 0.3,
        };
        (u.pose, u.traversal, u.velocity) = (Pose::Hang, Traversal::Forced(hold), Vec3::ZERO);
        for frame in 0..18 {
            u.position = caught.lerp(hold.to, frame as f32 / 18.);
            u.pose_time = frame as f32 * dt;
            c.update(&u, dt);
            assert_eq!(c.last_clip, "Hang_Catch");
            assert_eq!(
                c.landing.dip(),
                0.,
                "frame {frame}: catching the hold dipped"
            );
        }
        u.traversal = Traversal::Hanging;
        for frame in 0..60 {
            // Shimmying right (toward -x, facing +z), then stopping.
            u.velocity = Vec3::X * if frame < 30 { -1. } else { 0. };
            u.position += u.velocity * dt;
            c.update(&u, dt);
            assert_eq!(c.leaned, Vec2::ZERO, "frame {frame}: leaned on the wall");
        }
        assert_eq!(c.last_clip, "Hang_Idle_Loop");
        let up = protocol::ForcedMove {
            kind: ForcedKind::Climb,
            from: u.position,
            to: u.position + Vec3::new(0., 1.5, 0.6),
            start: 3.,
            duration: 0.6,
        };
        (u.pose, u.traversal, u.velocity) = (Pose::Jump, Traversal::Forced(up), Vec3::ZERO);
        for frame in 0..36 {
            u.pose_time = frame as f32 * dt;
            c.update(&u, dt);
            assert_eq!(c.last_clip, "Hang_ClimbUp");
            assert!(c.air.is_none(), "frame {frame}: the climb up took off");
        }
    }
    #[test]
    fn neighbours_spawned_together_idle_out_of_step() {
        // Two knights with ids in a row, idling from the same moment, stand differently; the same
        // knight twice stands the same.
        let (mut a, u, _) = fighter(0.);
        let (mut b, ..) = fighter(0.);
        let (mut again, ..) = fighter(0.);
        let next = UnitState {
            id: u.id + 1,
            ..u.clone()
        };
        a.update(&u, 1. / 60.);
        b.update(&next, 1. / 60.);
        again.update(&u, 1. / 60.);
        let apart = |x: &Character, y: &Character| {
            (0..x.local.len())
                .map(|i| x.local[i].rotation.angle_between(y.local[i].rotation))
                .fold(0., f32::max)
        };
        assert!(apart(&a, &b) > 1f32.to_radians(), "{}", apart(&a, &b));
        assert!((0..a.local.len()).all(|i| a.local[i].rotation == again.local[i].rotation));
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

    /// The sim carrying `u` through a move of `kind` to `to` over `duration` seconds, hopping
    /// `arc` on the way (docs/MOVEMENT.md), in `pose`: the unit on each frame from the first after
    /// the move began (at sim time 10) to the last before it ends. On the next, the sim has let go.
    fn carried(
        u: &UnitState,
        kind: ForcedKind,
        to: Vec3,
        duration: f32,
        arc: f32,
        pose: Pose,
    ) -> Vec<UnitState> {
        let from = u.position;
        let m = ForcedMove {
            kind,
            from,
            to,
            start: 10.,
            duration,
        };
        let frames = (duration * 60.).round() as usize;
        let mut was = from;
        (1..frames)
            .map(|f| {
                let k = f as f32 / frames as f32;
                let at = if matches!(kind, ForcedKind::Climb | ForcedKind::Mantle) {
                    let up = 1. - (1. - (k * 1.6).min(1.)).powi(2);
                    let p = from.lerp(to, k * k);
                    Vec3::new(p.x, from.y + (to.y - from.y) * up, p.z)
                } else {
                    let eased = 1. - (1. - k) * (1. - k);
                    from.lerp(to, eased) + Vec3::Y * (k * std::f32::consts::PI).sin() * arc
                };
                let mut v = u.clone();
                (v.position, v.velocity) = (at, (at - was) * 60.);
                was = at;
                (v.pose, v.pose_time) = (pose, f as f32 / 60.);
                v.traversal = Traversal::Forced(m);
                v
            })
            .collect()
    }
    /// `u` standing or moving on as it was, `frames` frames on from `pose_time` 0.
    fn going_on(
        c: &mut Character,
        u: &mut UnitState,
        frames: usize,
        mut each: impl FnMut(&Character, usize),
    ) {
        u.traversal = Traversal::Ground;
        u.pose_time = 0.;
        for f in 0..frames {
            c.update(u, 1. / 60.);
            each(c, f);
            u.pose_time += 1. / 60.;
            u.position += u.velocity / 60.;
        }
    }
    #[test]
    fn a_strafing_roll_turns_into_the_roll_plays_by_distance_and_turns_back_once_up() {
        // Facing ahead and rolling 5 m to the side in 0.45 s, as the sim dodges while strafing:
        // the body turns to face the roll within a few frames, and the roll's clip keeps pace with the
        // ground covered, its hands down halfway and its feet down just as the move ends (by the
        // clock it would be standing up by then). Standing up it still faces the way it rolled,
        // then turns back to the sim's yaw.
        use std::f32::consts::FRAC_PI_2;
        let (mut c, mut u, _) = fighter(0.);
        going_on(&mut c, &mut u, 30, |_, _| {});
        let to = u.position + Vec3::X * 5.;
        let frames = carried(&u, ForcedKind::Roll, to, 0.45, 0.15, Pose::Dodge);
        let mut times = vec![];
        for (f, v) in frames.iter().enumerate() {
            c.update(v, 1. / 60.);
            assert_eq!(c.last_clip, "Roll");
            times.push(c.carried.unwrap().time);
            if f >= 8 {
                let off = wrap(c.body_yaw() - FRAC_PI_2);
                assert!(
                    off.abs() < 0.05,
                    "frame {f}: {}° off the roll",
                    off.to_degrees()
                );
            }
        }
        assert!(times.windows(2).all(|w| w[1] >= w[0]), "{times:?}");
        let halfway = frames.iter().position(|v| v.position.x >= 2.5).unwrap();
        assert!(
            (times[halfway] - 0.43).abs() < 0.04,
            "halfway at {}",
            times[halfway]
        );
        let last = times[times.len() - 1];
        assert!((last - 0.87).abs() < 0.04, "the move ended at {last}");
        // Standing up, the rest of the roll by the clock, still facing it; then the idle, and the
        // body turns back.
        u.position = to;
        let up = c.clip_duration("Roll") - 0.87;
        let mut rest = vec![];
        going_on(&mut c, &mut u, 200, |c, f| {
            rest.push((c.last_clip, c.body_yaw(), f))
        });
        let stood = rest.iter().position(|r| r.0 != "Roll").unwrap();
        assert!(
            (stood as f32 / 60. - up).abs() < 2. / 60.,
            "stood up after {stood} frames"
        );
        for (clip, yaw, f) in &rest[..stood] {
            assert_eq!(*clip, "Roll");
            assert!(
                wrap(yaw - FRAC_PI_2).abs() < 0.05,
                "frame {f} turned back early"
            );
        }
        assert_eq!(rest[stood].0, "Sword_Idle");
        let (_, yaw, _) = rest[rest.len() - 1];
        assert!(
            wrap(yaw).abs() < 0.01,
            "turned back to {}°",
            yaw.to_degrees()
        );
    }
    #[test]
    fn running_out_of_a_roll_the_legs_run_and_the_roll_recovers_over_them() {
        let (mut c, mut u, _) = fighter(6.);
        going_on(&mut c, &mut u, 30, |_, _| {});
        let to = u.position + Vec3::Z * 5.;
        for v in carried(&u, ForcedKind::Roll, to, 0.45, 0.15, Pose::Dodge) {
            c.update(&v, 1. / 60.);
        }
        u.position = to;
        let mut seen = vec![];
        going_on(&mut c, &mut u, 60, |c, _| {
            seen.push((c.last_clip, c.last_upper))
        });
        assert_eq!(seen[0], ("Jog_Fwd_Loop", Some("Roll")));
        let recovered = seen.iter().position(|s| s.1 != Some("Roll")).unwrap();
        assert!(
            recovered > 30,
            "the recovery stopped after {recovered} frames"
        );
        assert_eq!(seen[recovered], ("Jog_Fwd_Loop", Some("Sword_Idle")));
    }
    #[test]
    fn a_charge_plays_the_dash_by_distance_and_its_blow_on_arrival() {
        // The sword's lunge at a target 8 m off, as the sim plays it: a 0.4 s rush with a 0.3 m
        // hop, its thrust begun as it sets off and the Attack pose held 0.5 s, the blow landing
        // on arrival. The sword's dash plays over the whole body, its lunge ending as the body
        // arrives; the rest of it plays on by the clock while the pose holds, until the next blow.
        let (mut c, u, mut fight) = fighter(0.);
        fight.action = Action::Thrust;
        let frames = carried(&u, ForcedKind::Charge, Vec3::Z * 8., 0.4, 0.3, Pose::Attack);
        for v in &frames {
            fight.action_time = v.pose_time;
            c.update_combat(v, Some(&fight), 1. / 60.);
            assert_eq!((c.last_clip, c.last_legs), ("Sword_Dash", None));
        }
        let last = c.carried.unwrap().time;
        assert!(
            (last - 0.25).abs() < 0.03,
            "the lunge was at {last} arriving"
        );
        let mut at = frames[frames.len() - 1].clone();
        (at.position, at.velocity, at.traversal) = (Vec3::Z * 8., Vec3::ZERO, Traversal::Ground);
        let held = frames.len() + 1..30;
        for f in held.clone() {
            (at.pose_time, fight.action_time) = (f as f32 / 60., f as f32 / 60.);
            c.update_combat(&at, Some(&fight), 1. / 60.);
            assert_eq!(c.last_clip, "Sword_Dash", "frame {f}");
        }
        let on = c.carried.unwrap().time - last;
        assert!(
            on >= held.len() as f32 / 60.,
            "the dash went on {on} s after"
        );
        fight.action_time = 0.;
        c.update_combat(&at, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Sword_Regular_C");
        // Running on after the greatsword's rush, whose action is the charge, the sim's Run pose
        // and its stale action are just a run.
        let (mut c, u, mut fight) = fighter(6.);
        fight.action = Action::Charge;
        fight.action_time = 0.8;
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Jog_Fwd_Loop");
        // Without traversal, as before: the dash by the clock over sprinting legs, and again on
        // the run after.
        c.traversing = false;
        fight.action_time = 0.1;
        c.update_combat(&frames[5], Some(&fight), 1. / 60.);
        assert_eq!(
            (c.last_clip, c.last_legs),
            ("Sword_Dash", Some("Sprint_Loop"))
        );
        fight.action_time = 0.8;
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Sword_Dash");
    }
    /// Each hand's distance from where it holds the edge `edge` of a climb along `way`, as
    /// `grip_edge` places them.
    fn off_the_edge(c: &Character, position: Vec3, edge: Vec3, way: Vec3) -> [f32; 2] {
        let to_model = Quat::from_rotation_y(-c.body_yaw());
        let way = to_model * way;
        let across = Vec3::new(way.z, 0., -way.x);
        let edge = to_model * (edge - position);
        [(Side::Left, 1.), (Side::Right, -1.)].map(|(side, toward)| {
            let socket = c.arm_bones[side as usize].unwrap()[3];
            let want = edge + across * (toward * HAND_SPREAD) + way * 0.05;
            c.world[socket].w_axis.truncate().distance(want)
        })
    }
    #[test]
    fn climbs_and_mantles_play_the_climb_with_the_hands_on_the_edge() {
        // Up a 1.2 m wall whose face is 0.1 m ahead, onto its top 0.85 m on (where the sim stands
        // a climber): the climb plays over the whole body by distance, nothing of a jump, and the
        // hands grab the edge while they reach it, then let go as the sim lifts the body on.
        let wall = |_: f32, z: f32| if z >= 0.1 { 1.2 } else { 0. };
        let (mut c, mut u, _) = fighter(0.);
        c.set_ground(Arc::new(wall));
        going_on(&mut c, &mut u, 30, |_, _| {});
        let to = Vec3::new(0., 1.2, 0.85);
        let frames = carried(&u, ForcedKind::Climb, to, 0.6, 0., Pose::Jump);
        let mut closest = [f32::MAX; 2];
        let mut times = vec![];
        for v in &frames {
            c.update(v, 1. / 60.);
            assert_eq!(c.last_clip, "ClimbUp_1m");
            assert_eq!(c.air, None, "a climb isn't a jump");
            times.push(c.carried.unwrap().time);
            let edge = c.edge.unwrap().1;
            assert!(
                (edge - Vec3::new(0., 1.2, 0.1)).length() < 0.051,
                "edge at {edge}"
            );
            let off = off_the_edge(&c, v.position, edge, Vec3::Z);
            for side in 0..2 {
                closest[side] = closest[side].min(off[side]);
            }
        }
        // The nearer hand onto its place, the other as near as its arm reaches.
        let [near, far] = [closest[0].min(closest[1]), closest[0].max(closest[1])];
        assert!(
            near < 0.01 && far < 0.05,
            "the hands came {closest:?} m from the edge"
        );
        assert_eq!(c.grip, [0.; 2], "the hands let go coming over");
        assert!(times.windows(2).all(|w| w[1] >= w[0]), "{times:?}");
        let last = times[times.len() - 1];
        assert!((last - 0.667).abs() < 0.05, "the climb ended at {last}");
        // A step up 0.3 m high hops up: the jump, the hands off the top.
        let (mut c, u, _) = fighter(0.);
        c.set_ground(Arc::new(|_: f32, z: f32| if z >= 0.1 { 0.3 } else { 0. }));
        for v in carried(
            &u,
            ForcedKind::Climb,
            Vec3::new(0., 0.3, 0.85),
            0.6,
            0.,
            Pose::Jump,
        ) {
            c.update(&v, 1. / 60.);
            assert!(c.last_clip.starts_with("Jump_"), "{}", c.last_clip);
            assert_eq!(c.grip, [0.; 2]);
        }
        // Caught in a jump a metre up, a ledge 2.2 m high is pulled up onto the same way.
        let (mut c, mut u, _) = fighter(0.);
        c.set_ground(Arc::new(|_: f32, z: f32| if z >= 0.1 { 2.2 } else { 0. }));
        u.position.y = 1.;
        u.pose = Pose::Jump;
        u.velocity.y = 2.;
        c.update(&u, 1. / 60.);
        u.velocity.y = 0.;
        for v in carried(
            &u,
            ForcedKind::Mantle,
            Vec3::new(0., 2.2, 0.85),
            0.5,
            0.,
            Pose::Jump,
        ) {
            c.update(&v, 1. / 60.);
            assert_eq!(c.last_clip, "ClimbUp_1m");
        }
    }
    #[test]
    fn swimmers_play_the_swim_loops_at_the_surface() {
        // Afloat, the sim's feet 0.5 m under the surface: treading water standing, stroking on
        // the move, the body raised to the surface the clips have at their origin.
        let (mut c, mut u, _) = fighter(0.);
        let pelvis = c.names["pelvis"];
        u.traversal = Traversal::Swimming;
        let raised = |c: &mut Character, time: f32| {
            let shown = c.world[pelvis].w_axis.y;
            let mut bare = c.local.clone();
            for (n, l) in c.nodes.iter().zip(&mut bare) {
                *l = n.rest;
            }
            std::mem::swap(&mut c.local, &mut bare);
            c.apply_clip(c.last_clip, time, true, Bones::All);
            let clip = world_matrices(&c.nodes, &c.local)[pelvis].w_axis.y;
            std::mem::swap(&mut c.local, &mut bare);
            shown - clip
        };
        for frame in 0..20 {
            u.pose_time = frame as f32 / 60.;
            c.update(&u, 1. / 60.);
        }
        assert_eq!(c.last_clip, "Swim_Idle_Loop");
        let time = anim::idle::time(u.id, u.pose_time, c.clip_duration("Swim_Idle_Loop"));
        let up = raised(&mut c, time);
        assert!(
            (up - anim::traverse::SWIM_FLOAT).abs() < 0.01,
            "raised {up} m"
        );
        u.pose = Pose::Run;
        u.velocity = Vec3::Z * 2.7;
        for frame in 0..20 {
            u.pose_time = frame as f32 / 60.;
            c.update(&u, 1. / 60.);
            assert_eq!((c.last_clip, c.last_legs), ("Swim_Fwd_Loop", None));
        }
        let time = c.stride.phase * c.clip_duration("Swim_Fwd_Loop");
        let up = raised(&mut c, time);
        assert!(
            (up - anim::traverse::SWIM_FLOAT).abs() < 0.01,
            "raised {up} m"
        );
        // Without traversal, as before: the run.
        c.traversing = false;
        c.update(&u, 1. / 60.);
        assert_eq!(c.last_clip, "Jog_Fwd_Loop");
    }
    #[test]
    fn a_leap_ahead_lunges_and_a_hop_back_springs_off_by_distance() {
        // The sim's leaps (0.3 s, a 0.4 m hop, the Attack pose held): 6 m ahead behind the shield,
        // or 4 m back, as a sidestep or a disengage hops.
        for (to, clip, end) in [
            (Vec3::Z * 6., "Shield_Dash", 0.27),
            (Vec3::Z * -4., "NinjaJump_Start", 0.967),
        ] {
            let (mut c, u, mut fight) = fighter(0.);
            fight.action = Action::Leap;
            let frames = carried(&u, ForcedKind::Leap, to, 0.3, 0.4, Pose::Attack);
            for v in &frames {
                fight.action_time = v.pose_time;
                c.update_combat(v, Some(&fight), 1. / 60.);
                assert_eq!(c.last_clip, clip);
            }
            let last = c.carried.unwrap().time;
            assert!((last - end).abs() < 0.1 * end, "{clip} ended at {last}");
        }
    }

    /// How far above its feet the sim hangs a body's hands (`sim::climbing::HANG_DROP`).
    const HANG: f32 = anim::climb::HANG_DROP;
    /// A plain body of `art` hanging at the origin from a hold 1.9 m above its feet, facing it
    /// (+z), the top of a wall that runs from x 1.5 to -1.5 there (#248).
    fn climber(art: &str) -> (Character, UnitState) {
        let (_, mut u, _) = fighter(0.);
        let cat = assets::Catalog::load(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        )
        .unwrap();
        let look = Look {
            art: art.into(),
            ..Default::default()
        };
        let mut c = Character::load(&cat, &mut ModelCache::default(), &look).unwrap();
        c.blender.set_method(anim::Method::Off);
        c.set_ground(Arc::new(|_, _| -3.));
        c.set_holds(Arc::new(|p: Vec3| {
            (p.x.abs() <= 1.5 && p.y.abs() < 0.3 && p.z.abs() < 0.3).then_some(world::Hold {
                a: Vec3::new(1.5, anim::climb::HANG_DROP, 0.4),
                b: Vec3::new(-1.5, anim::climb::HANG_DROP, 0.4),
                out: Vec2::new(0., -1.),
                braced: true,
            })
        }));
        u.look = look.into();
        (u.pose, u.traversal) = (Pose::Hang, Traversal::Hanging);
        (c, u)
    }
    /// Where joint `i` of `c` is in the world, the unit at `position`.
    fn joint_at(c: &Character, position: Vec3, i: usize) -> Vec3 {
        let root = Mat4::from_rotation_translation(Quat::from_rotation_y(c.body_yaw()), position);
        root.transform_point3(c.world[i].w_axis.truncate())
    }
    /// Where `c`'s wrists are in the world, left and right, the unit at `position`.
    fn wrists(c: &Character, position: Vec3) -> [Vec3; 2] {
        c.arm_bones
            .map(|arm| joint_at(c, position, arm.unwrap()[2]))
    }
    /// Where the balls of `c`'s feet are in the world, the unit at `position`.
    fn balls(c: &Character, position: Vec3) -> [Vec3; 2] {
        c.ball_bones.unwrap().map(|i| joint_at(c, position, i))
    }
    #[test]
    fn every_frame_hangs_with_its_hands_on_the_hold_and_its_feet_on_the_wall() {
        // Short, tall and in between, each frame hangs from the hold (1.9 m up, its face 0.4 m
        // ahead) with its wrists where the clips close the hands on it and the balls of its feet
        // against the wall below.
        let grip = Vec3::new(
            0.,
            HANG - anim::climb::GRIP_DOWN,
            0.4 - anim::climb::GRIP_OUT,
        );
        for art in [
            "male",
            "female",
            "elf_female",
            "elf_male",
            "dwarf_male",
            "dwarf_female",
            "goblin_male",
            "foxfolk_male",
        ] {
            let (mut c, mut u) = climber(art);
            for frame in 0..40 {
                u.pose_time = frame as f32 / 60.;
                c.update(&u, 1. / 60.);
            }
            assert_eq!(c.last_clip, "Hang_Idle_Loop");
            for wrist in wrists(&c, u.position) {
                let off = (wrist - grip).with_x(0.);
                assert!(off.length() < 0.02, "{art}: wrist off the hold by {off}");
            }
            for ball in balls(&c, u.position) {
                let out = 0.4 - ball.z;
                assert!(
                    (out - anim::climb::BALL_OUT).abs() < 0.015 && ball.y > 0.2,
                    "{art}: a foot at {ball}, {out} m out from the wall"
                );
            }
        }
    }
    #[test]
    fn under_a_low_hold_the_body_sinks_to_it_and_keeps_its_feet_on_the_wall() {
        // A hold 1.6 m up, too low to hang from at full stretch, so the sim stands the body on
        // the ground under it: the body sinks the 0.3 m the hold lacks, its hands on the hold and
        // its feet still on the wall, above the ground.
        let (mut c, mut u) = climber("male");
        c.set_ground(Arc::new(|_, _| 0.));
        c.set_holds(Arc::new(|p: Vec3| {
            (p.x.abs() <= 1.5 && p.y.abs() < 0.3 && p.z.abs() < 0.3).then_some(world::Hold {
                a: Vec3::new(1.5, 1.6, 0.4),
                b: Vec3::new(-1.5, 1.6, 0.4),
                out: Vec2::new(0., -1.),
                braced: true,
            })
        }));
        for frame in 0..40 {
            u.pose_time = frame as f32 / 60.;
            c.update(&u, 1. / 60.);
        }
        let sunk = c.hang_reach - Vec3::Y * 0.3;
        assert!(c.raised.distance(sunk) < 1e-3, "{}", c.raised);
        let grip = Vec3::new(
            0.,
            1.6 - anim::climb::GRIP_DOWN,
            0.4 - anim::climb::GRIP_OUT,
        );
        for wrist in wrists(&c, u.position) {
            let off = (wrist - grip).with_x(0.);
            assert!(off.length() < 0.02, "wrist off the hold by {off}");
        }
        for ball in balls(&c, u.position) {
            let out = 0.4 - ball.z;
            assert!(
                ball.y > 0.05 && (out - anim::climb::BALL_OUT).abs() < 0.015,
                "a foot at {ball}"
            );
        }
    }
    #[test]
    fn shimmying_hands_keep_still_on_the_hold_at_any_speed() {
        // Shimmying right at the sim's speed and at half of it: the hands are in the same places
        // on the hold at the same distances slid, as the shimmy plays by how far the body slides.
        let dt = 1. / 60.;
        let run = |speed: f32| {
            let (mut c, mut u) = climber("male");
            for _ in 0..20 {
                c.update(&u, dt);
            }
            u.velocity = Vec3::X * -speed;
            let mut seen = vec![];
            // A stretch into the shimmy, then a whole loop, every 4 cm.
            let per = (0.04 / (speed * dt)).round() as usize;
            for frame in 1..=(1.08 / (speed * dt)).round() as usize {
                u.position += u.velocity * dt;
                c.update(&u, dt);
                assert_eq!(c.last_clip, "Hang_Shimmy_R_Loop");
                if frame % per == 0 && u.position.x <= -0.6 + 1e-3 {
                    seen.push(wrists(&c, u.position));
                }
            }
            seen
        };
        let (fast, slow) = (run(1.2), run(0.6));
        assert_eq!(fast.len(), 13);
        assert_eq!(fast.len(), slow.len());
        for (a, b) in fast.iter().zip(&slow) {
            for side in 0..2 {
                let apart = a[side].distance(b[side]);
                assert!(apart < 0.01, "the hands {apart} m apart: {a:?} and {b:?}");
            }
        }
    }
    #[test]
    fn moves_onto_holds_play_by_how_far_the_body_has_come() {
        // Up to the next hold, 0.8 m above, the drawn body moving every other frame: the clip
        // follows the body while its path says how far it has come, then the clock takes it to
        // the end as the hands finish the move.
        let dt = 1. / 60.;
        let (mut c, mut u) = climber("male");
        c.set_holds(Arc::new(|p: Vec3| {
            let at = |y: f32| world::Hold {
                a: Vec3::new(1.5, y, 0.4),
                b: Vec3::new(-1.5, y, 0.4),
                out: Vec2::new(0., -1.),
                braced: true,
            };
            if p.x.abs() > 1.5 || p.z.abs() > 0.3 {
                None
            } else if p.y.abs() < 0.3 {
                Some(at(HANG))
            } else if (p.y - 0.8).abs() < 0.3 {
                Some(at(HANG + 0.8))
            } else {
                None
            }
        }));
        for _ in 0..10 {
            c.update(&u, dt);
        }
        let up = protocol::ForcedMove {
            kind: ForcedKind::Hold,
            from: Vec3::ZERO,
            to: Vec3::Y * 0.8,
            start: 2.,
            duration: 0.4,
        };
        u.traversal = Traversal::Forced(up);
        let mut drawn = 0.;
        for frame in 0..=30 {
            let k = (frame as f32 / 24.).min(1.);
            if frame % 2 == 0 {
                drawn = k;
                u.position = anim::climb::carry(up.from, up.to, k);
            }
            c.update(&u, dt);
            assert_eq!(c.last_clip, "Hang_Climb_Up");
            let share = c.climb.as_ref().unwrap().progress.share();
            if drawn < 0.6 {
                assert!(
                    (share - drawn).abs() < 0.02,
                    "frame {frame}: {share} at {drawn}"
                );
            }
        }
        assert_eq!(c.climb.as_ref().unwrap().progress.share(), 1.);
        // Lowering itself over the edge from standing on the top, 0.7 m back from the hang.
        let (mut c, mut u) = climber("male");
        let top = Vec3::new(0., HANG, 0.7);
        (u.position, u.pose, u.traversal) = (top, Pose::Idle, Traversal::Ground);
        for _ in 0..10 {
            c.update(&u, dt);
        }
        let drop = protocol::ForcedMove {
            kind: ForcedKind::Hold,
            from: top,
            to: Vec3::ZERO,
            start: 2.,
            duration: 0.6,
        };
        (u.pose, u.traversal) = (Pose::Hang, Traversal::Forced(drop));
        for frame in 0..=36 {
            let k = frame as f32 / 36.;
            if frame % 2 == 0 {
                drawn = k;
                u.position = anim::climb::carry(drop.from, drop.to, k);
            }
            c.update(&u, dt);
            assert_eq!(c.last_clip, "Hang_Drop");
            let share = c.climb.as_ref().unwrap().progress.share();
            assert!(
                (share - drawn).abs() < 0.02,
                "frame {frame}: {share} at {drawn}"
            );
        }
    }
    #[test]
    fn round_a_corner_the_hop_plays_by_the_clock_the_way_the_body_shimmied() {
        // Shimmying right to the end of the hold, then wrapping round the corner there: the hop to
        // the right plays by the clock wherever the drawn body is, as the sim swings it round the
        // corner on the easing the hop is made on.
        let dt = 1. / 60.;
        let (mut c, mut u) = climber("male");
        u.velocity = Vec3::X * -1.2;
        for _ in 0..10 {
            u.position += u.velocity * dt;
            c.update(&u, dt);
        }
        assert_eq!(c.last_clip, "Hang_Shimmy_R_Loop");
        let corner = protocol::ForcedMove {
            kind: ForcedKind::Corner,
            from: u.position,
            to: u.position + Vec3::new(-0.6, 0., 0.6),
            start: 3.,
            duration: 0.5,
        };
        (u.traversal, u.velocity) = (Traversal::Forced(corner), Vec3::ZERO);
        for frame in 1..=30 {
            c.update(&u, dt);
            assert_eq!(c.last_clip, "Hang_Hop_R");
            let share = c.climb.as_ref().unwrap().progress.share();
            let clock = frame as f32 / 30.;
            assert!((share - clock).abs() < 1e-3, "frame {frame}: {share}");
        }
    }
    #[test]
    fn without_traversal_a_climber_hangs_as_the_clips_have_it() {
        // `--traversal off`, to compare: a dwarf shimmying hangs where the clips put it, its
        // hands and feet left alone.
        let (mut c, mut u) = climber("dwarf_male");
        c.traversing = false;
        u.velocity = Vec3::X * -0.6;
        for frame in 0..30 {
            u.pose_time = frame as f32 / 60.;
            u.position += u.velocity / 60.;
            c.update(&u, 1. / 60.);
        }
        assert_eq!(c.last_clip, "Hang_Shimmy_R_Loop");
        assert_eq!((c.raised, c.hold_grip), (Vec3::ZERO, [0.; 2]));
    }
    #[test]
    fn off_the_wall_a_short_body_eases_down_from_its_reach() {
        // A dwarf hangs a third of a metre higher than the clips do, to reach the hold. Letting
        // go, it comes down from there smoothly, all of it within a few tenths of a second.
        let dt = 1. / 60.;
        let (mut c, mut u) = climber("dwarf_male");
        for _ in 0..30 {
            c.update(&u, dt);
        }
        assert!(c.raised.distance(c.hang_reach) < 1e-3, "{}", c.raised);
        assert!(c.raised.y > 0.3, "{}", c.raised);
        (u.pose, u.traversal) = (Pose::Fall, Traversal::Ground);
        let mut last = c.raised;
        for frame in 0..18 {
            u.velocity.y = -anim::land::GRAVITY * frame as f32 * dt;
            u.position.y += u.velocity.y * dt;
            c.update(&u, dt);
            let moved = last.distance(c.raised);
            assert!(moved < 0.1, "frame {frame}: moved {moved}");
            last = c.raised;
        }
        assert!(c.raised.length() < 0.01, "{}", c.raised);
    }
}
