//! Catalogue assembly and animation. The workshop is the reference, not filenames or rig order.
use crate::model::*;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3, Vec3Swizzles};
use protocol::{Action, Look, Pose, UnitCombat, UnitState};
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
    pub parts: Vec<Part>,
    pub nodes: Vec<Node>,
    pub local: Vec<Transform>,
    pub world: Vec<Mat4>,
    pub names: HashMap<String, usize>,
    pub look: Look,
    pub clips: Arc<Model>,
    pub clip_meta: HashMap<String, assets::Clip>,
    pub rig_height: f32,
    pelvis_ratio: f32,
    reference_pelvis: Vec3,
    humanoid: bool,
    beast_speeds: [f32; 2],
    upper: Vec<bool>,
    role_clips: HashMap<String, String>,
    main: Option<assets::Item>,
    off_hand: Option<assets::Item>,
    combat: Option<UnitCombat>,
    windup: Option<f32>,
    rally_time: Option<f32>,
    was_guarding: bool,
    idle: String,
    gear_scale: f32,
    last_pose: Pose,
    attack: usize,
    transition: Option<(String, f32)>,
    last_drawn: bool,
    blend_from: Vec<Transform>,
    blend_time: f32,
    last_clip: String,
    /// The legs' locomotion clip under an action on the move.
    last_legs: Option<&'static str>,
    /// Where the walk, jog or sprint cycle is, from 0 to 1.
    stride: f32,
    drawn_now: bool,
    main_offset: f32,
    grab_offset: Option<f32>,
    bow_morph: [f32; 2],
    springs: Vec<crate::springs::SpringBones>,
    children: Vec<Vec<usize>>,
    lod_parts: HashMap<usize, (Vec<Part>, Arc<Model>)>,
}

