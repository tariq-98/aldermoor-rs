//! glTF decoding, kept independent of Metal so asset/animation checks run headlessly.
use crate::visibility::Bounds;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Quat, Vec3, Vec4};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 4], // xyz; region index, or -1 for unclassified geometry
    pub normal: [f32; 4],
    pub tangent: [f32; 4],
    pub color: [f32; 4],
    pub uv: [f32; 4],
    pub joints: [u32; 4],
    pub weights: [f32; 4],
}
impl Vertex {
    pub fn plain(p: Vec3, n: Vec3, color: [f32; 4]) -> Self {
        Self {
            position: [p.x, p.y, p.z, -1.],
            normal: [n.x, n.y, n.z, 0.],
            tangent: [0.; 4],
            color,
            uv: [0.; 4],
            joints: [0; 4],
            weights: [1., 0., 0., 0.],
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}
impl Transform {
    pub fn matrix(self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}
impl anim::Bone for Transform {
    fn rotation(&self) -> Quat {
        self.rotation
    }
    fn translation(&self) -> Vec3 {
        self.translation
    }
    fn set(&mut self, rotation: Quat, translation: Vec3) {
        self.rotation = rotation;
        self.translation = translation;
    }
}
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub rest: Transform,
}
#[derive(Clone)]
pub struct Skin {
    pub joints: Vec<usize>,
    pub inverse: Vec<Mat4>,
}
pub struct Texture {
    pub key: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Mutex<Option<Vec<u8>>>,
    pub compressed: Option<crate::ktx::Ktx>,
}
#[derive(Clone)]
pub struct Material {
    pub name: String,
    pub base: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    pub maps: [Option<Arc<Texture>>; 5], // base, normal, metallic-roughness, emissive, occlusion
    pub double_sided: bool,
    pub alpha_mode: u32,
    pub alpha_cutoff: f32,
}
impl Default for Material {
    fn default() -> Self {
        Self {
            name: String::new(),
            base: [1.; 4],
            metallic: 0.,
            roughness: 0.8,
            emissive: [0.; 3],
            normal_scale: 1.,
            occlusion_strength: 1.,
            maps: Default::default(),
            double_sided: false,
            alpha_mode: 0,
            alpha_cutoff: 0.5,
        }
    }
}
pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub material: Material,
    pub bounds: Bounds,
    pub node: usize,
    pub skin: Option<usize>,
    pub morphs: Vec<Vec<[f32; 3]>>,
    pub morph_names: Vec<String>,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Property {
    Translation,
    Rotation,
    Scale,
}
/// One property of one bone through a clip, as evenly spaced keys (#215): clips are baked at 30
/// fps, and a track that isn't evenly spaced is resampled when it loads, so sampling indexes
/// its two keys instead of searching for them.
#[derive(Clone)]
pub struct Track {
    pub node: String,
    pub property: Property,
    /// When the first key is.
    start: f32,
    /// Keys a second; 0 for a track that never changes, which keeps a single key.
    rate: f32,
    /// Rotations are unit quaternions.
    values: Vec<Vec4>,
    /// Holds each key until the next rather than blending to it.
    step: bool,
}
impl Track {
    pub fn new(
        node: String,
        property: Property,
        times: &[f32],
        values: &[Vec4],
        interpolation: gltf::animation::Interpolation,
    ) -> Self {
        use gltf::animation::Interpolation::*;
        let cubic = interpolation == CubicSpline;
        let key = |i: usize| values[if cubic { i * 3 + 1 } else { i }];
        let start = times[0];
        let span = times[times.len() - 1] - start;
        let gap = span / (times.len() - 1).max(1) as f32;
        let even = times
            .iter()
            .enumerate()
            .all(|(i, t)| (t - start - gap * i as f32).abs() <= gap * 0.01);
        let (rate, mut values) = if span <= 0. {
            (0., vec![key(times.len() - 1)])
        } else if (0..times.len()).all(|i| key(i).abs_diff_eq(key(0), 1e-6)) {
            (0., vec![key(0)])
        } else if even && !cubic {
            (1. / gap, values.to_vec())
        } else {
            // 30 keys a second at least, the first and last where the track's are.
            let frames = (span * 30.).ceil() as usize;
            let rate = frames as f32 / span;
            let times_at = (0..=frames).map(|f| start + f as f32 / rate);
            let keys = times_at.map(|t| sample_keys(times, values, interpolation, property, t));
            (rate, keys.collect())
        };
        if property == Property::Rotation {
            for v in &mut values {
                *v = v.normalize();
            }
        }
        Self {
            node,
            property,
            start,
            rate,
            values,
            step: interpolation == Step,
        }
    }
    pub fn sample(&self, t: f32) -> Vec4 {
        let last = self.values.len() - 1;
        let at = ((t - self.start) * self.rate).clamp(0., last as f32);
        let i = at as usize;
        let a = self.values[i];
        if i == last || self.step {
            return a;
        }
        let b = self.values[i + 1];
        let f = at - i as f32;
        if self.property == Property::Rotation {
            // Between keys a 30th of a second apart, a normalized lerp comes within 0.05° of slerp
            // for far less, unless the bone swings over 30° in the step (a fast blow).
            let d = a.dot(b);
            if d.abs() > 0.966 {
                a.lerp(b * d.signum(), f).normalize()
            } else {
                Quat::from_vec4(a).slerp(Quat::from_vec4(b), f).into()
            }
        } else {
            a.lerp(b, f)
        }
    }
}
/// A glTF sampler's value at `t`, searching its keys: only for resampling a track that isn't
/// evenly spaced.
fn sample_keys(
    times: &[f32],
    values: &[Vec4],
    interpolation: gltf::animation::Interpolation,
    property: Property,
    t: f32,
) -> Vec4 {
    let cubic = interpolation == gltf::animation::Interpolation::CubicSpline;
    let value = |i: usize| values[if cubic { i * 3 + 1 } else { i }];
    let hi = times.partition_point(|x| *x <= t);
    if hi == 0 {
        return value(0);
    }
    if hi == times.len() {
        return value(hi - 1);
    }
    let lo = hi - 1;
    let dt = times[hi] - times[lo];
    let f = ((t - times[lo]) / dt).clamp(0., 1.);
    let a = value(lo);
    let b = value(hi);
    if interpolation == gltf::animation::Interpolation::Step {
        return a;
    }
    if cubic {
        let f2 = f * f;
        let f3 = f2 * f;
        let v = (2. * f3 - 3. * f2 + 1.) * a
            + (f3 - 2. * f2 + f) * dt * values[lo * 3 + 2]
            + (-2. * f3 + 3. * f2) * b
            + (f3 - f2) * dt * values[hi * 3];
        return if property == Property::Rotation {
            v.normalize()
        } else {
            v
        };
    }
    if property == Property::Rotation {
        Vec4::from_array(
            Quat::from_array(a.to_array())
                .slerp(Quat::from_array(b.to_array()), f)
                .to_array(),
        )
    } else {
        a.lerp(b, f)
    }
}
#[derive(Clone)]
pub struct Animation {
    pub name: String,
    pub duration: f32,
    pub tracks: Vec<Track>,
}
/// What a bound track sets on its bone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Apply {
    Rotation,
    Translation,
    /// A humanoid's pelvis, which clips move in the proportions of the rig they were made on.
    Pelvis,
    Scale,
}
/// A track of a clip and the bone it drives.
#[derive(Clone, Copy, Debug)]
pub struct Bound {
    pub track: u32,
    pub bone: u32,
    pub apply: Apply,
}
/// A clip with the tracks a body uses bound to its bones, the upper body's first.
pub struct BoundClip {
    pub clip: Arc<Animation>,
    pub tracks: Box<[Bound]>,
    /// How many of `tracks` drive the upper body: `spine_01` and everything under it.
    pub upper: usize,
}
/// Every clip of a clip set bound to one skeleton (#204), so playing a clip looks up no bone by
/// name and samples only the tracks the body uses. Humanoid clips turn every bone but move only
/// the pelvis, and never scale. Characters share one for each skeleton and clip set through
/// `ModelCache::clip_set`.
pub struct ClipSet {
    pub clips: HashMap<String, BoundClip>,
    /// A humanoid's walk, jog and sprint as this skeleton plays them (#110), measured by the first
    /// character to use the set; `None` if they don't measure.
    pub gaits: OnceLock<Option<[anim::Gait; 3]>>,
    /// How far a body on this skeleton moves, up and in toward the wall, to reach a hold from its
    /// hang (#248), measured by the first character to use the set.
    pub reach: OnceLock<Vec3>,
}
impl ClipSet {
    pub fn bind(skeleton: &[Node], clips: &Model, humanoid: bool) -> Self {
        let bones: HashMap<&str, usize> = skeleton
            .iter()
            .enumerate()
            .map(|(i, n)| (n.name.as_str(), i))
            .collect();
        let upper: Vec<bool> = (0..skeleton.len())
            .map(|i| {
                let mut at = Some(i);
                while let Some(n) = at {
                    if skeleton[n].name == "spine_01" {
                        return humanoid;
                    }
                    at = skeleton[n].parent;
                }
                false
            })
            .collect();
        let clips = clips
            .clips
            .iter()
            .map(|(name, clip)| {
                let mut tracks: Vec<Bound> = clip
                    .tracks
                    .iter()
                    .enumerate()
                    .filter_map(|(track, t)| {
                        let bone = *bones.get(t.node.as_str())?;
                        let apply = match t.property {
                            Property::Rotation => Apply::Rotation,
                            Property::Translation if !humanoid => Apply::Translation,
                            Property::Translation if t.node == "pelvis" => Apply::Pelvis,
                            Property::Scale if !humanoid => Apply::Scale,
                            _ => return None,
                        };
                        Some(Bound {
                            track: track as u32,
                            bone: bone as u32,
                            apply,
                        })
                    })
                    .collect();
                // Stable, so each half keeps the clip's own order.
                tracks.sort_by_key(|b| !upper[b.bone as usize]);
                let bound = BoundClip {
                    clip: clip.clone(),
                    upper: tracks.iter().filter(|b| upper[b.bone as usize]).count(),
                    tracks: tracks.into(),
                };
                (name.clone(), bound)
            })
            .collect();
        Self {
            clips,
            gaits: OnceLock::new(),
            reach: OnceLock::new(),
        }
    }
}
/// A clip's name, kept for the life of the program, so that choosing a clip every frame copies a
/// pointer instead of allocating a `String` (#215). There are only as many as the catalogue names.
pub fn intern(name: &str) -> &'static str {
    static NAMES: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let mut names = NAMES.lock().unwrap();
    if let Some(name) = names.get(name) {
        return name;
    }
    let name: &'static str = Box::leak(name.into());
    names.insert(name);
    name
}
static NEXT_MODEL: AtomicU64 = AtomicU64::new(1);
pub fn next_model_id() -> u64 {
    NEXT_MODEL.fetch_add(1, Ordering::Relaxed)
}
pub struct Model {
    pub id: u64,
    pub key: String,
    pub nodes: Vec<Node>,
    pub rest_world: Vec<Mat4>,
    pub skins: Vec<Skin>,
    pub primitives: Vec<Primitive>,
    pub clips: HashMap<String, Arc<Animation>>,
}
impl Model {
    pub fn load(path: &Path) -> Result<Self> {
        // Blender emits implicit-zero morph accessors, which gltf 1.4 rejects despite the
        // glTF zero-initialisation rule. Materialise only these accessors in memory, then
        // run the normal validator; never disable validation for the rest of the document.
        let source = gltf::Gltf::from_slice_without_validation(&std::fs::read(path)?)
            .with_context(|| format!("loading {}", path.display()))?;
        let mut json = serde_json::to_value(source.document.into_json())?;
        let count = json["accessors"].as_array().map_or(0, Vec::len);
        for i in 0..count {
            let a = &json["accessors"][i];
            if a.get("bufferView").is_some_and(|v| !v.is_null())
                || a.get("sparse").is_some_and(|v| !v.is_null())
            {
                continue;
            }
            let components = match a["type"].as_str() {
                Some("SCALAR") => 1,
                Some("VEC2") => 2,
                Some("VEC3") => 3,
                Some("VEC4") => 4,
                Some("MAT4") => 16,
                _ => anyhow::bail!("unsupported implicit zero accessor"),
            };
            let component = match a["componentType"].as_u64() {
                Some(5120 | 5121) => 1,
                Some(5122 | 5123) => 2,
                Some(5125 | 5126) => 4,
                _ => anyhow::bail!("invalid zero accessor component"),
            };
            let size = a["count"].as_u64().context("zero accessor without count")? as usize
                * components
                * component;
            ensure!(
                size > 0 && size <= 32 * 1024 * 1024,
                "zero accessor exceeds asset budget"
            );
            let mut encoded = "A".repeat(size.div_ceil(3) * 4);
            let padding = (3 - size % 3) % 3;
            if padding > 0 {
                encoded.replace_range(encoded.len() - padding.., &"=".repeat(padding));
            }
            let buffers = json["buffers"].as_array_mut().context("no buffers")?;
            let bi = buffers.len();
            buffers.push(serde_json::json!({"byteLength":size,"uri":format!("data:application/octet-stream;base64,{encoded}")}));
            let views = json["bufferViews"]
                .as_array_mut()
                .context("no buffer views")?;
            let vi = views.len();
            views.push(serde_json::json!({"buffer":bi,"byteLength":size}));
            json["accessors"][i]["bufferView"] = serde_json::json!(vi);
        }
        let doc = gltf::Document::from_json(serde_json::from_value(json)?)?;
        let buffers = gltf::import_buffers(&doc, path.parent(), source.blob)?;
        let mut textures = Vec::new();
        for (i, source) in doc.images().enumerate() {
            let key = match source.source() {
                gltf::image::Source::Uri { uri, .. } => {
                    let p = path.parent().unwrap().join(uri);
                    p.canonicalize().unwrap_or(p).to_string_lossy().into_owned()
                }
                _ => format!(
                    "{}#image{i}",
                    path.canonicalize()
                        .unwrap_or_else(|_| path.to_owned())
                        .display()
                ),
            };
            if let Some(compressed) = compressed_texture(path, &key)? {
                textures.push(Arc::new(Texture {
                    key,
                    width: compressed.width,
                    height: compressed.height,
                    rgba: Mutex::new(None),
                    compressed: Some(compressed),
                }));
                continue;
            }
            let img = gltf::image::Data::from_source(source.source(), path.parent(), &buffers)?;
            use gltf::image::Format::*;
            let rgba = match img.format {
                R8G8B8A8 => img.pixels,
                R8G8B8 => img
                    .pixels
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect(),
                R8 => img.pixels.iter().flat_map(|p| [*p, *p, *p, 255]).collect(),
                R8G8 => img
                    .pixels
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[0], p[0], p[1]])
                    .collect(),
                other => anyhow::bail!("{}: unsupported image format {other:?}", path.display()),
            };
            textures.push(Arc::new(Texture {
                key,
                width: img.width,
                height: img.height,
                rgba: Mutex::new(Some(rgba)),
                compressed: None,
            }));
        }
        let mut nodes: Vec<_> = doc
            .nodes()
            .map(|n| {
                let (t, r, s) = n.transform().decomposed();
                Node {
                    name: n.name().unwrap_or("").to_owned(),
                    parent: None,
                    rest: Transform {
                        translation: t.into(),
                        rotation: Quat::from_array(r),
                        scale: s.into(),
                    },
                }
            })
            .collect();
        for n in doc.nodes() {
            for c in n.children() {
                nodes[c.index()].parent = Some(n.index());
            }
        }
        let skins = doc
            .skins()
            .map(|s| {
                let joints: Vec<_> = s.joints().map(|n| n.index()).collect();
                let inverse = s
                    .reader(|b| Some(&buffers[b.index()].0))
                    .read_inverse_bind_matrices()
                    .map(|it| it.map(|m| Mat4::from_cols_array_2d(&m)).collect())
                    .unwrap_or_else(|| vec![Mat4::IDENTITY; joints.len()]);
                Skin { joints, inverse }
            })
            .collect();
        let mut primitives = Vec::new();
        for n in doc.nodes() {
            if let Some(mesh) = n.mesh() {
                for p in mesh.primitives() {
                    ensure!(
                        p.mode() == gltf::mesh::Mode::Triangles,
                        "non-triangle primitive in {}",
                        path.display()
                    );
                    let reader = p.reader(|b| Some(&buffers[b.index()].0));
                    let positions: Vec<_> = reader
                        .read_positions()
                        .context("missing POSITION")?
                        .collect();
                    let count = positions.len();
                    let normals: Vec<_> = reader
                        .read_normals()
                        .map(|v| v.collect())
                        .unwrap_or_else(|| vec![[0., 1., 0.]; count]);
                    let tangents: Vec<_> = reader
                        .read_tangents()
                        .map(|v| v.collect())
                        .unwrap_or_else(|| vec![[0.; 4]; count]);
                    let colors: Vec<_> = reader
                        .read_colors(0)
                        .map(|v| v.into_rgba_f32().collect())
                        .unwrap_or_else(|| vec![[1.; 4]; count]);
                    let uvs: Vec<_> = reader
                        .read_tex_coords(0)
                        .map(|v| v.into_f32().collect())
                        .unwrap_or_else(|| vec![[0.; 2]; count]);
                    let joints: Vec<_> = reader
                        .read_joints(0)
                        .map(|v| v.into_u16().collect())
                        .unwrap_or_else(|| vec![[0; 4]; count]);
                    let weights: Vec<_> = reader
                        .read_weights(0)
                        .map(|v| v.into_f32().collect())
                        .unwrap_or_else(|| vec![[1., 0., 0., 0.]; count]);
                    let regions: Vec<f32> = p
                        .get(&gltf::Semantic::Extras("REGION".into()))
                        .and_then(|a| {
                            gltf::accessor::Iter::<f32>::new(a, |b| Some(&buffers[b.index()].0))
                        })
                        .map(|v| v.collect())
                        .unwrap_or_else(|| vec![-1.; count]);
                    let winds: Vec<f32> = p
                        .get(&gltf::Semantic::Extras("WIND".into()))
                        .and_then(|a| {
                            gltf::accessor::Iter::<f32>::new(a, |b| Some(&buffers[b.index()].0))
                        })
                        .map(|it| it.collect())
                        .unwrap_or_else(|| vec![0.; count]);
                    let bounds =
                        Bounds::from_points(positions.iter().map(|p| Vec3::from_array(*p)));
                    let vertices = (0..count)
                        .map(|i| Vertex {
                            position: [
                                positions[i][0],
                                positions[i][1],
                                positions[i][2],
                                regions[i],
                            ],
                            normal: [normals[i][0], normals[i][1], normals[i][2], winds[i]],
                            tangent: tangents[i],
                            color: colors[i],
                            uv: [uvs[i][0], uvs[i][1], 0., 0.],
                            joints: joints[i].map(u32::from),
                            weights: weights[i],
                        })
                        .collect();
                    let indices = reader
                        .read_indices()
                        .map(|v| v.into_u32().collect())
                        .unwrap_or_else(|| (0..count as u32).collect());
                    let m = p.material();
                    let mr = m.pbr_metallic_roughness();
                    let tex = |t: gltf::Texture| textures[t.source().index()].clone();
                    let strength = m.emissive_strength().unwrap_or(1.);
                    let material = Material {
                        name: m.name().unwrap_or("").into(),
                        base: mr.base_color_factor(),
                        metallic: mr.metallic_factor(),
                        roughness: mr.roughness_factor(),
                        emissive: m.emissive_factor().map(|x| x * strength),
                        normal_scale: m.normal_texture().map_or(1., |x| x.scale()),
                        occlusion_strength: m.occlusion_texture().map_or(1., |x| x.strength()),
                        maps: [
                            mr.base_color_texture().map(|x| tex(x.texture())),
                            m.normal_texture().map(|x| tex(x.texture())),
                            mr.metallic_roughness_texture().map(|x| tex(x.texture())),
                            m.emissive_texture().map(|x| tex(x.texture())),
                            m.occlusion_texture().map(|x| tex(x.texture())),
                        ],
                        double_sided: m.double_sided(),
                        alpha_mode: match m.alpha_mode() {
                            gltf::material::AlphaMode::Opaque => 0,
                            gltf::material::AlphaMode::Mask => 1,
                            gltf::material::AlphaMode::Blend => 2,
                        },
                        alpha_cutoff: m.alpha_cutoff().unwrap_or(0.5),
                    };
                    let morphs = reader
                        .read_morph_targets()
                        .map(|(p, _, _)| {
                            p.map(|it| it.collect())
                                .unwrap_or_else(|| vec![[0.; 3]; count])
                        })
                        .collect();
                    let morph_names = mesh
                        .extras()
                        .as_ref()
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(s.get()).ok())
                        .and_then(|j| serde_json::from_value(j["targetNames"].clone()).ok())
                        .unwrap_or_default();
                    primitives.push(Primitive {
                        bounds,
                        vertices,
                        indices,
                        material,
                        node: n.index(),
                        skin: n.skin().map(|s| s.index()),
                        morphs,
                        morph_names,
                    });
                }
            }
        }
        let mut clips = HashMap::new();
        for a in doc.animations() {
            let mut tracks = Vec::new();
            let mut duration = 0f32;
            for c in a.channels() {
                let r = c.reader(|b| Some(&buffers[b.index()].0));
                let times: Vec<_> = r
                    .read_inputs()
                    .context("animation without times")?
                    .collect();
                if times.is_empty() {
                    continue;
                }
                duration = duration.max(*times.last().unwrap());
                use gltf::animation::util::ReadOutputs::*;
                let (property, values): (Property, Vec<Vec4>) =
                    match r.read_outputs().context("animation without values")? {
                        Translations(v) => (
                            Property::Translation,
                            v.map(|a| Vec3::from_array(a).extend(0.)).collect(),
                        ),
                        Rotations(v) => (
                            Property::Rotation,
                            v.into_f32().map(Vec4::from_array).collect(),
                        ),
                        Scales(v) => (
                            Property::Scale,
                            v.map(|a| Vec3::from_array(a).extend(0.)).collect(),
                        ),
                        MorphTargetWeights(_) => continue,
                    };
                tracks.push(Track::new(
                    c.target().node().name().unwrap_or("").into(),
                    property,
                    &times,
                    &values,
                    c.sampler().interpolation(),
                ));
            }
            let name = a.name().unwrap_or("unnamed").to_owned();
            clips.insert(
                name.clone(),
                Arc::new(Animation {
                    name,
                    duration,
                    tracks,
                }),
            );
        }
        let rest_world = world_matrices(&nodes, &nodes.iter().map(|n| n.rest).collect::<Vec<_>>());
        Ok(Self {
            id: next_model_id(),
            rest_world,
            key: path.to_string_lossy().into_owned(),
            nodes,
            skins,
            primitives,
            clips,
        })
    }
}
#[derive(Default)]
pub struct ModelCache {
    models: HashMap<PathBuf, Arc<Model>>,
    metadata: HashMap<PathBuf, Arc<serde_json::Value>>,
    texture_pool: HashMap<String, Arc<Texture>>,
    clip_sets: HashMap<(u64, u64, bool), Arc<ClipSet>>,
}
impl ModelCache {
    /// `clips` bound to the bones of `skeleton`, built once for each pair.
    pub fn clip_set(&mut self, skeleton: &Model, clips: &Model, humanoid: bool) -> Arc<ClipSet> {
        self.clip_sets
            .entry((skeleton.id, clips.id, humanoid))
            .or_insert_with(|| Arc::new(ClipSet::bind(&skeleton.nodes, clips, humanoid)))
            .clone()
    }
    /// Catalogue extensions are immutable for the lifetime of an asset cache.
    pub fn metadata(&mut self, path: &Path) -> Result<Arc<serde_json::Value>> {
        if let Some(value) = self.metadata.get(path) {
            return Ok(value.clone());
        }
        let value: Arc<serde_json::Value> =
            Arc::new(serde_json::from_slice(&std::fs::read(path)?)?);
        self.metadata.insert(path.to_owned(), value.clone());
        Ok(value)
    }

