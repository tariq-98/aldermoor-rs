//! Catalogue assembly and animation. The workshop is the reference, not filenames or rig order.
use crate::model::*;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3};
use protocol::{Look, Pose, UnitState};
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
    pub palette: Vec<Mat4>,
    pub hidden: u32,
    pub base: [f32; 4],
    pub morph: [f32; 2],
}
pub struct Character {
    pub lod: usize,
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
    idle: String,
    gear_scale: f32,
    last_pose: Pose,
    attack: usize,
    transition: Option<(String, f32)>,
    last_drawn: bool,
    blend_from: Vec<Transform>,
    blend_time: f32,
    last_clip: String,
    drawn_now: bool,
    main_offset: f32,
    grab_offset: Option<f32>,
    bow_morph: [f32; 2],
    springs: Vec<crate::springs::SpringBones>,
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
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(catalog.path("catalog/humanoid.json"))?)?;
        let raw_gear: serde_json::Value =
            serde_json::from_slice(&std::fs::read(catalog.path("catalog/gear.json"))?)?;
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
            idle: "Idle_Loop".into(),
            gear_scale: body.and_then(|b| b.gear_scale).unwrap_or(1.),
            last_pose: Pose::Idle,
            attack: 0,
            transition: None,
            last_drawn: look.drawn,
            blend_from: local,
            blend_time: 1.,
            last_clip: String::new(),
            drawn_now: look.drawn,
            main_offset: 0.,
            grab_offset: None,
            bow_morph: [0.; 2],
            springs: Vec::new(),
        };
        if beast.is_some() {
            let raw_creatures: serde_json::Value =
                serde_json::from_slice(&std::fs::read(catalog.path("catalog/creatures.json"))?)?;
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
            c.apply_clip(&idle, 0., true, false);
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
    fn clip_for(&self, u: &UnitState) -> String {
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
            Pose::Walk => "Walk_Loop".into(),
            Pose::Run => if u.velocity.length() > 6. {
                "Sprint_Loop"
            } else {
                "Jog_Fwd_Loop"
            }
            .into(),
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
    fn apply_clip(&mut self, name: &str, t: f32, looping: bool, upper_only: bool) {
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
            if upper_only && !self.upper.get(i).copied().unwrap_or(false) {
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
    pub fn update(&mut self, u: &UnitState, dt: f32) {
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
        if name != self.last_clip {
            self.blend_from = self.local.clone();
            self.blend_time = 0.;
            self.last_clip = name.clone();
        }
        self.last_pose = u.pose;
        self.blend_time = (self.blend_time + dt).min(0.16);
        for (n, l) in self.nodes.iter().zip(&mut self.local) {
            *l = n.rest;
        }
        let locomotion = matches!(u.pose, Pose::Walk | Pose::Run);
        let looping = matches!(u.pose, Pose::Idle | Pose::Walk | Pose::Run | Pose::Fall);
        let rate = match u.pose {
            Pose::Walk => (u.velocity.length()
                / if self.humanoid {
                    1.5
                } else {
                    self.beast_speeds[0]
                })
            .clamp(0.3, 2.),
            Pose::Run => (u.velocity.length()
                / if self.humanoid {
                    4.5
                } else {
                    self.beast_speeds[1]
                })
            .clamp(0.5, 1.6),
            Pose::Dodge => self
                .clips
                .clips
                .get(&name)
                .map_or(1., |a| a.duration / 0.45),
            Pose::Attack => self.clips.clips.get(&name).map_or(1., |a| a.duration / 0.5),
            _ => 1.,
        };
        self.apply_clip(&name, u.pose_time * rate, looping, false);
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
            self.apply_clip(&idle, u.pose_time, true, true);
        }
        let transition = self.transition.clone();
        if let Some((n, t)) = &transition {
            self.apply_clip(n, *t, false, true);
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
            .unwrap_or((&name, u.pose_time * rate));
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
        self.world = world_matrices(&self.nodes, &self.local);
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
        self.world = world_matrices(&self.nodes, &self.local);
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
            for (pi, p) in part.model.primitives.iter().enumerate() {
                if part.eyes_hidden && p.vertices.first().is_some_and(|v| v.position[3] < 0.) {
                    continue;
                }
                let palette = if let Some(si) = p.skin {
                    let skin = &part.model.skins[si];
                    skin.joints
                        .iter()
                        .zip(&skin.inverse)
                        .map(|(j, inv)| self.world[part.remap[*j]] * *inv)
                        .collect()
                } else {
                    Vec::new()
                };
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
