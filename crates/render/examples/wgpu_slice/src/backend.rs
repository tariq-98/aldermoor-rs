use super::*;
use bytemuck::{Pod, Zeroable};
use std::{collections::HashMap, num::NonZeroU64};
use wgpu::util::DeviceExt;

const SHADOW_SIZE: u32 = 1024;
const HDR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Frame {
    vp: [[f32; 4]; 4],
    previous_vp: [[f32; 4]; 4],
    inverse_vp: [[f32; 4]; 4],
    jitter: [f32; 4],
    shadows: [[[f32; 4]; 4]; 3],
    eye: [f32; 4],
    sun: [f32; 4],
    sun_colour: [f32; 4],
    ambient: [f32; 4],
    sky: [f32; 4],
    fog: [f32; 4],
    grade: [[f32; 4]; 3],
    params: [f32; 4],
    clock: [f32; 4],
    viewport: [f32; 4],
    splits: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    base: [f32; 4],
    emission: [f32; 4],
    params: [f32; 4],
    flags: [u32; 4],
}
struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    bind: wgpu::BindGroup,
    two_sided: bool,
    blended: bool,
}
struct Batch {
    mesh: usize,
    start: u32,
    count: u32,
    distance: f32,
}
struct Prepared {
    mesh: usize,
    data: Instance,
    bounds: visibility::Bounds,
    scene: bool,
    shadows: [bool; 3],
}
struct Texture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}
fn buffer(
    device: &wgpu::Device,
    label: &str,
    bytes: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytes,
        usage,
    })
}
fn texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    layers: u32,
    mips: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    array: bool,
) -> Texture {
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: layers,
        },
        mip_level_count: mips,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = t.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(if array {
            wgpu::TextureViewDimension::D2Array
        } else {
            wgpu::TextureViewDimension::D2
        }),
        ..Default::default()
    });
    Texture { texture: t, view }
}
fn uniform(binding: u32, stages: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: stages,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
fn tex_layout(binding: u32, array: bool, depth: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: if depth {
                wgpu::TextureSampleType::Depth
            } else {
                wgpu::TextureSampleType::Float { filterable: true }
            },
            view_dimension: if array {
                wgpu::TextureViewDimension::D2Array
            } else {
                wgpu::TextureViewDimension::D2
            },
            multisampled: false,
        },
        count: None,
    }
}
fn sampler_layout(binding: u32, comparison: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(if comparison {
            wgpu::SamplerBindingType::Comparison
        } else {
            wgpu::SamplerBindingType::Filtering
        }),
        count: None,
    }
}
fn bind<'a>(binding: u32, resource: wgpu::BindingResource<'a>) -> wgpu::BindGroupEntry<'a> {
    wgpu::BindGroupEntry { binding, resource }
}
fn view_bind(binding: u32, t: &Texture) -> wgpu::BindGroupEntry<'_> {
    bind(binding, wgpu::BindingResource::TextureView(&t.view))
}
fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    vertex: &str,
    fragment: &str,
    formats: &[wgpu::TextureFormat],
    geometry: bool,
    depth_write: Option<bool>,
    cull: bool,
    blend: bool,
) -> wgpu::RenderPipeline {
    let attrs =
        wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4,3=>Float32x4,4=>Float32x4];
    let vertices = [Some(wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<model::Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &attrs,
    })];
    let targets: Vec<_> = formats
        .iter()
        .enumerate()
        .map(|(i, format)| {
            Some(wgpu::ColorTargetState {
                format: *format,
                blend: if blend && i == 0 {
                    Some(wgpu::BlendState::ALPHA_BLENDING)
                } else {
                    None
                },
                write_mask: wgpu::ColorWrites::ALL,
            })
        })
        .collect();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: if geometry { &vertices } else { &[] },
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: if cull { Some(wgpu::Face::Back) } else { None },
            ..Default::default()
        },
        depth_stencil: depth_write.map(|write| wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(write),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: if vertex == "shadow_vertex" {
                wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: 1.25,
                    clamp: 0.01,
                }
            } else {
                Default::default()
            },
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}
/// `--checks trusted` drops naga's bounds checks and loop guards (#317). Every shader here is ours.
fn module(d: &wgpu::Device, trusted: bool, label: &str, wgsl: &'static str) -> wgpu::ShaderModule {
    let desc = wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    };
    if trusted {
        // SAFETY: the WGSL is part of this crate, indexes stay in range and loops are bounded.
        unsafe { d.create_shader_module_trusted(desc, wgpu::ShaderRuntimeChecks::unchecked()) }
    } else {
        d.create_shader_module(desc)
    }
}
/// The same instanced draws on a pass or a bundle encoder; returns the draw count.
macro_rules! draw_batches {
    ($p:expr, $meshes:expr, $list:expr, |$m:ident| $pipe:expr) => {{
        let mut n = 0;
        for b in $list {
            let $m = &$meshes[b.mesh];
            $p.set_pipeline($pipe);
            $p.set_bind_group(1, &$m.bind, &[]);
            $p.set_vertex_buffer(0, $m.vertices.slice(..));
            $p.set_index_buffer($m.indices.slice(..), wgpu::IndexFormat::Uint32);
            $p.draw_indexed(0..$m.count, 0, b.start..b.start + b.count);
            n += 1;
        }
        n
    }};
}
/// A recorded bundle and the batch list it replays; re-recorded when culling changes the list.
struct Cached {
    key: Vec<(usize, u32, u32)>,
    bundle: wgpu::RenderBundle,
    draws: usize,
}
fn batch_key<'a>(list: impl Iterator<Item = &'a Batch>) -> Vec<(usize, u32, u32)> {
    list.map(|b| (b.mesh, b.start, b.count)).collect()
}
fn attachment(
    view: &wgpu::TextureView,
    clear: Option<wgpu::Color>,
) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        resolve_target: None,
        depth_slice: None,
        ops: wgpu::Operations {
            load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
            store: wgpu::StoreOp::Store,
        },
    })
}
fn depth_attachment(
    view: &wgpu::TextureView,
    clear: bool,
) -> wgpu::RenderPassDepthStencilAttachment<'_> {
    wgpu::RenderPassDepthStencilAttachment {
        view,
        depth_ops: Some(wgpu::Operations {
            load: if clear {
                wgpu::LoadOp::Clear(1.)
            } else {
                wgpu::LoadOp::Load
            },
            store: wgpu::StoreOp::Store,
        }),
        stencil_ops: None,
    }
}
struct Utilities {
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
}
impl Utilities {
    fn new(d: &wgpu::Device) -> Self {
        let layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("utility"),
            entries: &[
                tex_layout(0, false, false),
                sampler_layout(1, false),
                uniform(2, wgpu::ShaderStages::FRAGMENT),
            ],
        });
        let pipeline_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("utility"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("utility"),
            source: wgpu::ShaderSource::Wgsl(include_str!("utility.wgsl").into()),
        });
        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("environment"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            layout,
            pipeline_layout,
            shader,
            sampler,
        }
    }
    fn draw(
        &self,
        d: &wgpu::Device,
        e: &mut wgpu::CommandEncoder,
        src: &wgpu::TextureView,
        dst: &wgpu::TextureView,
        pipe: &wgpu::RenderPipeline,
        rough: f32,
    ) {
        let u = buffer(
            d,
            "roughness",
            bytemuck::cast_slice(&[rough, 0., 0., 0.]),
            wgpu::BufferUsages::UNIFORM,
        );
        let b = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("utility"),
            layout: &self.layout,
            entries: &[
                bind(0, wgpu::BindingResource::TextureView(src)),
                bind(1, wgpu::BindingResource::Sampler(&self.sampler)),
                bind(2, u.as_entire_binding()),
            ],
        });
        let colors = [attachment(dst, Some(wgpu::Color::TRANSPARENT))];
        let mut p = e.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("texture preparation"),
            color_attachments: &colors,
            ..Default::default()
        });
        p.set_pipeline(pipe);
        p.set_bind_group(0, &b, &[]);
        p.draw(0..3, 0..1);
    }
    fn upload(
        &self,
        d: &wgpu::Device,
        q: &wgpu::Queue,
        images: &[Arc<model::Texture>],
        srgb: bool,
        array: bool,
    ) -> Result<Texture> {
        let first = &images[0];
        let width = first.width;
        let height = first.height;
        let mips = 1 + width.max(height).ilog2();
        let format = if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };
        // The supplied catalogue currently has uncompressed images. Fail explicitly if that
        // changes: silently decoding ASTC only for one backend invalidates the performance gate.
        ensure!(
            images.iter().all(|t| t.compressed.is_none()),
            "ASTC inputs need a separately reviewed cross-backend upload path"
        );
        ensure!(
            images
                .iter()
                .all(|t| t.width == width && t.height == height),
            "array texture dimensions differ"
        );
        let t = texture(
            d,
            &first.key,
            width,
            height,
            images.len() as u32,
            mips,
            format,
            wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            array,
        );
        let pipe = pipeline(
            d,
            &self.shader,
            &self.pipeline_layout,
            "fullscreen",
            "mip",
            &[format],
            false,
            None,
            false,
            false,
        );
        let mut e = d.create_command_encoder(&Default::default());
        for (layer, img) in images.iter().enumerate() {
            img.restore_pixels()?;
            let mut pixels = img.rgba.lock().unwrap();
            q.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &t.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                pixels.as_ref().context("missing pixels")?,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            pixels.take();
            drop(pixels);
            for level in 1..mips {
                let view = |mip| {
                    t.texture.create_view(&wgpu::TextureViewDescriptor {
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        base_mip_level: mip,
                        mip_level_count: Some(1),
                        base_array_layer: layer as u32,
                        array_layer_count: Some(1),
                        ..Default::default()
                    })
                };
                self.draw(d, &mut e, &view(level - 1), &view(level), &pipe, 0.);
            }
        }
        q.submit([e.finish()]);
        Ok(t)
    }
}
fn fallback(rgba: [u8; 4]) -> Arc<model::Texture> {
    Arc::new(model::Texture {
        key: format!("fallback:{rgba:?}"),
        width: 1,
        height: 1,
        rgba: std::sync::Mutex::new(Some(rgba.to_vec())),
        compressed: None,
    })
}
fn halton(mut i: usize, base: usize) -> f32 {
    let mut f = 1.;
    let mut r = 0.;
    while i > 0 {
        f /= base as f32;
        r += f * (i % base) as f32;
        i /= base;
    }
    r
}