/// Which bones a clip drives. The upper body is `spine_01` and everything under it.
#[derive(Clone, Copy, PartialEq)]
enum Bones {
    All,
    Upper,
    Lower,
}
/// How far round from the hips an action on the move may turn the chest, beyond the clip's own
/// twist, to face where the clip faced it.
const MAX_TWIST: f32 = std::f32::consts::FRAC_PI_3;
/// The humanoid loop for walking or running at `speed`.
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
        let names = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.name.clone(), i))
            .collect();
        let local = nodes.iter().map(|n| n.rest).collect::<Vec<_>>();
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
        let mut c = Self {
            lod,
            partial_updates: true,
            parts: Vec::new(),
            world: world_matrices(&nodes, &local),
            nodes,
            names,
            local: local.clone(),
            look: look.clone(),
            clips,
            clip_meta: h
                .animations
                .clips
                .iter()
                .map(|c| (c.name.clone(), c.clone()))
                .collect(),
            rig_height: body.map_or(1., |b| b.height),
            pelvis_ratio: body.map_or(1., |b| b.pelvis[1] / h.animations.pelvis[1]),
            reference_pelvis,
            humanoid: body.is_some(),
            beast_speeds: [1.; 2],
            upper: Vec::new(),
            role_clips: beast
                .map(|b| b.clips.clone().into_iter().collect())
                .unwrap_or_default(),
            main: None,
            off_hand: look
                .gear
                .iter()
                .find(|(slot, _)| slot == "off_hand")
                .and_then(|(_, id)| catalog.gear.as_ref()?.items.iter().find(|i| i.id == *id))
                .cloned(),
            combat: None,
            windup: None,
            rally_time: None,
            was_guarding: false,
            idle: "Idle_Loop".into(),
            gear_scale: body.and_then(|b| b.gear_scale).unwrap_or(1.),
            last_pose: Pose::Idle,
            attack: 0,
            transition: None,
            last_drawn: look.drawn,
            blend_from: local,
            blend_time: 1.,
            last_clip: String::new(),
            last_legs: None,
            stride: 0.,
            drawn_now: look.drawn,
            main_offset: 0.,
            grab_offset: None,
            bow_morph: [0.; 2],
            springs: Vec::new(),
            children: Vec::new(),
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
            let idle = c
                .role_clips
                .get("idle")
                .cloned()
                .context("beast must supply an idle clip")?;
            ensure!(
                c.clips.clips.contains_key(&idle),
                "beast idle clip is missing"
            );
            c.apply_clip(&idle, 0., true, Bones::All);
            c.world = world_matrices(&c.nodes, &c.local);
            c.blend_from = c.local.clone();
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
                c.idle = i.clone();
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
                c.main = Some(i.clone());
            }
            if let Some(f) = &i.file {
                ensure!(
                    c.names.contains_key(&attachment.held)
                        && c.names.contains_key(&attachment.stowed),
                    "missing equipment socket for {}",
                    i.id
                );
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
        c.upper = (0..c.nodes.len())
            .map(|mut i| {
                loop {
                    if c.nodes[i].name == "spine_01" {
                        return true;
                    }
                    if let Some(p) = c.nodes[i].parent {
                        i = p
                    } else {
                        return false;
                    }
                }
            })
            .collect();
        c.blend_from = c.local.clone();
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
        let (parts, clips) = if let Some(parts) = self.lod_parts.remove(&lod) {
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
            (next.parts, next.clips)
        };
        self.lod_parts.insert(
            self.lod,
            (
                std::mem::replace(&mut self.parts, parts),
                std::mem::replace(&mut self.clips, clips),
            ),
        );
        self.lod = lod;
        Ok(())
    }
    pub(crate) fn update_chain(&mut self, root: usize) {
        if !self.partial_updates {
            self.world = world_matrices(&self.nodes, &self.local);
            return;
        }
        if self.children.len() != self.nodes.len() {
            self.children = vec![Vec::new(); self.nodes.len()];
            for (i, n) in self.nodes.iter().enumerate() {
                if let Some(p) = n.parent {
                    self.children[p].push(i);
                }
            }
        }
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
    fn item_clip(&self, key: &str, fallback: &str) -> String {
        self.main
            .as_ref()
            .and_then(|i| i.clips.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or(fallback)
            .into()
    }
    fn shield_clip(&self, key: &str, fallback: &str) -> String {
        self.off_hand
            .as_ref()
            .and_then(|i| i.clips.get(key))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .unwrap_or_else(|| self.item_clip(key, fallback))
    }
    fn attack_clip(&self, index: usize) -> String {
        self.main
            .as_ref()
            .and_then(|i| i.clips.get("attacks"))
            .and_then(|v| v.as_array())
            .and_then(|a| a.get(index.min(a.len().saturating_sub(1))))
            .and_then(|v| v.as_str())
            .unwrap_or("Punch_Jab")
            .into()
    }
    fn action_clip(&self, u: &UnitState) -> Option<String> {
        let c = self.combat.as_ref()?;
        if c.downed {
            return Some(if self.humanoid {
                "Death01".into()
            } else {
                self.role_clips.get("death")?.clone()
            });
        }
        if self.rally_time.is_some() && self.humanoid {
            return Some("LayToIdle".into());
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
            return self.role_clips.get(role).cloned();
        }
        let family = self.main.as_ref().map_or("", |i| i.family.as_str());
        Some(match c.action {
            Action::Slash => self.attack_clip(0),
            Action::Slash2 => self.attack_clip(1),
            Action::Thrust => {
                if family == "spear" {
                    "Spear_Thrust".into()
                } else {
                    self.attack_clip(2)
                }
            }
            Action::Overhead | Action::Slam => {
                if family == "greatsword" {
                    "GreatSword_Overhead".into()
                } else {
                    "Sword_Overhead".into()
                }
            }
            Action::Spin => {
                if family == "greatsword" {
                    "GreatSword_Swing".into()
                } else {
                    "Sword_Regular_Combo".into()
                }
            }
            Action::Bash => self.shield_clip("bash", "Punch_Jab"),
            Action::Parry => "Sword_Block".into(),
            Action::Shoot => match family {
                "bow" => "Bow_Shoot",
                "wand" => "Spell_Simple_Shoot",
                _ => "OverhandThrow",
            }
            .into(),
            Action::CastBolt => self.attack_clip(0),
            Action::CastArea => self.item_clip("slam", "Spell_Simple_Shoot"),
            Action::Channel => self.item_clip("channel", "Spell_Simple_Idle_Loop"),
            Action::CastSelf => if c.cast.is_some() {
                "Spell_Simple_Enter"
            } else {
                "Spell_Simple_Exit"
            }
            .into(),
            Action::Kick => "Punch_Cross".into(),
            Action::Leap => self.shield_clip("dash", "NinjaJump_Start"),
            Action::Charge => self.item_clip(
                "dash",
                if u.velocity.xz().length() > 7. {
                    "Sprint_Loop"
                } else {
                    "Jog_Fwd_Loop"
                },
            ),
            Action::Roar => "Spell_Simple_Enter".into(),
            Action::Stagger => if self.was_guarding {
                "Idle_Shield_Break"
            } else {
                "Hit_Knockback"
            }
            .into(),
            Action::Revive => "Fixing_Kneeling".into(),
            _ => return None,
        })
    }
    fn clip_for(&self, u: &UnitState) -> String {
        if let Some(clip) = self.action_clip(u) {
            return clip;
        }
        if !self.humanoid {
            let role = match u.pose {
                Pose::Idle => "idle",
                Pose::Walk => "walk",
                Pose::Run | Pose::Dodge => "run",
                Pose::Jump | Pose::Fall => "jump",
                Pose::Attack | Pose::Cast => "attack",
                Pose::Hit => "hit",
                Pose::Dead => "death",
            };
            return self.role_clips.get(role).cloned().unwrap_or_default();
        }
        let drawn_idle = self
            .main
            .as_ref()
            .and_then(|i| i.clips.get("idle"))
            .and_then(|v| v.as_str());
        match u.pose {
            Pose::Idle => if u.look.drawn {
                drawn_idle.unwrap_or(&self.idle)
            } else {
                &self.idle
            }
            .into(),
            Pose::Walk | Pose::Run => locomotion_clip(u.pose, u.velocity.xz().length()).into(),
            Pose::Jump => "Jump_Start".into(),
            Pose::Fall => "Jump_Loop".into(),
            Pose::Dodge => "Roll".into(),
            Pose::Attack | Pose::Cast => self
                .main
                .as_ref()
                .and_then(|i| i.clips.get("attacks"))
                .and_then(|v| v.as_array())
                .and_then(|a| a.get(self.attack % a.len().max(1)))
                .and_then(|v| v.as_str())
                .unwrap_or(if u.pose == Pose::Cast {
                    "Spell_Simple_Shoot"
                } else {
                    "Sword_Regular_A"
                })
                .into(),
            Pose::Hit => "Hit_Chest".into(),
            Pose::Dead => "Death01".into(),
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
    /// Puts walking legs under the action just applied: `legs` drives the hips and legs at the
    /// stride phase, and the upper body turns about the vertical back toward where the action's
    /// own hips faced it. The turn stops once the chest is `MAX_TWIST` round from the hips (or the
    /// clip's own twist, if that is more), so swings that pivot the whole body keep a believable
    /// spine while side-on stances (a bow, a wand) still aim forward.
    fn walk_under(&mut self, legs: &str) {
        let ids = ["pelvis", "spine_01", "spine_03"].map(|n| self.names.get(n).copied());
        let [Some(pelvis), Some(spine), Some(chest)] = ids else {
            return;
        };
        let rest_hips = self.rotation_to(pelvis, None, true);
        let rest_chest = self.rotation_to(chest, None, true);
        let clip_hips = heading(self.rotation_to(pelvis, None, false), rest_hips);
        let clip_chest = heading(self.rotation_to(chest, None, false), rest_chest);
        let time = self.stride * self.clips.clips.get(legs).map_or(1., |a| a.duration);
        self.apply_clip(legs, time, true, Bones::Lower);
        let hips = self.rotation_to(pelvis, None, false);
        let hips_heading = heading(hips, rest_hips);
        let chest_heading = heading(self.rotation_to(chest, None, false), rest_chest);
        let own = wrap(clip_chest - clip_hips);
        let twist = wrap(clip_chest - hips_heading).clamp(own.min(-MAX_TWIST), own.max(MAX_TWIST));
        // A turn about the vertical, applied above the hips, adds straight onto the chest's heading.
        let turn = wrap(hips_heading + twist - chest_heading);
        self.local[spine].rotation =
            (hips.inverse() * Quat::from_rotation_y(turn) * hips * self.local[spine].rotation)
                .normalize();
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
        let Some(clip) = self.clips.clips.get(name) else {
            return;
        };
        let t = if looping {
            t.rem_euclid(clip.duration.max(0.001))
        } else {
            t.clamp(0., clip.duration)
        };
        for track in &clip.tracks {
            let Some(&i) = self.names.get(&track.node) else {
                continue;
            };
            if bones != Bones::All
                && self.upper.get(i).copied().unwrap_or(false) != (bones == Bones::Upper)
            {
                continue;
            }
            let v = track.sample(t);
            match track.property {
                Property::Rotation => {
                    self.local[i].rotation = Quat::from_array(v.to_array()).normalize()
                }
                Property::Translation => {
                    if !self.humanoid {
                        self.local[i].translation = v.truncate()
                    } else if track.node == "pelvis" {
                        self.local[i].translation = self.nodes[i].rest.translation
                            + (v.truncate() - self.reference_pelvis) * self.pelvis_ratio
                    }
                }
                Property::Scale => {
                    if !self.humanoid {
                        self.local[i].scale = v.truncate()
                    }
                }
            }
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
        self.combat = combat.cloned();
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
                .main
                .as_ref()
                .and_then(|i| i.clips.get(if u.look.drawn { "ready" } else { "stow" }))
                .and_then(|v| v.as_str())
                .map(|s| (s.to_owned(), 0.));
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
            if *time >= self.clips.clips.get(name).map_or(0., |a| a.duration) {
                self.transition = None;
            }
        }
        let name = self.clip_for(u);
        let speed = u.velocity.xz().length();
        let moving = self.moving_legs(u);
        let legs = moving.map(|pose| locomotion_clip(pose, speed));
        if name != self.last_clip || legs != self.last_legs {
            self.blend_from = self.local.clone();
            self.blend_time = 0.;
            self.last_clip = name.clone();
            self.last_legs = legs;
        }
        self.last_pose = u.pose;
        self.blend_time = (self.blend_time + dt).min(0.16);
        for (n, l) in self.nodes.iter().zip(&mut self.local) {
            *l = n.rest;
        }
        let locomotion = matches!(u.pose, Pose::Walk | Pose::Run);
        let acting = self.action_clip(u).is_some();
        let walking = locomotion && !acting;
        let looping = matches!(u.pose, Pose::Idle | Pose::Walk | Pose::Run | Pose::Fall)
            || self.combat.as_ref().is_some_and(|c| {
                matches!(c.action, Action::Channel | Action::Revive)
                    || c.action == Action::Spin && c.cast.as_ref().is_some_and(|c| c.channel)
            });
        let rate = match u.pose {
            Pose::Walk | Pose::Run => self.stride_rate(u.pose, speed),
            Pose::Dodge => self
                .clips
                .clips
                .get(&name)
                .map_or(1., |a| a.duration / 0.45),
            Pose::Attack => self.clips.clips.get(&name).map_or(1., |a| a.duration / 0.5),
            _ => 1.,
        };
        // One stride phase runs through attacks on the move and through changes between walk,
        // jog and sprint (all three loops start on the left foot), so the feet never restart.
        let stride = match moving {
            Some(pose) => Some((locomotion_clip(pose, speed), self.stride_rate(pose, speed))),
            None => walking.then_some((name.as_str(), rate)),
        };
        if let Some((clip, rate)) = stride {
            let length = self
                .clips
                .clips
                .get(clip)
                .map_or(1., |a| a.duration.max(0.001));
            self.stride = (self.stride + dt * rate / length).fract();
        }
        let mut clip_time = if walking {
            self.stride * self.clips.clips.get(&name).map_or(1., |a| a.duration)
        } else {
            u.pose_time * rate
        };
        if let Some(c) = &self.combat {
            if c.downed {
                clip_time = self.clips.clips.get(&name).map_or(0., |a| a.duration);
            } else if let Some(t) = self.rally_time {
                clip_time = t;
            } else if let Some(windup) = self.windup.filter(|_| u.pose == Pose::Attack) {
                // The blow lands on the clip's `hit` marker just as the wind-up ends.
                let duration = self.clips.clips.get(&name).map_or(1., |a| a.duration);
                let hit = self
                    .clip_meta
                    .get(&name)
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
        self.apply_clip(&name, clip_time, looping, Bones::All);
        if let Some(legs) = legs {
            self.walk_under(legs);
        }
        if locomotion
            && u.look.drawn
            && self.humanoid
            && let Some(idle) = self
                .main
                .as_ref()
                .and_then(|i| i.clips.get("idle"))
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        {
            self.apply_clip(&idle, u.pose_time, true, Bones::Upper);
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
                "Sword_Block".into()
            } else {
                self.shield_clip("block", "Sword_Block")
            };
            let looping = !parrying && self.clip_meta.get(&guard).is_some_and(|m| m.looping);
            self.apply_clip(&guard, time, looping, Bones::Upper);
            self.transition = None;
        }
        let transition = self.transition.clone();
        if let Some((n, t)) = &transition {
            self.apply_clip(n, *t, false, Bones::Upper);
        }
        let blend = self.blend_time / 0.16;
        for (i, p) in self.local.iter_mut().enumerate() {
            *p = self.blend_from[i].blend(*p, blend);
        }
        self.world = world_matrices(&self.nodes, &self.local);
        self.drawn_now = u.look.drawn;
        self.main_offset = 0.;
        let (active, t) = transition
            .as_ref()
            .map(|(n, t)| (n.as_str(), *t))
            .unwrap_or((&name, clip_time));
        self.perform(active, t);
        let root = Mat4::from_rotation_translation(Quat::from_rotation_y(u.yaw), u.position);
        let mut springs = std::mem::take(&mut self.springs);
        for s in &mut springs {
            s.update(self, root, dt);
        }
        self.springs = springs;
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
    fn arm_to(&mut self, side: &str, target: Mat4, weight: f32) {
        if weight <= 0. {
            return;
        }
        let ids = [
            format!("upperarm_{side}"),
            format!("lowerarm_{side}"),
            format!("hand_{side}"),
            format!("socket_hand_{side}"),
        ];
        let Some(v) = ids
            .iter()
            .map(|n| self.names.get(n).copied())
            .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        let [a, b, h, s] = [v[0], v[1], v[2], v[3]];
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
            "l"
        } else {
            "r"
        };
        let attachment = self
            .parts
            .iter()
            .filter_map(|p| p.attachment.as_ref())
            .find(|a| a.main && !a.sheath)
            .cloned();
        let Some(a) = attachment else { return };
        let mut offset = 0.;
        let grab = meta.event("grab");
        let stow = meta.event("stow");
        if grab.is_some() || stow.is_some() {
            let socket = self.socket(&a.stowed);
            let hand = self.socket(&format!("socket_hand_{side}"));
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
        let item = self.socket(if self.drawn_now { &a.held } else { &a.stowed })
            * Mat4::from_scale(Vec3::splat(self.gear_scale))
            * Mat4::from_translation(Vec3::Y * offset);
        if let Some(grip) = main.grips.get("off_hand") {
            let other = if side == "r" { "l" } else { "r" };
            let hand = self.socket(&format!("socket_hand_{other}"));
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
        if let Some(string) = main.string {
            let other = if side == "r" { "l" } else { "r" };
            let hand = self.socket(&format!("socket_hand_{other}"));
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
            health: 100.,
            max_health: 100.,
            hostile: false,
            target: None,
            look,
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
        c.off_hand = None;
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
            health: 100.,
            max_health: 100.,
            hostile: false,
            target: None,
            look,
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
        let c = Character::load(&cat, &mut ModelCache::default(), &look).unwrap();
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
            health: 100.,
            max_health: 100.,
            hostile: false,
            target: None,
            look,
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
        let (mut c, mut u, mut fight) = fighter(4.5);
        for _ in 0..20 {
            c.update_combat(&u, Some(&fight), 1. / 60.);
        }
        let jog = c.clips.clips["Jog_Fwd_Loop"].duration;
        let start = c.stride;
        u.pose = Pose::Attack;
        fight.action = Action::Slash;
        for frame in 0..30 {
            fight.action_time = frame as f32 / 60.;
            u.pose_time = fight.action_time;
            c.update_combat(&u, Some(&fight), 1. / 60.);
        }
        assert_eq!(c.last_clip, "Sword_Regular_A");
        assert_eq!(c.last_legs, Some("Jog_Fwd_Loop"));
        let stride = (start + 30. / 60. / jog).fract();
        assert!(
            (c.stride - stride).abs() < 1e-4,
            "the stride runs on through the swing"
        );
        for leg in ["thigh_l", "calf_l", "foot_r"] {
            let jogging = sampled(&c, "Jog_Fwd_Loop", leg, c.stride * jog);
            assert!(
                same(c.local[c.names[leg]].rotation, jogging),
                "{leg} keeps jogging under the swing"
            );
        }
        let swing = c.clips.clips["Sword_Regular_A"].duration;
        let arm = sampled(&c, "Sword_Regular_A", "upperarm_r", 29. / 60. * swing / 0.5);
        assert!(
            same(c.local[c.names["upperarm_r"]].rotation, arm),
            "the arm swings"
        );
        u.pose = Pose::Run;
        u.pose_time = 0.;
        c.update_combat(&u, Some(&fight), 1. / 60.);
        assert_eq!(c.last_clip, "Jog_Fwd_Loop");
        assert!(
            (c.stride - (stride + 1. / 60. / jog).fract()).abs() < 1e-4,
            "the jog carries on mid-stride after the swing"
        );
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
                if frame < 10 {
                    continue; // still blending in
                }
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
}