    pub fn texture(&mut self, path: &Path) -> Result<Arc<Texture>> {
        let key = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_owned())
            .to_string_lossy()
            .into_owned();
        if let Some(t) = self.texture_pool.get(&key) {
            return Ok(t.clone());
        }
        let t = Texture::load(path)?;
        self.texture_pool.insert(key, t.clone());
        Ok(t)
    }

    pub fn get_lod(&mut self, path: &Path, lod: usize) -> Result<Arc<Model>> {
        if lod == 0 {
            return self.get(path);
        }
        let rel = assets::lod_path(&path.to_string_lossy(), lod);
        let candidate = Path::new(&rel);
        if candidate.exists() {
            self.get(candidate)
        } else {
            self.get(path)
        }
    }

    pub fn get(&mut self, path: &Path) -> Result<Arc<Model>> {
        if let Some(m) = self.models.get(path) {
            return Ok(m.clone());
        }
        let mut decoded = Model::load(path)?;
        for p in &mut decoded.primitives {
            for map in p.material.maps.iter_mut().flatten() {
                if let Some(shared) = self.texture_pool.get(&map.key) {
                    *map = shared.clone();
                } else {
                    self.texture_pool.insert(map.key.clone(), map.clone());
                }
            }
        }
        let m = Arc::new(decoded);
        self.models.insert(path.to_owned(), m.clone());
        Ok(m)
    }
}