pub fn run(
    o: &Options,
    camera: Camera,
    draws: &[character::Draw],
) -> Result<(String, Vec<Sample>)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: if cfg!(target_os = "macos") {
            wgpu::Backends::METAL
        } else {
            wgpu::Backends::VULKAN
        },
        flags: if o.validate {
            wgpu::InstanceFlags::VALIDATION | wgpu::InstanceFlags::DEBUG
        } else {
            wgpu::InstanceFlags::empty()
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let info = adapter.get_info();
    ensure!(
        adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY),
        "adapter lacks pass timestamps"
    );
    let (d, q) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("renderer spike"),
        required_features: wgpu::Features::TIMESTAMP_QUERY,
        required_limits: adapter.limits(),
        ..Default::default()
    }))?;
    let utility = Utilities::new(&d);
    let look = settings::LookSettings::load(&o.assets.join("catalog/look.json"))?;
    let inputs = surfaces::SurfaceAssets::load(&o.assets.join("catalog/surfaces.json"))?;
    let mut su = surfaces::Uniform::default();
    let t = inputs.terrain.context("spike requires supplied terrain")?;
    let mut colours = Vec::new();
    let mut normals = Vec::new();
    for (i, l) in t.layers.iter().enumerate() {
        colours.push(model::Texture::load(&o.assets.join(&l.colour))?);
        normals.push(model::Texture::load(&o.assets.join(&l.normal))?);
        su.scales[i] = 1. / l.uv_metres;
        su.roughness[i] = l.roughness;
    }
    su.bounds = t.bounds;
    su.flags[0] = t.layers.len() as u32;
    let terrain_colour = utility.upload(&d, &q, &colours, true, true)?;
    let terrain_normal = utility.upload(&d, &q, &normals, false, true)?;
    let weights = utility.upload(
        &d,
        &q,
        &[model::Texture::load(&o.assets.join(t.weights))?],
        false,
        false,
    )?;
    let w = inputs.water.context("spike requires supplied water")?;
    let water = utility.upload(
        &d,
        &q,
        &[model::Texture::load(&o.assets.join(w.normal))?],
        false,
        false,
    )?;
    su.water_deep = [
        w.deep_colour[0],
        w.deep_colour[1],
        w.deep_colour[2],
        w.opacity,
    ];
    su.water_shallow = [
        w.shallow_colour[0],
        w.shallow_colour[1],
        w.shallow_colour[2],
        w.normal_strength,
    ];
    su.water_absorption = [
        w.absorption[0],
        w.absorption[1],
        w.absorption[2],
        w.reflection_strength,
    ];
    su.water_flow = [1. / w.uv_metres, w.flow[0], w.flow[1], w.roughness];
    su.sky[2] = w.ior;
    su.flags[1] = 1;
    let s = inputs.sky.context("spike requires supplied sky")?;
    let sky = utility.upload(
        &d,
        &q,
        &[model::Texture::load(&o.assets.join(s.texture))?],
        true,
        false,
    )?;
    su.sky[0] = s.rotation_radians / std::f32::consts::TAU;
    su.sky[1] = s.intensity;
    su.flags[2] = 1;
    let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let reflection = texture(&d, "sky reflection", 256, 128, 1, 9, HDR, rt, false);
    let prefilter = pipeline(
        &d,
        &utility.shader,
        &utility.pipeline_layout,
        "fullscreen",
        "prefilter",
        &[HDR],
        false,
        None,
        false,
        false,
    );
    let mut e = d.create_command_encoder(&Default::default());
    for level in 0..9 {
        let v = reflection
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            });
        utility.draw(&d, &mut e, &sky.view, &v, &prefilter, level as f32 / 8.);
    }
    q.submit([e.finish()]);
    let width = (o.width as f32 * o.scale).round() as u32;
    let height = (o.height as f32 * o.scale).round() as u32;
    let hdr = texture(&d, "scene", width, height, 1, 1, HDR, rt, false);
    let motion = texture(&d, "motion", width, height, 1, 1, HDR, rt, false);
    let depth = texture(&d, "depth", width, height, 1, 1, DEPTH, rt, false);
    let history = [
        texture(&d, "history 0", width, height, 1, 1, HDR, rt, false),
        texture(&d, "history 1", width, height, 1, 1, HDR, rt, false),
    ];
    let output = texture(
        &d,
        "output",
        o.width,
        o.height,
        1,
        1,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        rt | wgpu::TextureUsages::COPY_SRC,
        false,
    );
    let shadow = texture(
        &d,
        "shadow cascades",
        SHADOW_SIZE,
        SHADOW_SIZE,
        3,
        1,
        DEPTH,
        rt,
        true,
    );
    let dummy_shadow = texture(
        &d,
        "unused shadow input during shadow rendering",
        1,
        1,
        3,
        1,
        DEPTH,
        rt,
        true,
    );
    let shadow_views: Vec<_> = (0..3)
        .map(|layer| {
            shadow.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: layer,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let frame_buffer = buffer(
        &d,
        "frame",
        bytemuck::bytes_of(&Frame::zeroed()),
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    );
    let surface_buffer = buffer(
        &d,
        "supplied surfaces",
        bytemuck::bytes_of(&su),
        wgpu::BufferUsages::UNIFORM,
    );
    let instance_buffer = d.create_buffer(&wgpu::BufferDescriptor {
        label: Some("packed instances"),
        size: (draws.len().max(1) * 4 * std::mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let comparison = d.create_sampler(&wgpu::SamplerDescriptor {
        compare: Some(wgpu::CompareFunction::LessEqual),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let repeat = d.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 4,
        ..Default::default()
    });
    let linear = d.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
    let global_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene globals"),
        entries: &[
            uniform(0, vf),
            uniform(1, vf),
            tex_layout(2, true, true),
            sampler_layout(3, true),
            tex_layout(4, true, false),
            tex_layout(5, true, false),
            tex_layout(6, false, false),
            tex_layout(7, false, false),
            tex_layout(8, false, false),
            tex_layout(9, false, false),
            sampler_layout(10, false),
            sampler_layout(11, false),
            sampler_layout(12, false),
        ],
    });
    let globals = |shadow: &Texture| {
        d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene globals"),
            layout: &global_layout,
            entries: &[
                bind(0, frame_buffer.as_entire_binding()),
                bind(1, surface_buffer.as_entire_binding()),
                view_bind(2, shadow),
                bind(3, wgpu::BindingResource::Sampler(&comparison)),
                view_bind(4, &terrain_colour),
                view_bind(5, &terrain_normal),
                view_bind(6, &weights),
                view_bind(7, &water),
                view_bind(8, &sky),
                view_bind(9, &reflection),
                bind(10, wgpu::BindingResource::Sampler(&repeat)),
                bind(11, wgpu::BindingResource::Sampler(&linear)),
                bind(12, wgpu::BindingResource::Sampler(&utility.sampler)),
            ],
        })
    };
    let global = globals(&shadow);
    let shadow_global = globals(&dummy_shadow);
    let material_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("mesh material"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: vf,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<Instance>() as u64),
                },
                count: None,
            },
            tex_layout(1, false, false),
            tex_layout(2, false, false),
            tex_layout(3, false, false),
            tex_layout(4, false, false),
            tex_layout(5, false, false),
        ],
    });
    let cascade_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("cascade"),
        entries: &[uniform(0, wgpu::ShaderStages::VERTEX)],
    });
    let cascades: Vec<_> = (0..3u32)
        .map(|i| {
            let b = buffer(
                &d,
                "cascade index",
                bytemuck::cast_slice(&[i, 0, 0, 0]),
                wgpu::BufferUsages::UNIFORM,
            );
            d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cascade index"),
                layout: &cascade_layout,
                entries: &[bind(0, b.as_entire_binding())],
            })
        })
        .collect();
    let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("scene layout"),
        bind_group_layouts: &[
            Some(&global_layout),
            Some(&material_layout),
            Some(&cascade_layout),
        ],
        immediate_size: 0,
    });
    let trusted = o.checks == "trusted";
    let shader = module(&d, trusted, "scene port", include_str!("scene.wgsl"));
    let shadow_pipelines = [false, true].map(|cull| {
        pipeline(
            &d,
            &shader,
            &layout,
            "shadow_vertex",
            "shadow_fragment",
            &[],
            true,
            Some(true),
            cull,
            false,
        )
    });
    let scene_pipelines = [false, true].map(|blended| {
        [false, true].map(|cull| {
            pipeline(
                &d,
                &shader,
                &layout,
                "scene_vertex",
                "scene_fragment",
                &[HDR, HDR],
                true,
                Some(!blended),
                cull,
                blended,
            )
        })
    });
    let sky_pipeline = pipeline(
        &d,
        &shader,
        &layout,
        "fullscreen",
        "sky_fragment",
        &[HDR, HDR],
        false,
        Some(false),
        false,
        false,
    );
    let post_global_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("post frame"),
        entries: &[uniform(0, vf)],
    });
    let post_global = d.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("post frame"),
        layout: &post_global_layout,
        entries: &[bind(0, frame_buffer.as_entire_binding())],
    });
    let post_texture_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("post inputs"),
        entries: &[
            tex_layout(0, false, false),
            tex_layout(1, false, false),
            tex_layout(2, false, true),
            tex_layout(3, false, false),
            sampler_layout(4, false),
        ],
    });
    let post_layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("post layout"),
        bind_group_layouts: &[Some(&post_global_layout), Some(&post_texture_layout)],
        immediate_size: 0,
    });
    let post = module(&d, trusted, "post port", include_str!("post.wgsl"));
    let taa = pipeline(
        &d,
        &post,
        &post_layout,
        "fullscreen",
        "temporal_resolve",
        &[HDR],
        false,
        None,
        false,
        false,
    );
    let composite = pipeline(
        &d,
        &post,
        &post_layout,
        "fullscreen",
        "composite",
        &[wgpu::TextureFormat::Bgra8UnormSrgb],
        false,
        None,
        false,
        false,
    );
    let post_bind = |colour: &Texture, old: &Texture| {
        d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post inputs"),
            layout: &post_texture_layout,
            entries: &[
                view_bind(0, colour),
                view_bind(1, &motion),
                view_bind(2, &depth),
                view_bind(3, old),
                bind(4, wgpu::BindingResource::Sampler(&linear)),
            ],
        })
    };
    let taa_binds = [post_bind(&hdr, &history[1]), post_bind(&hdr, &history[0])];
    let composite_binds = [
        post_bind(&history[0], &history[1]),
        post_bind(&history[1], &history[0]),
    ];
    // Same as the Metal renderer's defaults: glTF multiplies the emissive factor by the map, so a
    // material with a factor and no map needs white here, not black.
    let fallback_maps = [
        [255; 4],
        [128, 128, 255, 255],
        [255; 4],
        [255; 4],
        [255; 4],
    ]
    .map(fallback);
    let mut textures: HashMap<(String, bool), Texture> = HashMap::new();
    let mut meshes = Vec::new();
    let mut mesh_ids = HashMap::new();
    let sun = Vec3::from_array(look.sun_direction).normalize();
    let aspect = o.width as f32 / o.height as f32;
    let unjittered = camera.matrix(aspect);
    let shadow_matrices = visibility::stable_cascades(
        camera.eye,
        camera.target,
        camera.fov,
        aspect,
        sun,
        SHADOW_SIZE,
    );
    let frustum = visibility::Frustum::new(unjittered);
    let shadow_frustums = shadow_matrices.map(visibility::Frustum::new);
    // Load only meshes used by at least one pass, matching Metal's upload-after-cull policy.
    for draw in draws {
        let p = &draw.model.primitives[draw.primitive];
        let bounds = p
            .bounds
            .transformed(draw.transform)
            .padded(0.2 + look.wind_strength * 0.12);
        let swept = visibility::Bounds {
            center: bounds.center - sun * 50.,
            half: bounds.half + sun.abs() * 50.,
        };
        if !frustum.intersects(bounds)
            && !shadow_frustums
                .iter()
                .any(|v| v.intersects(bounds) && frustum.intersects(swept))
        {
            continue;
        }
        let key = (draw.model.id, draw.primitive);
        if mesh_ids.contains_key(&key) {
            continue;
        }
        ensure!(
            draw.palette.is_empty() && draw.dye.is_none() && draw.base_map.is_none(),
            "spike supports rigid scenery only"
        );
        let m = &p.material;
        let keys: Vec<_> = (0..5)
            .map(|i| {
                let image = m.maps[i].as_ref().unwrap_or(&fallback_maps[i]);
                (image.key.clone(), i == 0 || i == 3)
            })
            .collect();
        for (i, key) in keys.iter().enumerate() {
            if !textures.contains_key(key) {
                let image = m.maps[i].as_ref().unwrap_or(&fallback_maps[i]);
                textures.insert(
                    key.clone(),
                    utility.upload(&d, &q, &[image.clone()], key.1, false)?,
                );
            }
        }
        let mut entries = vec![bind(0, instance_buffer.as_entire_binding())];
        for (i, key) in keys.iter().enumerate() {
            entries.push(view_bind(i as u32 + 1, &textures[key]));
        }
        let material = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&m.name),
            layout: &material_layout,
            entries: &entries,
        });
        mesh_ids.insert(key, meshes.len());
        meshes.push(Mesh {
            vertices: buffer(
                &d,
                "vertices",
                bytemuck::cast_slice(&p.vertices),
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                &d,
                "indices",
                bytemuck::cast_slice(&p.indices),
                wgpu::BufferUsages::INDEX,
            ),
            count: p.indices.len() as u32,
            bind: material,
            two_sided: m.double_sided || draw.model.key.starts_with("world:"),
            blended: m.alpha_mode == 2 || m.name == "__water_placeholder",
        });
    }
    ensure!(!meshes.is_empty(), "empty scene");
    let queries = d.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("pass times"),
        ty: wgpu::QueryType::Timestamp,
        count: 2 * PASSES.len() as u32,
    });
    let resolve = d.create_buffer(&wgpu::BufferDescriptor {
        label: Some("query resolve"),
        size: 16 * PASSES.len() as u64,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = d.create_buffer(&wgpu::BufferDescriptor {
        label: Some("query readback"),
        size: 16 * PASSES.len() as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    d.poll(wgpu::PollType::wait_indefinitely())?;
    let mut samples = Vec::new();
    // Shadow cascades 0-2, then opaque and water.
    let mut cache: [Option<Cached>; 5] = Default::default();
    let merged = o.passes == "merged";
    for frame in 0..o.frames {
        let started = Instant::now();
        let jitter = Vec3::new(
            (halton(frame % 16 + 1, 2) - 0.5) * 2. / width as f32,
            (halton(frame % 16 + 1, 3) - 0.5) * 2. / height as f32,
            0.,
        );
        let f = Frame {
            vp: unjittered.to_cols_array_2d(),
            previous_vp: unjittered.to_cols_array_2d(),
            inverse_vp: (Mat4::from_translation(jitter) * unjittered)
                .inverse()
                .to_cols_array_2d(),
            jitter: [jitter.x, jitter.y, jitter.x * 0.5, -jitter.y * 0.5],
            shadows: shadow_matrices.map(|m| m.to_cols_array_2d()),
            eye: camera.eye.extend(1.).to_array(),
            sun: sun.extend(look.sun_intensity).to_array(),
            sun_colour: Vec3::from_array(look.sun_colour).extend(1.).to_array(),
            ambient: Vec3::from_array(look.ambient).extend(1.).to_array(),
            sky: Vec3::from_array(look.sky).extend(1.).to_array(),
            fog: Vec3::from_array(look.fog_colour)
                .extend(look.fog_density)
                .to_array(),
            grade: look.grade.map(|r| [r[0], r[1], r[2], 0.]),
            params: [
                look.exposure,
                (look.tone_map == settings::ToneMap::Aces) as u32 as f32,
                0.,
                0.,
            ],
            clock: [
                frame as f32 / 60.,
                frame.saturating_sub(1) as f32 / 60.,
                look.wind_strength,
                (frame > 0) as u32 as f32,
            ],
            viewport: [
                width as f32,
                height as f32,
                1. / width as f32,
                1. / height as f32,
            ],
            splits: [15., 50., 150., 1. / SHADOW_SIZE as f32],
        };
        q.write_buffer(&frame_buffer, 0, bytemuck::bytes_of(&f));
        let mut prepared = Vec::new();
        for draw in draws {
            let p = &draw.model.primitives[draw.primitive];
            let m = &p.material;
            let bounds = p
                .bounds
                .transformed(draw.transform)
                .padded(0.2 + look.wind_strength * 0.12);
            let swept = visibility::Bounds {
                center: bounds.center - sun * 50.,
                half: bounds.half + sun.abs() * 50.,
            };
            let scene = frustum.intersects(bounds);
            let shadows = shadow_frustums
                .each_ref()
                .map(|v| v.intersects(bounds) && frustum.intersects(swept));
            if !scene && !shadows.iter().any(|b| *b) {
                continue;
            }
            let mesh = mesh_ids[&(draw.model.id, draw.primitive)];
            prepared.push(Prepared {
                mesh,
                bounds,
                scene,
                shadows,
                data: Instance {
                    model: draw.transform.to_cols_array_2d(),
                    base: draw.base,
                    emission: [m.emissive[0], m.emissive[1], m.emissive[2], m.roughness],
                    params: [
                        m.metallic,
                        m.normal_scale,
                        m.occlusion_strength,
                        m.alpha_cutoff,
                    ],
                    flags: [
                        m.alpha_mode,
                        match m.name.as_str() {
                            "__terrain_placeholder" => 1,
                            "__water_placeholder" => 2,
                            _ => 0,
                        },
                        m.double_sided as u32,
                        0,
                    ],
                },
            });
        }
        let mut packed = Vec::new();
        let mut batches: [Vec<Batch>; 4] = Default::default();
        for (pass, list) in batches.iter_mut().enumerate() {
            let mut groups: Vec<Vec<&Prepared>> = Vec::new();
            let mut ids = HashMap::new();
            for p in &prepared {
                let mesh = &meshes[p.mesh];
                if if pass < 3 {
                    !p.shadows[pass] || mesh.blended
                } else {
                    !p.scene
                } {
                    continue;
                }
                if mesh.blended {
                    groups.push(vec![p]);
                } else {
                    let i = *ids.entry(p.mesh).or_insert_with(|| {
                        groups.push(Vec::new());
                        groups.len() - 1
                    });
                    groups[i].push(p);
                }
            }
            for group in groups {
                let start = packed.len() as u32;
                for p in &group {
                    packed.push(p.data);
                }
                list.push(Batch {
                    mesh: group[0].mesh,
                    start,
                    count: group.len() as u32,
                    distance: group
                        .iter()
                        .map(|p| p.bounds.center.distance_squared(camera.eye))
                        .fold(f32::INFINITY, f32::min),
                });
            }
            list.sort_by(|a, b| {
                meshes[a.mesh]
                    .blended
                    .cmp(&meshes[b.mesh].blended)
                    .then_with(|| {
                        if meshes[a.mesh].blended {
                            b.distance.total_cmp(&a.distance)
                        } else {
                            a.distance.total_cmp(&b.distance)
                        }
                    })
            });
        }
        q.write_buffer(&instance_buffer, 0, bytemuck::cast_slice(&packed));
        let mut e = d.create_command_encoder(&Default::default());
        let mut calls = 0;
        let stamp = |index: usize| {
            Some(wgpu::RenderPassTimestampWrites {
                query_set: &queries,
                beginning_of_pass_write_index: Some(index as u32 * 2),
                end_of_pass_write_index: Some(index as u32 * 2 + 1),
            })
        };
        let mut timed = [false; PASSES.len()];
        if o.bundles {
            let scene: [Vec<&Batch>; 2] = [false, true].map(|blended| {
                batches[3]
                    .iter()
                    .filter(|b| meshes[b.mesh].blended == blended)
                    .collect()
            });
            for slot in 0..5 {
                let list: Vec<&Batch> = if slot < 3 {
                    batches[slot].iter().collect()
                } else {
                    scene[slot - 3].clone()
                };
                let key = batch_key(list.iter().copied());
                if cache[slot].as_ref().is_some_and(|c| c.key == key) {
                    continue;
                }
                let shadow = slot < 3;
                let colours = if shadow {
                    vec![]
                } else {
                    vec![Some(HDR), Some(HDR)]
                };
                let mut b = d.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
                    label: Some(if shadow {
                        "shadow draws"
                    } else {
                        "scene draws"
                    }),
                    color_formats: &colours,
                    depth_stencil: Some(wgpu::RenderBundleDepthStencil {
                        format: DEPTH,
                        depth_read_only: false,
                        stencil_read_only: true,
                    }),
                    sample_count: 1,
                    multiview: None,
                });
                let draws = if shadow {
                    b.set_bind_group(0, &shadow_global, &[]);
                    b.set_bind_group(2, &cascades[slot], &[]);
                    draw_batches!(b, meshes, list.iter(), |m| &shadow_pipelines
                        [(!m.two_sided) as usize])
                } else {
                    b.set_bind_group(0, &global, &[]);
                    b.set_bind_group(2, &cascades[0], &[]);
                    draw_batches!(b, meshes, list.iter(), |m| &scene_pipelines
                        [m.blended as usize][(!m.two_sided) as usize])
                };
                let bundle = b.finish(&wgpu::RenderBundleDescriptor { label: None });
                cache[slot] = Some(Cached { key, bundle, draws });
            }
        }
        for pass in 0..3 {
            let mut p = e.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(PASSES[pass]),
                color_attachments: &[],
                depth_stencil_attachment: Some(depth_attachment(&shadow_views[pass], true)),
                timestamp_writes: stamp(pass),
                ..Default::default()
            });
            timed[pass] = true;
            if let Some(c) = cache[pass].as_ref().filter(|_| o.bundles) {
                p.execute_bundles([&c.bundle]);
                calls += c.draws;
                continue;
            }
            p.set_bind_group(0, &shadow_global, &[]);
            p.set_bind_group(2, &cascades[pass], &[]);
            calls += draw_batches!(p, meshes, batches[pass].iter(), |m| &shadow_pipelines
                [(!m.two_sided) as usize]);
        }
        // Split: sky, opaque and water each in their own pass (3, 4, 5). Merged: one pass (6).
        let scene_passes: &[usize] = if merged { &[6] } else { &[3, 4, 5] };
        for &pass in scene_passes {
            let clear = pass != 4 && pass != 5;
            let colors = [
                attachment(
                    &hdr.view,
                    clear.then_some(wgpu::Color {
                        r: look.sky[0] as f64,
                        g: look.sky[1] as f64,
                        b: look.sky[2] as f64,
                        a: 1.,
                    }),
                ),
                attachment(
                    &motion.view,
                    clear.then_some(wgpu::Color {
                        r: 0.,
                        g: 0.,
                        b: 800.,
                        a: 0.,
                    }),
                ),
            ];
            let mut p = e.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(PASSES[pass]),
                color_attachments: &colors,
                depth_stencil_attachment: Some(depth_attachment(&depth.view, clear)),
                timestamp_writes: stamp(pass),
                ..Default::default()
            });
            timed[pass] = true;
            p.set_bind_group(0, &global, &[]);
            p.set_bind_group(1, &meshes[0].bind, &[]);
            p.set_bind_group(2, &cascades[0], &[]);
            if pass == 3 || pass == 6 {
                p.set_pipeline(&sky_pipeline);
                p.draw(0..3, 0..1);
                calls += 1;
            }
            for blended in [false, true] {
                if (pass == 4 && blended) || (pass == 5 && !blended) || pass == 3 {
                    continue;
                }
                if let Some(c) = cache[3 + blended as usize].as_ref().filter(|_| o.bundles) {
                    p.execute_bundles([&c.bundle]);
                    calls += c.draws;
                    continue;
                }
                p.set_bind_group(0, &global, &[]);
                p.set_bind_group(2, &cascades[0], &[]);
                calls += draw_batches!(
                    p,
                    meshes,
                    batches[3]
                        .iter()
                        .filter(|b| meshes[b.mesh].blended == blended),
                    |m| &scene_pipelines[m.blended as usize][(!m.two_sided) as usize]
                );
            }
        }
        for pass in 7..9 {
            let colors = [attachment(
                if pass == 7 {
                    &history[frame % 2].view
                } else {
                    &output.view
                },
                Some(wgpu::Color::TRANSPARENT),
            )];
            let mut p = e.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(PASSES[pass]),
                color_attachments: &colors,
                timestamp_writes: stamp(pass),
                ..Default::default()
            });
            timed[pass] = true;
            p.set_pipeline(if pass == 7 { &taa } else { &composite });
            p.set_bind_group(0, &post_global, &[]);
            p.set_bind_group(
                1,
                if pass == 7 {
                    &taa_binds[frame % 2]
                } else {
                    &composite_binds[frame % 2]
                },
                &[],
            );
            p.draw(0..3, 0..1);
            calls += 1;
        }
        let queries_used = 2 * PASSES.len() as u32;
        e.resolve_query_set(&queries, 0..queries_used, &resolve, 0);
        e.copy_buffer_to_buffer(&resolve, 0, &readback, 0, u64::from(queries_used) * 8);
        q.submit([e.finish()]);
        let cpu_encode_ms = started.elapsed().as_secs_f64() * 1000.;
        let (tx, rx) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        d.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })?;
        rx.recv()??;
        let frame_work_ms = started.elapsed().as_secs_f64() * 1000.;
        let bytes = readback.slice(..).get_mapped_range()?;
        let ticks: &[u64] = bytemuck::cast_slice(&bytes);
        let factor = q.get_timestamp_period() as f64 / 1e6;
        let passes_ms = std::array::from_fn(|i| {
            timed[i].then(|| ticks[i * 2 + 1].saturating_sub(ticks[i * 2]) as f64 * factor)
        });
        let gpu_ms = ticks[2 * PASSES.len() - 1].saturating_sub(ticks[0]) as f64 * factor;
        if frame >= o.warmup {
            samples.push(Sample {
                frame,
                cpu_encode_ms,
                frame_work_ms,
                gpu_ms,
                passes_ms,
                draw_calls: calls,
            });
        }
        drop(bytes);
        readback.unmap();
    }
    if let Some(path) = &o.capture {
        // Explicit opt-in for the Mac capture worker. Benchmark invocations omit this.
        let row = (o.width * 4).div_ceil(256) * 256;
        let pixels = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: u64::from(row) * u64::from(o.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = d.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &output.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &pixels,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(o.height),
                },
            },
            wgpu::Extent3d {
                width: o.width,
                height: o.height,
                depth_or_array_layers: 1,
            },
        );
        q.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        pixels.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        d.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })?;
        rx.recv()??;
        let mapped = pixels.slice(..).get_mapped_range()?;
        let mut rgba = Vec::with_capacity((o.width * o.height * 4) as usize);
        for line in mapped.chunks_exact(row as usize) {
            for pixel in line[..o.width as usize * 4].chunks_exact(4) {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
        }
        image::save_buffer(path, &rgba, o.width, o.height, image::ColorType::Rgba8)?;
    }
    Ok((format!("{} ({:?})", info.name, info.backend), samples))
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_ports_validate_before_a_device_is_required() {
        for (name, source) in [
            ("scene", include_str!("scene.wgsl")),
            ("post", include_str!("post.wgsl")),
            ("utility", include_str!("utility.wgsl")),
        ] {
            let module = naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(source)));
        }
    }
}