/// Nodes in exports are often children-first. Resolve parents explicitly, never by file order.
pub fn world_matrices(nodes: &[Node], local: &[Transform]) -> Vec<Mat4> {
    fn visit(i: usize, nodes: &[Node], local: &[Transform], out: &mut [Mat4], done: &mut [bool]) {
        if done[i] {
            return;
        }
        let parent = if let Some(p) = nodes[i].parent {
            visit(p, nodes, local, out, done);
            out[p]
        } else {
            Mat4::IDENTITY
        };
        out[i] = parent * local[i].matrix();
        done[i] = true;
    }
    let mut out = vec![Mat4::IDENTITY; nodes.len()];
    let mut done = vec![false; nodes.len()];
    for i in 0..nodes.len() {
        visit(i, nodes, local, &mut out, &mut done)
    }
    out
}

fn compressed_texture(model: &Path, key: &str) -> Result<Option<crate::ktx::Ktx>> {
    if !key.contains("#image") {
        let sidecar = Path::new(key).with_extension("ktx2");
        if sidecar.exists() {
            return Ok(Some(crate::ktx::Ktx::load(&sidecar)?));
        }
    }
    let Some(root) = model.ancestors().find(|p| p.join("catalog").is_dir()) else {
        return Ok(None);
    };
    let root = root.canonicalize()?;
    let manifest = root.join("catalog/textures.json");
    if !manifest.exists() {
        return Ok(None);
    }
    let table: serde_json::Value = serde_json::from_slice(&std::fs::read(manifest)?)?;
    let root_prefix = format!("{}/", root.display());
    let relative = key.strip_prefix(&root_prefix).unwrap_or(key);
    if let Some(file) = table["textures"][relative].as_str() {
        return Ok(Some(crate::ktx::Ktx::load(&root.join(file))?));
    }
    Ok(None)
}
impl Texture {
    pub fn restore_pixels(&self) -> Result<()> {
        let mut rgba = self.rgba.lock().unwrap();
        if rgba.is_some() {
            return Ok(());
        }
        if let Some((file, index)) = self.key.rsplit_once("#image") {
            let g = gltf::Gltf::from_slice_without_validation(&std::fs::read(file)?)?;
            let buffers = gltf::import_buffers(&g.document, Path::new(file).parent(), g.blob)?;
            let source = g
                .document
                .images()
                .nth(index.parse()?)
                .context("missing embedded image")?;
            let image = gltf::image::Data::from_source(
                source.source(),
                Path::new(file).parent(),
                &buffers,
            )?;
            *rgba = Some(match image.format {
                gltf::image::Format::R8G8B8A8 => image.pixels,
                gltf::image::Format::R8G8B8 => image
                    .pixels
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[1], p[2], 255])
                    .collect(),
                _ => anyhow::bail!("unsupported embedded texture reload"),
            });
        } else {
            *rgba = Some(image::open(&self.key)?.into_rgba8().into_raw());
        }
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Arc<Self>> {
        let key = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_owned())
            .to_string_lossy()
            .into_owned();
        if let Some(compressed) = compressed_texture(path, &key)? {
            return Ok(Arc::new(Self {
                key,
                width: compressed.width,
                height: compressed.height,
                rgba: Mutex::new(None),
                compressed: Some(compressed),
            }));
        }
        let img = image::open(path)?.into_rgba8();
        Ok(Arc::new(Self {
            key,
            width: img.width(),
            height: img.height(),
            rgba: Mutex::new(Some(img.into_raw())),
            compressed: None,
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use gltf::animation::Interpolation::{Linear, Step};
    fn x(v: f32) -> Vec4 {
        Vec4::new(v, 0., 0., 0.)
    }
    fn turned(degrees: f32) -> Vec4 {
        Vec4::from(Quat::from_rotation_y(degrees.to_radians()))
    }
    #[test]
    fn even_keys_are_indexed_and_blended() {
        let times = [0., 1. / 30., 2. / 30., 3. / 30.];
        let at = [0., 10., 30., 60.].map(x);
        let track = Track::new("a".into(), Property::Translation, &times, &at, Linear);
        for (t, v) in times.iter().zip(&at) {
            assert!(track.sample(*t).abs_diff_eq(*v, 1e-4));
        }
        assert!(track.sample(1.5 / 30.).abs_diff_eq(x(20.), 1e-3));
        // Before the first key and after the last, the track holds.
        assert_eq!(track.sample(-1.), at[0]);
        assert_eq!(track.sample(5.), at[3]);
        // The last step is a fast blow's 100° swing.
        let turns = [0., 12., 30., 130.].map(turned);
        let track = Track::new("a".into(), Property::Rotation, &times, &turns, Linear);
        for (i, f) in [(0, 0.25), (1, 0.5), (2, 0.2), (2, 0.9)] {
            let [a, b] = [turns[i], turns[i + 1]].map(Quat::from_vec4);
            let q = Quat::from_vec4(track.sample((i as f32 + f) / 30.));
            assert!(q.angle_between(a.slerp(b, f)) < 0.05f32.to_radians());
            assert!(q.is_normalized());
        }
    }
    #[test]
    fn a_track_that_never_changes_keeps_one_key() {
        let held = Vec4::new(1., 2., 3., 0.);
        let track = Track::new("a".into(), Property::Scale, &[0., 2.], &[held; 2], Linear);
        assert_eq!(track.values.len(), 1);
        assert_eq!(track.sample(0.7), held);
    }
    #[test]
    fn uneven_keys_are_resampled_at_30_a_second() {
        let times = [0., 0.1, 0.5, 1.];
        let at = [0., 1., 5., 10.].map(x);
        let track = Track::new("a".into(), Property::Translation, &times, &at, Linear);
        assert!(track.rate >= 30.);
        for t in [0., 0.05, 0.1, 0.33, 0.5, 0.8, 1.] {
            let want = sample_keys(&times, &at, Linear, Property::Translation, t);
            assert!(track.sample(t).abs_diff_eq(want, 1e-3), "{t}");
        }
    }
    #[test]
    fn step_keys_hold_until_the_next() {
        let at = [0., 1., 2.].map(x);
        let track = Track::new("a".into(), Property::Translation, &[0., 0.5, 1.], &at, Step);
        assert_eq!(track.sample(0.49), at[0]);
        assert_eq!(track.sample(0.51), at[1]);
        assert_eq!(track.sample(1.), at[2]);
    }
}
