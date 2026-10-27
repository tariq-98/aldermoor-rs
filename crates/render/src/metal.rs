//! Direct Metal: temporal AA, stable cascades, visibility and bounded frames in flight.
use crate::{
    character::Draw,
    model::{Material, Texture},
    settings::{LookSettings, ToneMap},
    visibility::{Bounds, FarShadowCache, Frustum, ShadowCasterVolume, stable_cascades},
};
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_foundation::{NSRange, NSString};
use objc2_metal::*;
use objc2_metal_fx::{MTLFXTemporalScaler, MTLFXTemporalScalerBase, MTLFXTemporalScalerDescriptor};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use std::{
    collections::{HashMap, VecDeque},
    ffi::c_void,
    path::Path,
    ptr::NonNull,
    sync::{Arc, Mutex},
    time::Instant,
};
mod fx_pass;
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}
type Obj<T> = Retained<ProtocolObject<T>>;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
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
    // Height fog reserves grade[0/1/2].w: base height, height falloff, sky distance.
    grade: [[f32; 4]; 3],
    // params.w is height-fog enabled; grading RGB and params.xyz keep their meaning.
    params: [f32; 4],
    clock: [f32; 4],
    viewport: [f32; 4],
    splits: [f32; 4],
}
// Separate from the scene Frame contract owned by the lighting/fog lane.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostUniform {
    ao: [f32; 4],
    bloom: [f32; 4],
    projection: [f32; 4],
    blur: [f32; 4],
}
impl PostUniform {
    fn new(look: &LookSettings, camera: Camera, aspect: f32) -> Self {
        let projection = 1. / (camera.fov * 0.5).tan();
        Self {
            ao: if look.ao_active() {
                [look.ao_radius, look.ao_bias, look.ao_strength, 1.]
            } else {
                [0.; 4]
            },
            bloom: if look.bloom_active() {
                [
                    look.bloom_threshold,
                    look.bloom_radius,
                    look.bloom_strength,
                    1.,
                ]
            } else {
                [0.; 4]
            },
            projection: [projection / aspect, projection, 0., look.bloom_knee],
            blur: [0.; 4],
        }
    }
    fn active(&self) -> bool {
        self.ao[3] > 0. || self.bloom[3] > 0.
    }
}
struct PostTargets {
    ao: Obj<dyn MTLTexture>,
    bloom: [Obj<dyn MTLTexture>; 3],
    colour: Obj<dyn MTLTexture>,
}
impl PostTargets {
    /// AO, three bloom targets, then colour.
    fn descs(
        w: usize,
        h: usize,
        colour_usage: MTLTextureUsage,
    ) -> [Retained<MTLTextureDescriptor>; 5] {
        let usage = MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead;
        let half = |format| target_desc(w.div_ceil(2), h.div_ceil(2), format, 1, usage);
        [
            half(MTLPixelFormat::RG16Float),
            half(MTLPixelFormat::RGBA16Float),
            half(MTLPixelFormat::RGBA16Float),
            half(MTLPixelFormat::RGBA16Float),
            target_desc(w, h, MTLPixelFormat::RGBA16Float, 1, usage | colour_usage),
        ]
    }
    fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        heap: Option<&ProtocolObject<dyn MTLHeap>>,
        w: usize,
        h: usize,
        colour_usage: MTLTextureUsage,
    ) -> Result<Self> {
        let [ao, b0, b1, b2, colour] = Self::descs(w, h, colour_usage);
        Ok(Self {
            ao: target(device, heap, &ao)?,
            bloom: [
                target(device, heap, &b0)?,
                target(device, heap, &b1)?,
                target(device, heap, &b2)?,
            ],
            colour: target(device, heap, &colour)?,
        })
    }
}
impl Frame {
    fn with_height_fog(mut self, fog: crate::settings::HeightFogSettings) -> Self {
        let values = fog.uniform();
        for (row, value) in self.grade.iter_mut().zip(values) {
            row[3] = value;
        }
        self.params[3] = values[3];
        self
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawUniform {
    model: [[f32; 4]; 4],
    previous_model: [[f32; 4]; 4],
    base: [f32; 4],
    emission: [f32; 4],
    params: [f32; 4],
    flags: [u32; 4],
    morph: [f32; 4],
    previous_morph: [f32; 4],
    dyes: [[f32; 4]; 2],
    references: [[f32; 4]; 2],
}
// Word-packed shadow inputs. Offsets are shared with shadow_vertex in shaders.metal.
// Positions are always present; unclassified rigid meshes use exactly 12 bytes/vertex.
// Optional data retains full precision, including uint joints and vertex alpha.
fn shadow_stream(p: &crate::model::Primitive) -> (Vec<u32>, [u32; 2]) {
    let region = p.vertices.iter().any(|v| v.position[3] != -1.);
    let wind = p.vertices.iter().any(|v| v.normal[3].to_bits() != 0);
    let skin = p.skin.is_some()
        || p.vertices.iter().any(|v| {
            v.joints != [0; 4] || v.weights.map(f32::to_bits) != [1f32.to_bits(), 0, 0, 0]
        });
    let mask = p.material.alpha_mode == 1;
    let flags = region as u32 | (wind as u32) << 1 | (skin as u32) << 2 | (mask as u32) << 3;
    let stride = 3 + region as usize + wind as usize + 8 * skin as usize + 3 * mask as usize;
    let mut words = Vec::with_capacity(p.vertices.len() * stride);
    for v in &p.vertices {
        words.extend(v.position[..3].iter().map(|x| x.to_bits()));
        if region {
            words.push(v.position[3].to_bits());
        }
        if wind {
            words.push(v.normal[3].to_bits());
        }
        if skin {
            words.extend(v.joints);
            words.extend(v.weights.map(f32::to_bits));
        }
        if mask {
            words.extend([v.uv[0].to_bits(), v.uv[1].to_bits(), v.color[3].to_bits()]);
        }
    }
    (words, [stride as u32, flags])
}
struct GpuMesh {
    vertices: Obj<dyn MTLBuffer>,
    indices: Obj<dyn MTLBuffer>,
    morphs: Obj<dyn MTLBuffer>,
    count: usize,
    maps: [Obj<dyn MTLTexture>; 5],
    material: Material,
    shadow_vertices: Obj<dyn MTLBuffer>,
    shadow_layout: [u32; 2],
}
pub const GPU_PASS_NAMES: [&str; 15] = [
    "shadow_0",
    "shadow_1",
    "shadow_2",
    "scene_sky_opaque_water_transparent",
    "taa",
    "composite",
    "hud",
    "sky",
    "opaque",
    "water_transparent",
    "ao_half",
    "bloom_extract_half",
    "bloom_horizontal_half",
    "bloom_vertical_half",
    "post_apply",
];
const PASS_COUNT: usize = GPU_PASS_NAMES.len();
const SAMPLE_COUNT: usize = PASS_COUNT * 4;

#[derive(Clone, Copy, Default, Debug, serde::Serialize)]
pub struct PassTiming {
    pub elapsed_ms: f64,
    pub vertex_ms: f64,
    pub fragment_ms: f64,
}
#[derive(Clone, Copy, Default, Debug, serde::Serialize)]
pub struct GpuFrame {
    pub frame: usize,
    pub counters_sampled: bool,
    /// Passes deliberately omitted by scheduling, independent of counter availability.
    pub skipped_pass_mask: u16,
    /// Seconds since renderer creation, at command-buffer submission (not completion).
    pub elapsed_seconds: f64,
    pub gpu_ms: f64,
    pub passes: [Option<PassTiming>; PASS_COUNT],
    pub draw_calls: usize,
    pub visible_triangles: usize,
    /// Metal device allocations, not physical resident memory on a unified-memory GPU.
    pub allocated_gpu_bytes: usize,
}

fn clock_pair(device: &ProtocolObject<dyn MTLDevice>) -> (u64, u64) {
    let (mut cpu, mut gpu) = (0, 0);
    unsafe {
        device.sampleTimestamps_gpuTimestamp(NonNull::from(&mut cpu), NonNull::from(&mut gpu));
    }
    (cpu, gpu)
}

/// One reusable sample buffer per in-flight slot. Read only after its command buffer completes.
pub(crate) struct PassCounters {
    buffer: Obj<dyn MTLCounterSampleBuffer>,
}
impl PassCounters {
    fn new(device: &ProtocolObject<dyn MTLDevice>) -> Result<Self> {
        ensure!(
            device.supportsCounterSampling(MTLCounterSamplingPoint::AtStageBoundary),
            "GPU stage-boundary timestamp sampling is unavailable"
        );
        let sets = device
            .counterSets()
            .context("GPU counter sets unavailable")?;
        let set = sets
            .iter()
            .find(|s| {
                s.name()
                    .isEqualToString(unsafe { MTLCommonCounterSetTimestamp })
            })
            .context("GPU timestamp counter set unavailable")?;
        ensure!(
            set.counters().iter().any(|c| c
                .name()
                .isEqualToString(unsafe { MTLCommonCounterTimestamp })),
            "GPU timestamp counter unavailable"
        );
        let desc = MTLCounterSampleBufferDescriptor::new();
        desc.setCounterSet(Some(&set));
        desc.setStorageMode(MTLStorageMode::Shared);
        unsafe {
            desc.setSampleCount(SAMPLE_COUNT);
        }
        let buffer = device
            .newCounterSampleBufferWithDescriptor_error(&desc)
            .map_err(|e| anyhow::anyhow!("GPU counter allocation: {e}"))?;
        Ok(Self { buffer })
    }
    pub(crate) fn attach(&self, pass: &MTLRenderPassDescriptor, index: usize) {
        assert!(index < PASS_COUNT);
        unsafe {
            let a = pass.sampleBufferAttachments().objectAtIndexedSubscript(0);
            a.setSampleBuffer(Some(&self.buffer));
            a.setStartOfVertexSampleIndex(index * 4);
            a.setEndOfVertexSampleIndex(index * 4 + 1);
            a.setStartOfFragmentSampleIndex(index * 4 + 2);
            a.setEndOfFragmentSampleIndex(index * 4 + 3);
        }
    }
    fn resolve(
        &self,
        mask: u16,
        start: (u64, u64),
        end: (u64, u64),
    ) -> [Option<PassTiming>; PASS_COUNT] {
        let mut result = [None; PASS_COUNT];
        let Some(cpu_ticks) = end.0.checked_sub(start.0).filter(|v| *v > 0) else {
            return result;
        };
        let Some(gpu_ticks) = end.1.checked_sub(start.1).filter(|v| *v > 0) else {
            return result;
        };
        // sampleTimestamps returns CPU nanoseconds, not raw mach_absolute_time ticks.
        // https://developer.apple.com/documentation/metal/converting-gpu-timestamps-into-cpu-time
        let ms_per_tick = cpu_ticks as f64 / gpu_ticks as f64 / 1e6;
        let Some(data) = (unsafe {
            self.buffer
                .resolveCounterRange(NSRange::new(0, SAMPLE_COUNT))
        }) else {
            return result;
        };
        let bytes = unsafe { data.as_bytes_unchecked() };
        if bytes.len() != SAMPLE_COUNT * 8 {
            return result;
        }
        for (i, timing) in result.iter_mut().enumerate() {
            if mask & (1 << i) == 0 {
                continue;
            }
            let t = std::array::from_fn(|j| {
                let offset = (i * 4 + j) * 8;
                u64::from_ne_bytes(bytes[offset..offset + 8].try_into().unwrap())
            });
            *timing = decode_pass(t, ms_per_tick);
        }
        result
    }
}
fn decode_pass(t: [u64; 4], ms_per_tick: f64) -> Option<PassTiming> {
    if !ms_per_tick.is_finite()
        || ms_per_tick <= 0.
        || t.contains(&0)
        || t.contains(&u64::MAX)
        || t[1] < t[0]
        || t[3] < t[2]
        || t[3] < t[0]
    {
        return None;
    }
    Some(PassTiming {
        elapsed_ms: (t[3] - t[0]) as f64 * ms_per_tick,
        vertex_ms: (t[1] - t[0]) as f64 * ms_per_tick,
        fragment_ms: (t[3] - t[2]) as f64 * ms_per_tick,
    })
}
struct Slot {
    draws: Obj<dyn MTLBuffer>,
    bones: Obj<dyn MTLBuffer>,
    pending: Option<Obj<dyn MTLCommandBuffer>>,
    counters: Option<PassCounters>,
    measured: GpuFrame,
    pass_mask: u16,
    clock_start: (u64, u64),
}
struct Previous {
    model_key: u64,
    transform: Mat4,
    palette: Arc<[Mat4]>,
    morph: [f32; 2],
}
#[derive(Clone, Copy)]
enum DrawPass {
    Shadow(usize),
    Scene,
    Opaque,
    Transparent,
}
struct Prepared {
    uniform: usize,
    draw: usize,
    current: usize,
    previous: usize,
    bounds: Bounds,
    scene: bool,
    shadows: [bool; 3],
}
#[derive(Clone, Copy, Default, Debug)]
pub struct Stats {
    pub gpu_ms: f64,
    pub completed: Option<GpuFrame>,
    pub draw_calls: usize,
    pub triangles: usize,
    pub culled: usize,
    pub bone_bytes: usize,
    pub bone_capacity: usize,
    pub history_resets: usize,
}
#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov: f32,
}
impl Camera {
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(self.fov, aspect, 0.1, 800.)
            * glam::camera::rh::view::look_at_mat4(self.eye, self.target, Vec3::Y)
    }
}
pub struct Renderer {
    surface_uniform: crate::surfaces::Uniform,
    surface_maps: [Obj<dyn MTLTexture>; 5],
    sky_pipeline: Obj<dyn MTLRenderPipelineState>,
    prefilter_pipeline: Obj<dyn MTLRenderPipelineState>,
    sky_reflection: Obj<dyn MTLTexture>,
    metalfx: Option<Obj<dyn MTLFXTemporalScaler>>,
    metalfx_output: Option<Obj<dyn MTLTexture>>,
    metalfx_requested: bool,
    presented: Arc<Mutex<Vec<(usize, f64)>>>,
    pub target_fps: f64,
    gui: crate::gui::Gui,
    pub hud: Option<crate::gui::HudFrame>,
    pub device: Obj<dyn MTLDevice>,
    pub layer: Option<Retained<CAMetalLayer>>,
    queue: Obj<dyn MTLCommandQueue>,
    opaque: Obj<dyn MTLRenderPipelineState>,
    blend: Obj<dyn MTLRenderPipelineState>,
    shadow_pipeline: Obj<dyn MTLRenderPipelineState>,
    shadow_mask_pipeline: Obj<dyn MTLRenderPipelineState>,
    taa: Obj<dyn MTLRenderPipelineState>,
    composite: Obj<dyn MTLRenderPipelineState>,
    post_pipelines: [Obj<dyn MTLRenderPipelineState>; 4],
    post_targets: Option<PostTargets>,
    target_heaps: Option<TargetHeaps>,
    post_config: [[f32; 4]; 3],
    depth_state: Obj<dyn MTLDepthStencilState>,
    read_depth: Obj<dyn MTLDepthStencilState>,
    hdr: Obj<dyn MTLTexture>,
    msaa: Obj<dyn MTLTexture>,
    motion: Obj<dyn MTLTexture>,
    motion_msaa: Obj<dyn MTLTexture>,
    depth: Obj<dyn MTLTexture>,
    shadow: Obj<dyn MTLTexture>,
    far_shadow_cache: FarShadowCache,
    history: [Obj<dyn MTLTexture>; 2],
    output: Obj<dyn MTLTexture>,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    samples: usize,
    temporal: bool,
    pub look: LookSettings,
    pub scene_time: Option<f32>,
    pub inline_constants: bool,
    pub direct_drawable: bool,
    /// Diagnostic only: separate scene encoders change tile load/store cost.
    pub profile_scene: bool,
    /// Diagnostic A/B sampling gate; buffers stay allocated and in-flight work stays intact.
    pub sample_gpu_counters: bool,
    started: Instant,
    meshes: HashMap<(u64, usize), GpuMesh>,
    textures: HashMap<(String, bool), Obj<dyn MTLTexture>>,
    fallbacks: [Obj<dyn MTLTexture>; 5],
    slots: Vec<Slot>,
    completed_frames: VecDeque<GpuFrame>,
    counters_enabled: bool,
    frame: usize,
    pub stats: Stats,
    previous: HashMap<u64, Previous>,
    previous_vp: Mat4,
    previous_camera: Option<Camera>,
    previous_time: f32,
    history_valid: bool,
    fx_pass: fx_pass::FxPass,
    /// This frame's combat effects (crate::fx); taken by `draw`.
    pub effects: Option<crate::fx::Frame>,
}
const INITIAL_RING_BYTES: usize = 64 * 1024;
const SHADOW_SIZE: usize = 1024;
fn ptr<T>(v: &T) -> NonNull<c_void> {
    NonNull::from(v).cast()
}
fn buffer<T: bytemuck::Pod>(
    d: &ProtocolObject<dyn MTLDevice>,
    data: &[T],
) -> Result<Obj<dyn MTLBuffer>> {
    ensure!(!data.is_empty(), "empty GPU buffer");
    unsafe {
        d.newBufferWithBytes_length_options(
            NonNull::new(data.as_ptr() as *mut c_void).unwrap(),
            std::mem::size_of_val(data),
            MTLResourceOptions::StorageModeShared,
        )
        .context("Metal buffer allocation")
    }
}
fn target_desc(
    w: usize,
    h: usize,
    format: MTLPixelFormat,
    samples: usize,
    usage: MTLTextureUsage,
) -> Retained<MTLTextureDescriptor> {
    let desc = MTLTextureDescriptor::new();
    unsafe { desc.setWidth(w) };
    unsafe { desc.setHeight(h) };
    desc.setPixelFormat(format);
    desc.setStorageMode(MTLStorageMode::Private);
    desc.setUsage(usage);
    unsafe { desc.setSampleCount(samples) };
    if samples > 1 {
        desc.setTextureType(MTLTextureType::Type2DMultisample);
    }
    desc
}
fn texture(
    d: &ProtocolObject<dyn MTLDevice>,
    w: usize,
    h: usize,
    format: MTLPixelFormat,
    samples: usize,
    usage: MTLTextureUsage,
) -> Result<Obj<dyn MTLTexture>> {
    d.newTextureWithDescriptor(&target_desc(w, h, format, samples, usage))
        .context("Metal texture allocation")
}
/// Places a render target in `heap` when it has room, otherwise allocates from the device.
fn target(
    d: &ProtocolObject<dyn MTLDevice>,
    heap: Option<&ProtocolObject<dyn MTLHeap>>,
    desc: &MTLTextureDescriptor,
) -> Result<Obj<dyn MTLTexture>> {
    if let Some(texture) = heap.and_then(|heap| heap.newTextureWithDescriptor(desc)) {
        return Ok(texture);
    }
    if let Some(heap) = heap {
        eprintln!(
            "render target heap full ({} of {} bytes used); allocating from the device",
            heap.usedSize(),
            heap.size()
        );
    }
    d.newTextureWithDescriptor(desc)
        .context("Metal texture allocation")
}
/// Every texture sized by the render scale: hdr, MSAA colour, motion, MSAA motion, depth,
/// two history targets, then the post targets (`PostTargets::descs`).
fn scaled_target_descs(w: usize, h: usize, samples: usize) -> Vec<Retained<MTLTextureDescriptor>> {
    let rt = MTLTextureUsage::RenderTarget;
    let read = MTLTextureUsage::ShaderRead;
    let colour = MTLPixelFormat::RGBA16Float;
    let mut descs = vec![
        target_desc(w, h, colour, 1, rt | read),
        target_desc(w, h, colour, samples, rt),
        target_desc(w, h, colour, 1, rt | read),
        target_desc(w, h, colour, samples, rt),
        target_desc(w, h, MTLPixelFormat::Depth32Float, samples, rt | read),
        target_desc(w, h, colour, 1, rt | read),
        target_desc(w, h, colour, 1, rt | read),
    ];
    descs.extend(PostTargets::descs(w, h, rt | read));
    descs
}
/// Scale-dependent targets live in two heaps used in turn. A dynamic-resolution step then
/// places textures in memory that is already allocated, instead of asking the system for
/// ~150 MB mid-game, which hitched for hundreds of milliseconds under memory pressure.
/// Frames still in flight keep the other heap's textures; steps are a second apart.
struct TargetHeaps {
    /// The largest render scale the heaps are sized for.
    max_scale: f32,
    heaps: [Option<Obj<dyn MTLHeap>>; 2],
    current: usize,
}
impl TargetHeaps {
    fn current(&self) -> Option<&ProtocolObject<dyn MTLHeap>> {
        self.heaps[self.current].as_deref()
    }
    /// Grows both heaps to hold a full set at `bytes`, then returns the one not in use.
    fn spare(
        &mut self,
        d: &ProtocolObject<dyn MTLDevice>,
        bytes: usize,
    ) -> Result<&ProtocolObject<dyn MTLHeap>> {
        for heap in &mut self.heaps {
            if heap.as_ref().is_none_or(|h| h.size() < bytes) {
                // Textures in a replaced heap keep it alive until they are released.
                let desc = MTLHeapDescriptor::new();
                desc.setSize(bytes);
                desc.setStorageMode(MTLStorageMode::Private);
                desc.setType(MTLHeapType::Automatic);
                desc.setHazardTrackingMode(MTLHazardTrackingMode::Tracked);
                *heap = Some(
                    d.newHeapWithDescriptor(&desc)
                        .context("render target heap")?,
                );
            }
        }
        self.current = 1 - self.current;
        Ok(self.heaps[self.current].as_deref().unwrap())
    }
}
fn heap_bytes(
    d: &ProtocolObject<dyn MTLDevice>,
    descs: &[Retained<MTLTextureDescriptor>],
) -> usize {
    let mut bytes: usize = 0;
    let mut largest_align: usize = 1;
    for desc in descs {
        let MTLSizeAndAlign { size, align } = d.heapTextureSizeAndAlignWithDescriptor(desc);
        bytes = bytes.next_multiple_of(align.max(1)) + size;
        largest_align = largest_align.max(align);
    }
    bytes + largest_align
}
fn image_texture(
    d: &ProtocolObject<dyn MTLDevice>,
    queue: &ProtocolObject<dyn MTLCommandQueue>,
    t: &Texture,
    srgb: bool,
) -> Result<Obj<dyn MTLTexture>> {
    if let Some(k) = &t.compressed {
        ensure!(
            k.srgb == srgb,
            "KTX colour space does not match material slot: {}",
            t.key
        );
        let desc = MTLTextureDescriptor::new();
        unsafe {
            desc.setWidth(k.width as usize);
            desc.setHeight(k.height as usize);
            desc.setMipmapLevelCount(k.levels.len());
        }
        desc.setPixelFormat(if srgb {
            MTLPixelFormat::ASTC_6x6_sRGB
        } else {
            MTLPixelFormat::ASTC_6x6_LDR
        });
        desc.setStorageMode(MTLStorageMode::Shared);
        desc.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::PixelFormatView);
        let tex = d
            .newTextureWithDescriptor(&desc)
            .context("ASTC texture allocation")?;
        for (i, range) in k.levels.iter().enumerate() {
            let w = (k.width >> i).max(1) as usize;
            let h = (k.height >> i).max(1) as usize;
            unsafe {
                tex.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                    MTLRegion {
                        origin: MTLOrigin { x: 0, y: 0, z: 0 },
                        size: MTLSize {
                            width: w,
                            height: h,
                            depth: 1,
                        },
                    },
                    i,
                    NonNull::new(k.bytes[range.clone()].as_ptr() as *mut c_void).unwrap(),
                    w.div_ceil(6) * 16,
                );
            }
        }
        return Ok(tex);
    }
    let desc = MTLTextureDescriptor::new();
    unsafe { desc.setWidth(t.width as usize) };
    unsafe { desc.setHeight(t.height as usize) };
    desc.setPixelFormat(if srgb {
        MTLPixelFormat::RGBA8Unorm_sRGB
    } else {
        MTLPixelFormat::RGBA8Unorm
    });
    desc.setStorageMode(MTLStorageMode::Shared);
    desc.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::PixelFormatView);
    unsafe { desc.setMipmapLevelCount(1 + (t.width.max(t.height).ilog2() as usize)) };
    let tex = d
        .newTextureWithDescriptor(&desc)
        .context("material texture allocation")?;
    t.restore_pixels()?;
    let mut pixels_lock = t.rgba.lock().unwrap();
    let pixels = pixels_lock
        .as_ref()
        .context("texture pixels already uploaded")?;
    unsafe {
        tex.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            MTLRegion {
                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                size: MTLSize {
                    width: t.width as usize,
                    height: t.height as usize,
                    depth: 1,
                },
            },
            0,
            NonNull::new(pixels.as_ptr() as *mut c_void).unwrap(),
            t.width as usize * 4,
        );
    }
    pixels_lock.take();
    drop(pixels_lock);
    if desc.mipmapLevelCount() > 1 {
        let cb = queue.commandBuffer().context("texture command buffer")?;
        let blit = cb.blitCommandEncoder().context("mipmap encoder")?;
        blit.generateMipmapsForTexture(&tex);
        blit.endEncoding();
        cb.commit();
    }
    Ok(tex)
}
impl Renderer {
    pub fn new(width: u32, height: u32, scale: f32) -> Result<Self> {
        Self::with_samples(width, height, scale, 0)
    }
    /// 0 is temporal AA, 1 is unfiltered (diagnostics), 4 is MSAA (comparison only).
    pub fn with_samples(width: u32, height: u32, scale: f32, mode: usize) -> Result<Self> {
        ensure!(
            [0, 1, 4].contains(&mode),
            "AA mode must be temporal, none or MSAA"
        );
        let temporal = mode == 0;
        let samples = mode.max(1);
        ensure!(
            width > 0 && height > 0 && scale.is_finite() && (0.25..=1.).contains(&scale),
            "invalid render size or scale"
        );
        let device = MTLCreateSystemDefaultDevice().context("No Metal GPU is available")?;
        let queue = device.newCommandQueue().context("Metal command queue")?;
        let library = device
            .newLibraryWithSource_options_error(
                &NSString::from_str(include_str!("shaders.metal")),
                None,
            )
            .map_err(|e| anyhow::anyhow!("Metal shaders: {e}"))?;
        let pipeline = |vs: &str,
                        fs: Option<&str>,
                        samples: usize,
                        colours: &[MTLPixelFormat],
                        depth: bool,
                        blending: bool|
         -> Result<Obj<dyn MTLRenderPipelineState>> {
            let p = MTLRenderPipelineDescriptor::new();
            p.setVertexFunction(
                library
                    .newFunctionWithName(&NSString::from_str(vs))
                    .as_deref(),
            );
            if let Some(fs) = fs {
                p.setFragmentFunction(
                    library
                        .newFunctionWithName(&NSString::from_str(fs))
                        .as_deref(),
                );
            }
            p.setRasterSampleCount(samples);
            for (i, format) in colours.iter().enumerate() {
                let c = unsafe { p.colorAttachments().objectAtIndexedSubscript(i) };
                c.setPixelFormat(*format);
                if blending && i == 0 {
                    c.setBlendingEnabled(true);
                    c.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
                    c.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
                    c.setSourceAlphaBlendFactor(MTLBlendFactor::One);
                    c.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
                }
            }
            if depth {
                p.setDepthAttachmentPixelFormat(MTLPixelFormat::Depth32Float);
            }
            device
                .newRenderPipelineStateWithDescriptor_error(&p)
                .map_err(|e| anyhow::anyhow!("Metal pipeline {vs}: {e}"))
        };
        let scene_formats = [MTLPixelFormat::RGBA16Float; 2];
        let sky_pipeline = pipeline(
            "fullscreen",
            Some("sky_fragment"),
            samples,
            &scene_formats,
            true,
            false,
        )?;
        let opaque = pipeline(
            "scene_vertex",
            Some("scene_fragment"),
            samples,
            &scene_formats,
            true,
            false,
        )?;
        let blend = pipeline(
            "scene_vertex",
            Some("scene_fragment"),
            samples,
            &scene_formats,
            true,
            true,
        )?;
        // Opaque casters need only depth. Keep discard confined to masked casters.
        let shadow_pipeline = pipeline("shadow_vertex", None, 1, &[], true, false)?;
        let shadow_mask_pipeline = pipeline(
            "shadow_vertex",
            Some("shadow_fragment"),
            1,
            &[],
            true,
            false,
        )?;
        let taa = pipeline(
            "fullscreen",
            Some("temporal_resolve"),
            1,
            &[MTLPixelFormat::RGBA16Float],
            false,
            false,
        )?;
        let prefilter_pipeline = pipeline(
            "fullscreen",
            Some("prefilter_sky"),
            1,
            &[MTLPixelFormat::RGBA16Float],
            false,
            false,
        )?;
        let composite = pipeline(
            "fullscreen",
            Some("composite"),
            1,
            &[MTLPixelFormat::BGRA8Unorm_sRGB],
            false,
            false,
        )?;
        let post_pipelines = [
            pipeline(
                "fullscreen",
                Some(if samples > 1 {
                    "post_ao_msaa"
                } else {
                    "post_ao"
                }),
                1,
                &[MTLPixelFormat::RG16Float],
                false,
                false,
            )?,
            pipeline(
                "fullscreen",
                Some("post_bloom_extract"),
                1,
                &[MTLPixelFormat::RGBA16Float],
                false,
                false,
            )?,
            pipeline(
                "fullscreen",
                Some("post_bloom_blur"),
                1,
                &[MTLPixelFormat::RGBA16Float],
                false,
                false,
            )?,
            pipeline(
                "fullscreen",
                Some(if samples > 1 {
                    "post_apply_msaa"
                } else {
                    "post_apply"
                }),
                1,
                &[MTLPixelFormat::RGBA16Float],
                false,
                false,
            )?,
        ];
        let desc = MTLDepthStencilDescriptor::new();
        desc.setDepthCompareFunction(MTLCompareFunction::LessEqual);
        desc.setDepthWriteEnabled(true);
        let depth_state = device
            .newDepthStencilStateWithDescriptor(&desc)
            .context("depth state")?;
        desc.setDepthWriteEnabled(false);
        let read_depth = device
            .newDepthStencilStateWithDescriptor(&desc)
            .context("blend depth state")?;
        let (w, h) = (
            (width as f32 * scale).round() as usize,
            (height as f32 * scale).round() as usize,
        );
        let rt = MTLTextureUsage::RenderTarget;
        let read = MTLTextureUsage::ShaderRead;
        let hdr = texture(&device, w, h, MTLPixelFormat::RGBA16Float, 1, rt | read)?;
        let msaa = texture(&device, w, h, MTLPixelFormat::RGBA16Float, samples, rt)?;
        let motion = texture(&device, w, h, MTLPixelFormat::RGBA16Float, 1, rt | read)?;
        let motion_msaa = texture(&device, w, h, MTLPixelFormat::RGBA16Float, samples, rt)?;
        let depth = texture(
            &device,
            w,
            h,
            MTLPixelFormat::Depth32Float,
            samples,
            rt | read,
        )?;
        let sd = MTLTextureDescriptor::new();
        unsafe {
            sd.setWidth(SHADOW_SIZE);
            sd.setHeight(SHADOW_SIZE);
            sd.setArrayLength(3);
        }
        sd.setTextureType(MTLTextureType::Type2DArray);
        sd.setPixelFormat(MTLPixelFormat::Depth32Float);
        sd.setStorageMode(MTLStorageMode::Private);
        sd.setUsage(rt | read);
        let shadow = device
            .newTextureWithDescriptor(&sd)
            .context("cascade array")?;
        let history = [
            texture(&device, w, h, MTLPixelFormat::RGBA16Float, 1, rt | read)?,
            texture(&device, w, h, MTLPixelFormat::RGBA16Float, 1, rt | read)?,
        ];
        let output = texture(
            &device,
            width as usize,
            height as usize,
            MTLPixelFormat::BGRA8Unorm_sRGB,
            1,
            rt | read,
        )?;
        let mut fallback = Vec::new();
        for (i, pixel) in [
            [255, 255, 255, 255],
            [128, 128, 255, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
            [255, 255, 255, 255],
        ]
        .into_iter()
        .enumerate()
        {
            fallback.push(image_texture(
                &device,
                &queue,
                &Texture {
                    key: format!("engine-default{i}"),
                    width: 1,
                    height: 1,
                    rgba: Mutex::new(Some(pixel.to_vec())),
                    compressed: None,
                },
                i == 0 || i == 3,
            )?);
        }
        let mut slots = Vec::new();
        for _ in 0..1 {
            slots.push(Slot {
                draws: device
                    .newBufferWithLength_options(
                        INITIAL_RING_BYTES,
                        MTLResourceOptions::StorageModeShared,
                    )
                    .context("draw ring buffer")?,
                bones: device
                    .newBufferWithLength_options(
                        INITIAL_RING_BYTES,
                        MTLResourceOptions::StorageModeShared,
                    )
                    .context("bone ring buffer")?,
                pending: None,
                counters: None,
                measured: GpuFrame::default(),
                pass_mask: 0,
                clock_start: (0, 0),
            });
        }
        eprintln!(
            "Metal: {} | {w}x{h} -> {width}x{height} | {samples}x samples | {} | cascades=3x{SHADOW_SIZE}",
            device.name(),
            if temporal { "TAA" } else { "no temporal AA" }
        );
        let gui = crate::gui::Gui::new(&device, &library)?;
        let fx_pass = fx_pass::FxPass::new(&device, samples)?;
        let surface_maps = [
            texture_array(&device, &queue, &[fallback[0].clone()])?,
            texture_array(&device, &queue, &[fallback[1].clone()])?,
            fallback[0].clone(),
            fallback[1].clone(),
            fallback[0].clone(),
        ];
        Ok(Self {
            gui,
            surface_uniform: Default::default(),
            surface_maps,
            sky_pipeline,
            prefilter_pipeline,
            sky_reflection: fallback[0].clone(),
            metalfx: None,
            metalfx_output: None,
            metalfx_requested: false,
            hud: None,
            presented: Arc::new(Mutex::new(Vec::new())),
            target_fps: 60.,
            device,
            queue,
            layer: None,
            opaque,
            blend,
            shadow_pipeline,
            shadow_mask_pipeline,
            taa,
            composite,
            post_pipelines,
            post_targets: None,
            target_heaps: None,
            post_config: [[0.; 4]; 3],
            depth_state,
            read_depth,
            hdr,
            msaa,
            motion,
            motion_msaa,
            depth,
            shadow,
            far_shadow_cache: FarShadowCache::default(),
            history,
            output,
            width,
            height,
            scale,
            samples,
            temporal,
            look: LookSettings::default(),
            scene_time: None,
            inline_constants: true,
            direct_drawable: true,
            profile_scene: false,
            sample_gpu_counters: true,
            started: Instant::now(),
            meshes: HashMap::new(),
            textures: HashMap::new(),
            fallbacks: fallback.try_into().ok().unwrap(),
            slots,
            completed_frames: VecDeque::with_capacity(64),
            counters_enabled: false,
            frame: 0,
            stats: Stats::default(),
            previous: HashMap::new(),
            previous_vp: Mat4::IDENTITY,
            previous_camera: None,
            previous_time: 0.,
            history_valid: false,
            fx_pass,
            effects: None,
        })
    }
    pub fn load_surface_assets(&mut self, path: &Path, root: &Path) -> Result<()> {
        self.wait()?;
        let inputs = crate::surfaces::SurfaceAssets::load(path)?;
        let mut u = crate::surfaces::Uniform::default();
        if let Some(t) = inputs.terrain {
            let mut colours = Vec::new();
            let mut normals = Vec::new();
            for (i, layer) in t.layers.iter().enumerate() {
                colours.push(self.image(&*Texture::load(&root.join(&layer.colour))?, true)?);
                normals.push(self.image(&*Texture::load(&root.join(&layer.normal))?, false)?);
                u.scales[i] = 1. / layer.uv_metres;
                u.roughness[i] = layer.roughness;
            }
            self.surface_maps[0] = texture_array(&self.device, &self.queue, &colours)?;
            self.surface_maps[1] = texture_array(&self.device, &self.queue, &normals)?;
            self.surface_maps[2] = self.image(&*Texture::load(&root.join(t.weights))?, false)?;
            u.bounds = t.bounds;
            u.flags[0] = t.layers.len() as u32;
        }
        if let Some(w) = inputs.water {
            self.surface_maps[3] = self.image(&*Texture::load(&root.join(w.normal))?, false)?;
            u.water_deep = [
                w.deep_colour[0],
                w.deep_colour[1],
                w.deep_colour[2],
                w.opacity,
            ];
            u.water_shallow = [
                w.shallow_colour[0],
                w.shallow_colour[1],
                w.shallow_colour[2],
                w.normal_strength,
            ];
            u.water_absorption = [
                w.absorption[0],
                w.absorption[1],
                w.absorption[2],
                w.reflection_strength,
            ];
            u.water_flow = [1. / w.uv_metres, w.flow[0], w.flow[1], w.roughness];
            u.sky[2] = w.ior;
            u.flags[1] = 1;
        }
        if let Some(s) = inputs.sky {
            self.surface_maps[4] = self.image(&*Texture::load(&root.join(s.texture))?, true)?;
            u.sky[0] = s.rotation_radians / std::f32::consts::TAU;
            u.sky[1] = s.intensity;
            u.flags[2] = 1;
            self.prefilter_sky()?;
        }
        self.surface_uniform = u;
        self.reset_temporal_history();
        Ok(())
    }
    fn prefilter_sky(&mut self) -> Result<()> {
        let desc = MTLTextureDescriptor::new();
        unsafe {
            desc.setWidth(256);
            desc.setHeight(128);
            desc.setMipmapLevelCount(9);
        }
        desc.setPixelFormat(MTLPixelFormat::RGBA16Float);
        desc.setStorageMode(MTLStorageMode::Private);
        desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        let map = self
            .device
            .newTextureWithDescriptor(&desc)
            .context("sky radiance")?;
        let cb = self
            .queue
            .commandBuffer()
            .context("sky prefilter command")?;
        for level in 0..9 {
            let pass = MTLRenderPassDescriptor::new();
            let c = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            c.setTexture(Some(&map));
            c.setLevel(level);
            c.setLoadAction(MTLLoadAction::DontCare);
            c.setStoreAction(MTLStoreAction::Store);
            let enc = cb
                .renderCommandEncoderWithDescriptor(&pass)
                .context("sky prefilter encoder")?;
            enc.setRenderPipelineState(&self.prefilter_pipeline);
            let rough = level as f32 / 8.;
            unsafe {
                enc.setFragmentBytes_length_atIndex(ptr(&rough), 4, 0);
                enc.setFragmentTexture_atIndex(Some(&self.surface_maps[4]), 0);
                enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
            }
            enc.endEncoding();
        }
        cb.commit();
        cb.waitUntilCompleted();
        if let Some(e) = cb.error() {
            anyhow::bail!("sky prefilter: {e}");
        }
        self.sky_reflection = map;
        Ok(())
    }
    pub fn enable_metalfx(&mut self, enabled: bool) -> Result<bool> {
        self.wait()?;
        self.metalfx_requested = enabled;
        self.metalfx = None;
        self.metalfx_output = None;
        self.reset_temporal_history();
        if !enabled || !self.temporal {
            return Ok(false);
        }
        unsafe {
            if !MTLFXTemporalScalerDescriptor::supportsDevice(&self.device) {
                eprintln!("MetalFX temporal unsupported; using custom TAA");
                return Ok(false);
            }
            let desc = MTLFXTemporalScalerDescriptor::new();
            desc.setInputWidth(self.hdr.width());
            desc.setInputHeight(self.hdr.height());
            desc.setOutputWidth(self.width as usize);
            desc.setOutputHeight(self.height as usize);
            desc.setColorTextureFormat(MTLPixelFormat::RGBA16Float);
            desc.setDepthTextureFormat(MTLPixelFormat::Depth32Float);
            desc.setMotionTextureFormat(MTLPixelFormat::RGBA16Float);
            desc.setOutputTextureFormat(MTLPixelFormat::RGBA16Float);
            desc.setAutoExposureEnabled(false);
            let Some(scaler) = desc.newTemporalScalerWithDevice(&self.device) else {
                eprintln!("MetalFX allocation unavailable; using custom TAA");
                return Ok(false);
            };
            // Respect MetalFX's required input usage flags instead of assuming shader-read only.
            let rt = MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead;
            self.hdr = texture(
                &self.device,
                self.hdr.width(),
                self.hdr.height(),
                MTLPixelFormat::RGBA16Float,
                1,
                rt | scaler.colorTextureUsage(),
            )?;
            self.motion = texture(
                &self.device,
                self.hdr.width(),
                self.hdr.height(),
                MTLPixelFormat::RGBA16Float,
                1,
                rt | scaler.motionTextureUsage(),
            )?;
            self.depth = texture(
                &self.device,
                self.hdr.width(),
                self.hdr.height(),
                MTLPixelFormat::Depth32Float,
                1,
                rt | scaler.depthTextureUsage(),
            )?;
            self.metalfx_output = Some(texture(
                &self.device,
                self.width as usize,
                self.height as usize,
                MTLPixelFormat::RGBA16Float,
                1,
                scaler.outputTextureUsage()
                    | MTLTextureUsage::ShaderRead
                    | MTLTextureUsage::ShaderWrite,
            )?);
            scaler.setInputContentWidth(self.hdr.width());
            scaler.setInputContentHeight(self.hdr.height());
            scaler.setDepthReversed(false);
            scaler.setPreExposure(1.);
            self.metalfx = Some(scaler);
        }
        eprintln!("TEMPORAL MetalFX");
        Ok(true)
    }
    fn blended(&self, d: &Draw) -> bool {
        let m = &d.model.primitives[d.primitive].material;
        m.alpha_mode == 2
            || (m.name == "__water_placeholder"
                && self.surface_uniform.flags[1] > 0
                && self.surface_uniform.water_deep[3] < 1.)
    }
    pub fn reset_temporal_history(&mut self) {
        self.history_valid = false;
        self.previous.clear();
    }
    pub fn set_frames_in_flight(&mut self, n: usize) -> Result<()> {
        ensure!((1..=3).contains(&n), "frames in flight must be 1..3");
        self.wait()?;
        let counters_enabled = self.counters_enabled;
        self.slots.clear();
        for _ in 0..n {
            self.slots.push(Slot {
                draws: self
                    .device
                    .newBufferWithLength_options(
                        INITIAL_RING_BYTES,
                        MTLResourceOptions::StorageModeShared,
                    )
                    .context("draw ring buffer")?,
                bones: self
                    .device
                    .newBufferWithLength_options(
                        INITIAL_RING_BYTES,
                        MTLResourceOptions::StorageModeShared,
                    )
                    .context("bone ring buffer")?,
                pending: None,
                counters: None,
                measured: GpuFrame::default(),
                pass_mask: 0,
                clock_start: (0, 0),
            });
        }
        self.set_gpu_counters(counters_enabled)?;
        Ok(())
    }
    pub fn attach(&mut self, view: &objc2_app_kit::NSView) {
        let layer = CAMetalLayer::layer();
        layer.setDevice(Some(&self.device));
        layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm_sRGB);
        layer.setFramebufferOnly(false);
        layer.setMaximumDrawableCount(3);
        layer.setDrawableSize(objc2_core_foundation::CGSize::new(
            self.width as f64,
            self.height as f64,
        ));
        view.setWantsLayer(true);
        view.setLayer(Some(&layer));
        self.layer = Some(layer);
    }
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        self.resize_at_scale(width, height, self.scale)
    }
    /// Resize only the internal targets; presentation and HUD stay at native resolution.
    pub fn set_render_scale(&mut self, scale: f32) -> Result<()> {
        ensure!(
            scale.is_finite() && (0.25..=1.).contains(&scale),
            "invalid render scale"
        );
        if self.scale == scale {
            return Ok(());
        }
        self.resize_at_scale(self.width, self.height, scale)
    }
    /// Moves the scale-dependent targets into two heaps sized for `max_scale`, so later
    /// `set_render_scale` calls up to it allocate nothing from the system. This costs one
    /// spare set of targets; call it only when the scale will change during play.
    pub fn reserve_render_scales(&mut self, max_scale: f32) -> Result<()> {
        ensure!(
            max_scale.is_finite() && (0.25..=1.).contains(&max_scale),
            "invalid render scale"
        );
        self.target_heaps = Some(TargetHeaps {
            max_scale,
            heaps: [None, None],
            current: 0,
        });
        self.post_targets = None;
        self.resize_at_scale(self.width, self.height, self.scale)
    }
    fn resize_at_scale(&mut self, width: u32, height: u32, scale: f32) -> Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }
        let output_changed = width != self.width || height != self.height;
        // Command buffers retain the textures they encode, so a scale-only change swaps
        // targets while earlier frames finish. Draining here stalled a whole GPU frame.
        if output_changed || self.metalfx_requested {
            self.wait()?;
        }
        let (w, h) = (
            (width as f32 * scale).round().max(1.) as usize,
            (height as f32 * scale).round().max(1.) as usize,
        );
        let descs = scaled_target_descs(w, h, self.samples);
        let heap = match &mut self.target_heaps {
            Some(heaps) => {
                let cap = heaps.max_scale.max(scale);
                let cap = (
                    (width as f32 * cap).round().max(1.) as usize,
                    (height as f32 * cap).round().max(1.) as usize,
                );
                let bytes = heap_bytes(
                    &self.device,
                    &scaled_target_descs(cap.0, cap.1, self.samples),
                );
                Some(heaps.spare(&self.device, bytes)?)
            }
            None => None,
        };
        let hdr = target(&self.device, heap, &descs[0])?;
        let msaa = target(&self.device, heap, &descs[1])?;
        let motion = target(&self.device, heap, &descs[2])?;
        let motion_msaa = target(&self.device, heap, &descs[3])?;
        let depth = target(&self.device, heap, &descs[4])?;
        let history = [
            target(&self.device, heap, &descs[5])?,
            target(&self.device, heap, &descs[6])?,
        ];
        let rt = MTLTextureUsage::RenderTarget;
        let read = MTLTextureUsage::ShaderRead;
        let output = if output_changed {
            Some(texture(
                &self.device,
                width as usize,
                height as usize,
                MTLPixelFormat::BGRA8Unorm_sRGB,
                1,
                rt | read,
            )?)
        } else {
            None
        };
        // All allocations have succeeded; in-flight frames keep their own references.
        self.hdr = hdr;
        self.msaa = msaa;
        self.motion = motion;
        self.motion_msaa = motion_msaa;
        self.depth = depth;
        self.history = history;
        if let Some(output) = output {
            self.output = output;
        }
        self.width = width;
        self.height = height;
        self.scale = scale;
        self.history_valid = false;
        self.previous.clear();
        if output_changed && let Some(l) = &self.layer {
            l.setDrawableSize(objc2_core_foundation::CGSize::new(
                width as f64,
                height as f64,
            ));
        }
        if self.metalfx_requested {
            self.enable_metalfx(true)?;
        }
        Ok(())
    }
    fn image(&mut self, t: &Texture, srgb: bool) -> Result<Obj<dyn MTLTexture>> {
        let key = (t.key.clone(), srgb);
        if let Some(texture) = self.textures.get(&key) {
            return Ok(texture.clone());
        }
        let texture = if let Some(other) = self.textures.get(&(t.key.clone(), !srgb)) {
            let format = if t.compressed.is_some() {
                if srgb {
                    MTLPixelFormat::ASTC_6x6_sRGB
                } else {
                    MTLPixelFormat::ASTC_6x6_LDR
                }
            } else if srgb {
                MTLPixelFormat::RGBA8Unorm_sRGB
            } else {
                MTLPixelFormat::RGBA8Unorm
            };
            other
                .newTextureViewWithPixelFormat(format)
                .context("texture colour-space view")?
        } else {
            image_texture(&self.device, &self.queue, t, srgb)?
        };
        self.textures.insert(key, texture.clone());
        Ok(texture)
    }
    fn upload(&mut self, draw: &Draw) -> Result<()> {
        if let Some(t) = &draw.base_map {
            self.image(t, true)?;
        }
        if let Some(dye) = &draw.dye {
            self.image(&dye.mask, false)?;
        }
        let key = (draw.model.id, draw.primitive);
        if self.meshes.contains_key(&key) {
            return Ok(());
        }
        let p = &draw.model.primitives[draw.primitive];
        let (shadow_words, shadow_layout) = shadow_stream(p);
        let mut maps = self.fallbacks.clone();
        for (i, map) in p.material.maps.iter().enumerate() {
            if let Some(t) = map {
                maps[i] = self.image(t, i == 0 || i == 3)?;
            }
        }
        let mut morphs = vec![[0f32; 4]; p.vertices.len() * 2];
        for (j, name) in ["arrow", "draw"].iter().enumerate() {
            if let Some(mi) = p.morph_names.iter().position(|n| n == name) {
                for (i, v) in p.morphs[mi].iter().enumerate() {
                    morphs[i * 2 + j] = [v[0], v[1], v[2], 0.];
                }
            }
        }
        self.meshes.insert(
            key,
            GpuMesh {
                vertices: buffer(&self.device, &p.vertices)?,
                indices: buffer(&self.device, &p.indices)?,
                morphs: buffer(&self.device, &morphs)?,
                count: p.indices.len(),
                maps,
                material: p.material.clone(),
                shadow_vertices: buffer(&self.device, &shadow_words)?,
                shadow_layout,
            },
        );
        Ok(())
    }
    pub fn draw(
        &mut self,
        camera: Camera,
        draws: &[Draw],
        capture: Option<&Path>,
    ) -> Result<Stats> {
        self.look.validate()?;
        let slot = self.frame % self.slots.len();
        self.complete_slot(slot)?;
        let hud = self.hud.take();
        if let Some(hud) = &hud {
            if !hud.textures.set.is_empty() {
                self.wait()?;
            }
            self.gui.upload(&self.device, hud)?;
        }
        let width = self.hdr.width() as f32;
        let height = self.hdr.height() as f32;
        let aspect = self.width as f32 / self.height as f32;
        let mut post = PostUniform::new(&self.look, camera, aspect);
        // Repeat a bounded sampling sequence; keep non-temporal AA stable between frames.
        post.projection[2] = if self.temporal || self.metalfx.is_some() {
            (self.frame % 64) as f32
        } else {
            0.
        };
        let post_config = [
            post.ao,
            post.bloom,
            [
                if post.bloom[3] > 0. {
                    post.projection[3]
                } else {
                    0.
                },
                0.,
                0.,
                0.,
            ],
        ];
        if self.post_config != post_config {
            // A toggle or changed look must not blend the old effect through temporal history.
            self.history_valid = false;
            self.post_config = post_config;
        }
        if post.active()
            && self.post_targets.as_ref().is_none_or(|p| {
                p.colour.width() != self.hdr.width()
                    || p.colour.height() != self.hdr.height()
                    || !p.colour.usage().contains(self.hdr.usage())
            })
        {
            // In-flight frames retain the old targets, as in `resize_at_scale`.
            self.post_targets = Some(PostTargets::new(
                &self.device,
                self.target_heaps.as_ref().and_then(TargetHeaps::current),
                self.hdr.width(),
                self.hdr.height(),
                self.hdr.usage(),
            )?);
        }
        let unjittered = camera.matrix(aspect);
        let jitter = if self.temporal {
            let phases = if self.metalfx.is_some() {
                (8. / (self.scale * self.scale)).ceil() as usize
            } else {
                16
            };
            let i = self.frame % phases + 1;
            Vec3::new(
                (halton(i, 2) - 0.5) * 2. / width,
                (halton(i, 3) - 0.5) * 2. / height,
                0.,
            )
        } else {
            Vec3::ZERO
        };
        let vp = Mat4::from_translation(jitter) * unjittered;
        let cut = self.previous_camera.is_none_or(|p| {
            p.eye.distance(camera.eye) > 5.
                || (p.target - p.eye)
                    .normalize()
                    .dot((camera.target - camera.eye).normalize())
                    < 0.75
        });
        let valid = self.history_valid && !cut;
        self.stats.history_resets += usize::from(!valid);
        let previous_vp = if valid { self.previous_vp } else { unjittered };
        let time = self
            .scene_time
            .unwrap_or_else(|| self.started.elapsed().as_secs_f32());
        let lighting = self.look.lighting_at(f64::from(time));
        let sun = Vec3::from_array(lighting.sun_direction).normalize();
        let mut cascades = stable_cascades(
            camera.eye,
            camera.target,
            camera.fov,
            aspect,
            sun,
            SHADOW_SIZE as u32,
        );
        let (far_matrix, refresh_far) =
            self.far_shadow_cache
                .plan(self.frame, cascades[2], SHADOW_SIZE as u32, !valid);
        // Sampling and caster culling must use the matrix that produced the stored layer.
        cascades[2] = far_matrix;
        let view = Frustum::new(unjittered);
        let f = Frame {
            vp: unjittered.to_cols_array_2d(),
            previous_vp: previous_vp.to_cols_array_2d(),
            inverse_vp: vp.inverse().to_cols_array_2d(),
            jitter: [jitter.x, jitter.y, jitter.x * 0.5, -jitter.y * 0.5],
            shadows: cascades.map(|m| m.to_cols_array_2d()),
            eye: camera.eye.extend(1.).to_array(),
            sun: sun.extend(lighting.sun_intensity).to_array(),
            sun_colour: Vec3::from_array(lighting.sun_colour).extend(1.).to_array(),
            ambient: Vec3::from_array(lighting.ambient).extend(1.).to_array(),
            // Combine the two authored factors once, retaining one multiply per sky sample.
            sky: Vec3::from_array(lighting.sky)
                .extend(lighting.sky_texture_intensity * self.surface_uniform.sky[1])
                .to_array(),
            fog: Vec3::from_array(lighting.fog_colour)
                .extend(lighting.fog_density)
                .to_array(),
            grade: self.look.grade.map(|r| [r[0], r[1], r[2], 0.]),
            params: [
                lighting.exposure,
                (self.look.tone_map == ToneMap::Aces) as u32 as f32,
                if self.metalfx.is_some() {
                    self.scale.log2()
                } else {
                    0.
                },
                0.,
            ],
            clock: [
                time,
                self.previous_time,
                self.look.wind_strength,
                valid as u32 as f32,
            ],
            viewport: [width, height, 1. / width, 1. / height],
            splits: [15., 50., 150., 1. / SHADOW_SIZE as f32],
        }
        .with_height_fog(self.look.height_fog);
        let shadow_views: [_; 3] = std::array::from_fn(|i| {
            ShadowCasterVolume::new(
                cascades[i],
                unjittered,
                camera.eye,
                camera.target - camera.eye,
                f.splits[i],
                SHADOW_SIZE as u32,
            )
        });
        let effects = self.effects.take();
        let fx = self
            .fx_pass
            .prepare(&self.device, slot, effects.as_ref(), camera.eye)?;
        // Size before writing, after this slot's fence. Buffers only grow and remain alive
        // until their submitted command buffer completes. Count each shared skin once.
        let mut unique = std::collections::HashSet::new();
        let bone_need = 256
            + draws
                .iter()
                .filter(|d| !d.palette.is_empty() && unique.insert(d.palette.as_ptr() as usize))
                .map(|d| ((std::mem::size_of_val(&*d.palette) + 255) & !255) * 2)
                .sum::<usize>();
        let stride = (std::mem::size_of::<DrawUniform>() + 255) & !255;
        // Base constants plus worst-case packed instances for scene and three shadows.
        let draw_need = draws.len().max(1) * stride * 5;
        let frame_slot = &mut self.slots[slot];
        for (buffer, need) in [
            (&mut frame_slot.bones, bone_need),
            (&mut frame_slot.draws, draw_need),
        ] {
            if buffer.length() < need {
                *buffer = self
                    .device
                    .newBufferWithLength_options(
                        need.next_power_of_two(),
                        MTLResourceOptions::StorageModeShared,
                    )
                    .context("grow frame ring")?;
            }
        }
        let bones = self.slots[slot].bones.clone();
        let uniform_buffer = self.slots[slot].draws.clone();
        let mut uniform_used = 0;
        // Rigid primitives share a single identity matrix at zero.
        unsafe {
            std::ptr::copy_nonoverlapping(
                Mat4::IDENTITY.to_cols_array().as_ptr().cast::<u8>(),
                bones.contents().as_ptr().cast::<u8>(),
                64,
            );
        }
        let mut used = 256;
        let mut palette_offsets = HashMap::new();
        let mut prepared = Vec::new();
        let mut new_previous = HashMap::new();
        let mut calls = 0;
        let mut triangles = 0;
        let mut culled = 0;
        for (i, d) in draws.iter().enumerate() {
            let p = &d.model.primitives[d.primitive];
            let mut bounds = p.bounds.transformed(d.transform);
            if !d.palette.is_empty() {
                bounds = bounds.padded(1.5);
            } else {
                bounds = bounds.padded(0.2 + self.look.wind_strength * 0.12);
            }
            let scene = view.intersects(bounds);
            let shadows = std::array::from_fn(|cascade| {
                shadow_views[cascade].intersects(bounds, sun, cascade > 0)
            });
            if !scene && !shadows.iter().any(|v| *v) {
                culled += 1;
                continue;
            }
            self.upload(d)?;
            let old = self.previous.get(&d.instance).filter(|old| {
                valid && old.model_key == d.model.id && old.palette.len() == d.palette.len()
            });
            let (current, previous) = if d.palette.is_empty() {
                (0, 0)
            } else {
                *palette_offsets
                    .entry(d.palette.as_ptr() as usize)
                    .or_insert_with(|| {
                        let current = &*d.palette;
                        let prev = old.map_or(current, |o| &*o.palette);
                        let mut put = |data: &[Mat4]| {
                            let size = std::mem::size_of_val(data);
                            let at = used;
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    data.as_ptr().cast::<u8>(),
                                    bones.contents().as_ptr().cast::<u8>().add(at),
                                    size,
                                );
                            }
                            used = (used + size + 255) & !255;
                            at
                        };
                        (put(current), put(prev))
                    })
            };
            let m = &p.material;
            let u = DrawUniform {
                model: d.transform.to_cols_array_2d(),
                previous_model: old.map_or(d.transform, |o| o.transform).to_cols_array_2d(),
                base: d.base,
                emission: [m.emissive[0], m.emissive[1], m.emissive[2], m.roughness],
                params: [
                    m.metallic,
                    m.normal_scale,
                    m.occlusion_strength,
                    m.alpha_cutoff,
                ],
                flags: [
                    d.hidden,
                    (!d.palette.is_empty()) as u32,
                    if self.blended(d) { 2 } else { m.alpha_mode },
                    match m.name.as_str() {
                        "__terrain_placeholder" => 1,
                        "__water_placeholder" => 2,
                        _ => 0,
                    },
                ],
                morph: [
                    d.morph[0],
                    d.morph[1],
                    (!p.morphs.is_empty()) as u32 as f32,
                    m.double_sided as u32 as f32,
                ],
                previous_morph: [
                    old.map_or(d.morph, |o| o.morph)[0],
                    old.map_or(d.morph, |o| o.morph)[1],
                    0.,
                    0.,
                ],
                dyes: d.dye.as_ref().map_or([[0.; 4]; 2], |dye| dye.colours),
                references: d.dye.as_ref().map_or([[1.; 4]; 2], |dye| dye.references),
            };

            ensure!(
                uniform_used + std::mem::size_of::<DrawUniform>() <= uniform_buffer.length(),
                "draw constant budget exceeded"
            );
            unsafe {
                std::ptr::copy_nonoverlapping(
                    ptr(&u).as_ptr().cast::<u8>(),
                    uniform_buffer
                        .contents()
                        .as_ptr()
                        .cast::<u8>()
                        .add(uniform_used),
                    std::mem::size_of::<DrawUniform>(),
                );
            }
            prepared.push(Prepared {
                uniform: uniform_used,
                draw: i,
                current,
                previous,
                bounds,
                scene,
                shadows,
            });
            uniform_used = (uniform_used + std::mem::size_of::<DrawUniform>() + 255) & !255;
            new_previous.insert(
                d.instance,
                Previous {
                    model_key: d.model.id,
                    transform: d.transform,
                    palette: d.palette.clone(),
                    morph: d.morph,
                },
            );
            if scene {
                triangles += p.indices.len() / 3;
            }
        }
        let cb = self.queue.commandBuffer().context("frame command buffer")?;
        let counters = self.slots[slot]
            .counters
            .as_ref()
            .filter(|_| self.sample_gpu_counters);
        let clock_start = counters
            .map(|_| clock_pair(&self.device))
            .unwrap_or_default();
        let submitted_at = self.started.elapsed().as_secs_f64();
        let mut pass_mask = 0u16;
        let mut skipped_pass_mask = 0u16;
        for cascade in 0..3 {
            if cascade == 2 && !refresh_far {
                skipped_pass_mask |= 1 << cascade;
                // Preserve the layer without an encoder, clear, or stale timestamp samples.
                continue;
            }
            let pass = MTLRenderPassDescriptor::new();
            if let Some(c) = counters {
                c.attach(&pass, cascade);
                pass_mask |= 1 << cascade;
            }
            let depth = pass.depthAttachment();
            depth.setTexture(Some(&self.shadow));
            depth.setSlice(cascade);
            depth.setLoadAction(MTLLoadAction::Clear);
            depth.setStoreAction(MTLStoreAction::Store);
            depth.setClearDepth(1.);
            let enc = cb
                .renderCommandEncoderWithDescriptor(&pass)
                .context("cascade encoder")?;
            enc.setDepthStencilState(Some(&self.depth_state));
            enc.setDepthBias_slopeScale_clamp(0.001, 1.25, 0.01);
            calls += self.encode(
                &enc,
                draws,
                &prepared,
                (&bones, &uniform_buffer, &mut uniform_used),
                &f,
                DrawPass::Shadow(cascade),
            )?;
            enc.endEncoding();
            if cascade == 2 {
                self.far_shadow_cache.encoded(self.frame, far_matrix);
            }
        }
        let split_scene = self.profile_scene && counters.is_some();
        // MSAA depth cannot be stored/reloaded as a resolved depth texture here.
        ensure!(
            !split_scene || self.samples == 1,
            "split scene profiling requires single-sample rendering"
        );
        for scene_part in 0..if split_scene { 3 } else { 1 } {
            let first_scene_pass = scene_part == 0;
            let pass_index = if split_scene { 7 + scene_part } else { 3 };
            let pass = MTLRenderPassDescriptor::new();
            if let Some(c) = counters {
                c.attach(&pass, pass_index);
                pass_mask |= 1 << pass_index;
            }
            for i in 0..2 {
                let c = unsafe { pass.colorAttachments().objectAtIndexedSubscript(i) };
                let single = if i == 0 { &self.hdr } else { &self.motion };
                let multi = if i == 0 {
                    &self.msaa
                } else {
                    &self.motion_msaa
                };
                c.setTexture(Some(if self.samples == 1 { single } else { multi }));
                if self.samples > 1 {
                    c.setResolveTexture(Some(single));
                }
                c.setLoadAction(if first_scene_pass {
                    MTLLoadAction::Clear
                } else {
                    MTLLoadAction::Load
                });
                c.setStoreAction(if self.samples > 1 {
                    MTLStoreAction::MultisampleResolve
                } else {
                    MTLStoreAction::Store
                });
                c.setClearColor(if i == 0 {
                    MTLClearColor {
                        red: self.look.sky[0] as f64,
                        green: self.look.sky[1] as f64,
                        blue: self.look.sky[2] as f64,
                        alpha: 1.,
                    }
                } else {
                    MTLClearColor {
                        red: 0.,
                        green: 0.,
                        blue: 800.,
                        alpha: 0.,
                    }
                });
            }
            pass.depthAttachment().setTexture(Some(&self.depth));
            pass.depthAttachment().setClearDepth(1.);
            pass.depthAttachment().setLoadAction(if first_scene_pass {
                MTLLoadAction::Clear
            } else {
                MTLLoadAction::Load
            });
            pass.depthAttachment().setStoreAction(MTLStoreAction::Store);
            let enc = cb
                .renderCommandEncoderWithDescriptor(&pass)
                .context("scene encoder")?;
            if first_scene_pass && self.surface_uniform.flags[2] > 0 {
                enc.setRenderPipelineState(&self.sky_pipeline);
                enc.setDepthStencilState(Some(&self.read_depth));
                unsafe {
                    enc.setFragmentBytes_length_atIndex(ptr(&f), std::mem::size_of::<Frame>(), 1);
                    enc.setFragmentBytes_length_atIndex(
                        ptr(&self.surface_uniform),
                        std::mem::size_of::<crate::surfaces::Uniform>(),
                        3,
                    );
                    enc.setFragmentTexture_atIndex(Some(&self.surface_maps[4]), 11);
                    enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
                }
                calls += 1;
            }

            if !split_scene || scene_part != 0 {
                calls += self.encode(
                    &enc,
                    draws,
                    &prepared,
                    (&bones, &uniform_buffer, &mut uniform_used),
                    &f,
                    if !split_scene {
                        DrawPass::Scene
                    } else if scene_part == 1 {
                        DrawPass::Opaque
                    } else {
                        DrawPass::Transparent
                    },
                )?;
            }
            // Ground marks go last in the scene, over whatever they lie on.
            if !split_scene || scene_part == 2 {
                calls += self.fx_pass.decals(&enc, &fx, &f);
            }
            enc.endEncoding();
        }
        let scene_colour = if post.active() {
            calls += self.encode_post(&cb, &f, &post, counters, &mut pass_mask)?;
            &self
                .post_targets
                .as_ref()
                .expect("active post targets")
                .colour
        } else {
            &self.hdr
        };
        let history_index = self.frame % 2;
        if let Some(scaler) = &self.metalfx {
            unsafe {
                scaler.setColorTexture(Some(scene_colour));
                scaler.setDepthTexture(Some(&self.depth));
                scaler.setMotionTexture(Some(&self.motion));
                scaler.setOutputTexture(self.metalfx_output.as_deref());
                // Our vectors are currentUV - previousUV. MetalFX expects previous - current in pixels.
                scaler.setMotionVectorScaleX(-width);
                scaler.setMotionVectorScaleY(-height);
                scaler.setJitterOffsetX(jitter.x * width * 0.5);
                scaler.setJitterOffsetY(-jitter.y * height * 0.5);
                scaler.setReset(!valid);
                scaler.encodeToCommandBuffer(&cb);
            }
        } else if self.temporal {
            let pass = MTLRenderPassDescriptor::new();
            if let Some(c) = counters {
                c.attach(&pass, 4);
                pass_mask |= 1 << 4;
            }
            let c = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            c.setTexture(Some(&self.history[history_index]));
            c.setLoadAction(MTLLoadAction::DontCare);
            c.setStoreAction(MTLStoreAction::Store);
            let enc = cb
                .renderCommandEncoderWithDescriptor(&pass)
                .context("temporal encoder")?;
            enc.setRenderPipelineState(&self.taa);
            unsafe {
                enc.setFragmentBytes_length_atIndex(ptr(&f), std::mem::size_of::<Frame>(), 1);
                enc.setFragmentTexture_atIndex(Some(scene_colour), 0);
                enc.setFragmentTexture_atIndex(Some(&self.motion), 1);
                enc.setFragmentTexture_atIndex(Some(&self.depth), 2);
                enc.setFragmentTexture_atIndex(Some(&self.history[1 - history_index]), 3);
                enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
            }
            enc.endEncoding();
            calls += 1;
        }
        let drawable = self.layer.as_ref().and_then(|l| l.nextDrawable());
        let target = if self.direct_drawable && capture.is_none() {
            drawable
                .as_ref()
                .map(|d| d.texture())
                .unwrap_or_else(|| self.output.clone())
        } else {
            self.output.clone()
        };
        let output = MTLRenderPassDescriptor::new();
        if let Some(c) = counters {
            c.attach(&output, 5);
            pass_mask |= 1 << 5;
        }
        let c = unsafe { output.colorAttachments().objectAtIndexedSubscript(0) };
        c.setTexture(Some(&target));
        c.setLoadAction(MTLLoadAction::DontCare);
        c.setStoreAction(MTLStoreAction::Store);
        let enc = cb
            .renderCommandEncoderWithDescriptor(&output)
            .context("composite encoder")?;
        enc.setRenderPipelineState(&self.composite);
        unsafe {
            enc.setFragmentBytes_length_atIndex(ptr(&f), std::mem::size_of::<Frame>(), 1);
            enc.setFragmentTexture_atIndex(
                Some(if let Some(output) = &self.metalfx_output {
                    output
                } else if self.temporal {
                    &self.history[history_index]
                } else {
                    scene_colour
                }),
                0,
            );
            enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
        }
        calls += 1;
        calls += self.fx_pass.overlay(
            &enc,
            &fx,
            &f,
            &self.depth,
            (self.width, self.height),
            camera.eye,
            camera.target,
        );
        enc.endEncoding();
        if let Some(hud) = &hud {
            let hud_calls = self.gui.paint(&cb, &target, hud, self.frame, counters)?;
            calls += hud_calls;
            if counters.is_some() && hud_calls > 0 {
                pass_mask |= 1 << 6;
            }
        }
        if let Some(drawable) = &drawable {
            if !self.direct_drawable || capture.is_some() {
                let blit = cb.blitCommandEncoder().context("present blit")?;
                unsafe {
                    blit.copyFromTexture_toTexture(&self.output, &drawable.texture());
                }
                blit.endEncoding();
            }
            let samples = self.presented.clone();
            let number = self.frame;
            let handler =
                block2::RcBlock::new(move |drawable: NonNull<ProtocolObject<dyn MTLDrawable>>| {
                    let t = unsafe { drawable.as_ref() }.presentedTime();
                    let mut times = samples.lock().unwrap();
                    if times.len() >= 16384 {
                        times.drain(..8192);
                    }
                    times.push((number, t));
                });
            let base: &ProtocolObject<dyn MTLDrawable> = ProtocolObject::from_ref(&**drawable);
            unsafe {
                base.addPresentedHandler(block2::RcBlock::as_ptr(&handler) as *mut _);
            }
            cb.presentDrawable_afterMinimumDuration(base, 1. / self.target_fps);
        }
        let row = (self.width as usize * 4 + 255) & !255;
        let readback = if capture.is_some() {
            let b = self
                .device
                .newBufferWithLength_options(
                    row * self.height as usize,
                    MTLResourceOptions::StorageModeShared,
                )
                .context("capture readback")?;
            let blit = cb.blitCommandEncoder().context("capture encoder")?;
            unsafe {
                blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(&self.output,0,0,MTLOrigin{x:0,y:0,z:0},MTLSize{width:self.width as usize,height:self.height as usize,depth:1},&b,0,row,row*self.height as usize);
            }
            blit.endEncoding();
            Some(b)
        } else {
            None
        };
        cb.commit();
        if let (Some(path), Some(b)) = (capture, readback) {
            cb.waitUntilCompleted();
            if let Some(e) = cb.error() {
                anyhow::bail!("GPU capture failed: {e}")
            }
            let src = unsafe {
                std::slice::from_raw_parts(
                    b.contents().as_ptr().cast::<u8>(),
                    row * self.height as usize,
                )
            };
            let mut rgba = Vec::with_capacity(self.width as usize * self.height as usize * 4);
            for y in 0..self.height as usize {
                for p in src[y * row..y * row + self.width as usize * 4]
                    .as_chunks::<4>()
                    .0
                {
                    rgba.extend([p[2], p[1], p[0], p[3]]);
                }
            }
            image::save_buffer(
                path,
                &rgba,
                self.width,
                self.height,
                image::ColorType::Rgba8,
            )?;
            self.stats.gpu_ms = (cb.GPUEndTime() - cb.GPUStartTime()) * 1000.;
        }
        self.slots[slot].pending = Some(cb);
        self.slots[slot].measured = GpuFrame {
            frame: self.frame,
            counters_sampled: pass_mask != 0,
            skipped_pass_mask,
            elapsed_seconds: submitted_at,
            draw_calls: calls,
            visible_triangles: triangles,
            allocated_gpu_bytes: self.device.currentAllocatedSize(),
            ..Default::default()
        };
        self.slots[slot].pass_mask = pass_mask;
        self.slots[slot].clock_start = clock_start;
        self.previous = new_previous;
        self.previous_vp = unjittered;
        self.previous_camera = Some(camera);
        self.previous_time = time;
        self.history_valid = true;
        self.frame += 1;
        self.stats.draw_calls = calls;
        self.stats.triangles = triangles;
        self.stats.culled = culled;
        self.stats.bone_bytes = used;
        self.stats.bone_capacity = bones.length();
        Ok(self.stats)
    }
    /// Each post pass has its own stage counters. The final application is full internal
    /// resolution; only AO evaluation and bloom filtering use the half-resolution targets.
    fn encode_post(
        &self,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        frame: &Frame,
        post: &PostUniform,
        counters: Option<&PassCounters>,
        mask: &mut u16,
    ) -> Result<usize> {
        let targets = self.post_targets.as_ref().expect("active post targets");
        let mut calls = 0;
        let mut run = |index: usize,
                       pipeline: usize,
                       target: &Obj<dyn MTLTexture>,
                       source: &Obj<dyn MTLTexture>,
                       uniforms: &PostUniform|
         -> Result<()> {
            let pass = MTLRenderPassDescriptor::new();
            if let Some(c) = counters {
                c.attach(&pass, index);
                *mask |= 1 << index;
            }
            let colour = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            colour.setTexture(Some(target));
            colour.setLoadAction(MTLLoadAction::DontCare);
            colour.setStoreAction(MTLStoreAction::Store);
            let enc = cb
                .renderCommandEncoderWithDescriptor(&pass)
                .context("post encoder")?;
            enc.setLabel(Some(&NSString::from_str(GPU_PASS_NAMES[index])));
            enc.setRenderPipelineState(&self.post_pipelines[pipeline]);
            unsafe {
                enc.setFragmentBytes_length_atIndex(
                    ptr(uniforms),
                    std::mem::size_of::<PostUniform>(),
                    0,
                );
                enc.setFragmentBytes_length_atIndex(ptr(frame), std::mem::size_of::<Frame>(), 1);
                enc.setFragmentTexture_atIndex(Some(source), 0);
                enc.setFragmentTexture_atIndex(Some(&self.depth), 1);
                // Bind the effect targets only when applying, never while rendering into them.
                if pipeline == 3 {
                    enc.setFragmentTexture_atIndex(
                        Some(if post.ao[3] > 0. {
                            &targets.ao
                        } else {
                            &self.fallbacks[0]
                        }),
                        2,
                    );
                    enc.setFragmentTexture_atIndex(
                        Some(if post.bloom[3] > 0. {
                            &targets.bloom[2]
                        } else {
                            &self.fallbacks[0]
                        }),
                        3,
                    );
                }
                enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
            }
            enc.endEncoding();
            calls += 1;
            Ok(())
        };
        if post.ao[3] > 0. {
            run(10, 0, &targets.ao, &self.hdr, post)?;
        }
        if post.bloom[3] > 0. {
            run(11, 1, &targets.bloom[0], &self.hdr, post)?;
            let mut blur = *post;
            blur.blur = [1., 0., 0., 0.];
            run(12, 2, &targets.bloom[1], &targets.bloom[0], &blur)?;
            blur.blur = [0., 1., 0., 0.];
            run(13, 2, &targets.bloom[2], &targets.bloom[1], &blur)?;
        }
        run(14, 3, &targets.colour, &self.hdr, post)?;
        Ok(calls)
    }
    fn encode(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        draws: &[Draw],
        prepared: &[Prepared],
        buffers: (
            &ProtocolObject<dyn MTLBuffer>,
            &ProtocolObject<dyn MTLBuffer>,
            &mut usize,
        ),
        f: &Frame,
        selection: DrawPass,
    ) -> Result<usize> {
        let cascade = match selection {
            DrawPass::Shadow(i) => Some(i),
            _ => None,
        };
        let (bones, uniform_buffer, packed_used) = buffers;
        unsafe {
            enc.setVertexBytes_length_atIndex(ptr(f), std::mem::size_of::<Frame>(), 1);
            if let Some(c) = cascade {
                let index = c as u32;
                enc.setVertexBytes_length_atIndex(ptr(&index), 4, 5);
            } else {
                enc.setFragmentBytes_length_atIndex(ptr(f), std::mem::size_of::<Frame>(), 1);
                enc.setFragmentTexture_atIndex(Some(&self.shadow), 5);
                enc.setFragmentBytes_length_atIndex(
                    ptr(&self.surface_uniform),
                    std::mem::size_of::<crate::surfaces::Uniform>(),
                    3,
                );
                enc.setFragmentTexture_atIndex(Some(&self.sky_reflection), 12);
                for (i, texture) in self.surface_maps.iter().enumerate() {
                    enc.setFragmentTexture_atIndex(Some(texture), 7 + i);
                }
            }
        }
        let mut count = 0;
        let mut visible: Vec<_> = prepared
            .iter()
            .filter(|p| cascade.map_or(p.scene, |c| p.shadows[c]))
            .filter(|p| match selection {
                DrawPass::Opaque => !self.blended(&draws[p.draw]),
                DrawPass::Transparent => self.blended(&draws[p.draw]),
                _ => true,
            })
            .collect();
        // Repeated opaque rigid meshes share an instanced draw. Each instance keeps its
        // own current/previous transforms (including spinning scenery) in packed constants.
        let mut groups: Vec<Vec<&Prepared>> = Vec::new();
        let mut rigid = HashMap::new();
        for p in visible.drain(..) {
            let d = &draws[p.draw];
            let batchable =
                d.palette.is_empty() && !self.blended(d) && d.dye.is_none() && d.base_map.is_none();
            if batchable {
                let index = *rigid.entry((d.model.id, d.primitive)).or_insert_with(|| {
                    groups.push(Vec::new());
                    groups.len() - 1
                });
                groups[index].push(p);
            } else {
                groups.push(vec![p]);
            }
        }
        let eye = Vec3::from_slice(&f.eye[..3]);
        let distance = |group: &Vec<&Prepared>| {
            group
                .iter()
                .map(|p| p.bounds.center.distance_squared(eye))
                .fold(f32::INFINITY, f32::min)
        };
        // Opaque batches front-to-back; transparent objects remain individual and back-to-front.
        groups.sort_by(|a, b| {
            let ba = self.blended(&draws[a[0].draw]);
            let bb = self.blended(&draws[b[0].draw]);
            ba.cmp(&bb).then_with(|| {
                if ba {
                    distance(b).total_cmp(&distance(a))
                } else {
                    distance(a).total_cmp(&distance(b))
                }
            })
        });
        let mut last_pipeline = None;
        let mut last_cull = None;
        for group in groups {
            let p = group[0];
            let d = &draws[p.draw];
            let g = &self.meshes[&(d.model.id, d.primitive)];
            let m = &g.material;
            let blended = self.blended(d);
            if cascade.is_some() && (blended || m.name == "__water_placeholder") {
                continue;
            }
            if cascade.is_some() {
                // Every instance in a batch shares its material. Blended draws were
                // excluded above, so this matches DrawUniform.flags.z == 1 exactly.
                let masked = m.alpha_mode == 1;
                if last_pipeline != Some(masked) {
                    enc.setRenderPipelineState(if masked {
                        &self.shadow_mask_pipeline
                    } else {
                        &self.shadow_pipeline
                    });
                    last_pipeline = Some(masked);
                }
            }
            if cascade.is_none() && last_pipeline != Some(blended) {
                enc.setRenderPipelineState(if blended { &self.blend } else { &self.opaque });
                enc.setDepthStencilState(Some(if blended {
                    &self.read_depth
                } else {
                    &self.depth_state
                }));
                last_pipeline = Some(blended);
            }
            let cull = if m.double_sided || d.model.key.starts_with("world:") {
                MTLCullMode::None
            } else {
                MTLCullMode::Back
            };
            if last_cull != Some(cull) {
                enc.setCullMode(cull);
                last_cull = Some(cull);
            }
            enc.setFrontFacingWinding(MTLWinding::CounterClockwise);
            let instanced = group.len() > 1;
            let uniform = if instanced {
                let at = *packed_used;
                let stride = std::mem::size_of::<DrawUniform>();
                ensure!(
                    at + group.len() * stride <= uniform_buffer.length(),
                    "instanced draw constant budget exceeded"
                );
                unsafe {
                    let bytes = uniform_buffer.contents().as_ptr().cast::<u8>();
                    for (i, instance) in group.iter().enumerate() {
                        std::ptr::copy_nonoverlapping(
                            bytes.add(instance.uniform),
                            bytes.add(at + i * stride),
                            stride,
                        );
                    }
                }
                *packed_used = (at + group.len() * stride + 255) & !255;
                at
            } else {
                p.uniform
            };
            unsafe {
                let compact_shadow = cascade.is_some();
                enc.setVertexBuffer_offset_atIndex(
                    Some(if compact_shadow {
                        &g.shadow_vertices
                    } else {
                        &g.vertices
                    }),
                    0,
                    0,
                );
                if compact_shadow {
                    enc.setVertexBytes_length_atIndex(ptr(&g.shadow_layout), 8, 7);
                }
                if self.inline_constants && !instanced {
                    enc.setVertexBytes_length_atIndex(
                        NonNull::new(
                            uniform_buffer
                                .contents()
                                .as_ptr()
                                .cast::<u8>()
                                .add(uniform)
                                .cast::<c_void>(),
                        )
                        .unwrap(),
                        std::mem::size_of::<DrawUniform>(),
                        2,
                    );
                } else {
                    enc.setVertexBuffer_offset_atIndex(Some(uniform_buffer), uniform, 2);
                }
                enc.setVertexBuffer_offset_atIndex(Some(bones), p.current, 3);
                enc.setVertexBuffer_offset_atIndex(Some(&g.morphs), 0, 4);
                enc.setVertexBuffer_offset_atIndex(Some(bones), p.previous, 6);
                if self.inline_constants && !instanced {
                    enc.setFragmentBytes_length_atIndex(
                        NonNull::new(
                            uniform_buffer
                                .contents()
                                .as_ptr()
                                .cast::<u8>()
                                .add(uniform)
                                .cast::<c_void>(),
                        )
                        .unwrap(),
                        std::mem::size_of::<DrawUniform>(),
                        2,
                    );
                } else {
                    enc.setFragmentBuffer_offset_atIndex(Some(uniform_buffer), uniform, 2);
                }
                for (j, t) in g.maps.iter().enumerate() {
                    if cascade.is_some() && j > 0 {
                        break;
                    }
                    let t = if j == 0 {
                        d.base_map
                            .as_ref()
                            .and_then(|t| self.textures.get(&(t.key.clone(), true)))
                            .unwrap_or(t)
                    } else {
                        t
                    };
                    enc.setFragmentTexture_atIndex(Some(t), j);
                }
                if cascade.is_none() {
                    let mask = d
                        .dye
                        .as_ref()
                        .and_then(|dye| self.textures.get(&(dye.mask.key.clone(), false)))
                        .unwrap_or(&self.fallbacks[4]);
                    enc.setFragmentTexture_atIndex(Some(mask), 6);
                }
                enc.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount(
                    MTLPrimitiveType::Triangle,
                    g.count,
                    MTLIndexType::UInt32,
                    &g.indices,
                    0,
                    group.len(),
                );
            }
            count += 1;
        }
        Ok(count)
    }
    /// The number the next drawn frame will carry (as `presented_at` takes it).
    pub fn next_frame(&self) -> usize {
        self.frame
    }
    /// When frame `frame` reached the screen, on the media clock, if it has.
    pub fn presented_at(&self, frame: usize) -> Option<f64> {
        let samples = self.presented.lock().unwrap();
        samples
            .iter()
            .rev()
            .take(64)
            .find(|(f, t)| *f == frame && *t > 0.)
            .map(|(_, t)| *t)
    }
    pub fn presentation_times(&self, warmup: usize) -> Vec<f64> {
        let mut samples = self.presented.lock().unwrap().clone();
        samples.sort_by_key(|(frame, _)| *frame);
        samples
            .into_iter()
            .filter(|(frame, t)| *frame >= warmup && *t > 0.)
            .map(|(_, t)| t)
            .collect()
    }
    /// Counters are optional; unsupported devices return a descriptive error without changing rendering.
    pub fn set_gpu_counters(&mut self, enabled: bool) -> Result<()> {
        if enabled == self.counters_enabled
            && self.slots.iter().all(|s| s.counters.is_some() == enabled)
        {
            return Ok(());
        }
        self.wait()?;
        let buffers = if enabled {
            (0..self.slots.len())
                .map(|_| PassCounters::new(&self.device))
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let mut buffers = buffers.into_iter();
        for slot in &mut self.slots {
            slot.counters = buffers.next();
        }
        self.counters_enabled = enabled;
        Ok(())
    }
    pub fn gpu_counters_enabled(&self) -> bool {
        self.counters_enabled
    }
    pub fn take_completed_frames(&mut self) -> impl Iterator<Item = GpuFrame> + '_ {
        self.completed_frames.drain(..)
    }
    fn complete_slot(&mut self, index: usize) -> Result<()> {
        let slot = &mut self.slots[index];
        let Some(cb) = slot.pending.take() else {
            return Ok(());
        };
        cb.waitUntilCompleted();
        if let Some(e) = cb.error() {
            anyhow::bail!("GPU command failed: {e}");
        }
        let mut measured = slot.measured;
        measured.gpu_ms = (cb.GPUEndTime() - cb.GPUStartTime()) * 1000.;
        if slot.pass_mask != 0
            && let Some(counters) = &slot.counters
        {
            measured.passes =
                counters.resolve(slot.pass_mask, slot.clock_start, clock_pair(&self.device));
        }
        if self
            .stats
            .completed
            .is_none_or(|previous| previous.frame <= measured.frame)
        {
            self.stats.gpu_ms = measured.gpu_ms;
            self.stats.completed = Some(measured);
        }
        if self.completed_frames.len() == 64 {
            self.completed_frames.pop_front();
        }
        self.completed_frames.push_back(measured);
        Ok(())
    }
    /// The newest frame ID handed to the GPU, which completion samples report as `frame`.
    pub fn last_submitted_frame(&self) -> Option<usize> {
        self.frame.checked_sub(1)
    }
    pub fn wait(&mut self) -> Result<()> {
        // Slots may wrap, so completion consumers identify samples by submitted frame number.
        for i in 0..self.slots.len() {
            self.complete_slot(i)?;
        }
        Ok(())
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        let _ = self.wait();
    }
}
fn halton(mut i: usize, base: usize) -> f32 {
    let (mut f, mut value) = (1f32, 0f32);
    while i > 0 {
        f /= base as f32;
        value += f * (i % base) as f32;
        i /= base;
    }
    value
}

fn texture_array(
    device: &ProtocolObject<dyn MTLDevice>,
    queue: &ProtocolObject<dyn MTLCommandQueue>,
    sources: &[Obj<dyn MTLTexture>],
) -> Result<Obj<dyn MTLTexture>> {
    ensure!(
        !sources.is_empty() && sources.len() <= 4,
        "terrain requires 1..4 supplied layers"
    );
    let first = &sources[0];
    ensure!(
        sources.iter().all(|t| t.width() == first.width()
            && t.height() == first.height()
            && t.pixelFormat() == first.pixelFormat()
            && t.mipmapLevelCount() == first.mipmapLevelCount()),
        "terrain array layers must share size, format and mip count"
    );
    let d = MTLTextureDescriptor::new();
    unsafe {
        d.setWidth(first.width());
        d.setHeight(first.height());
        d.setArrayLength(sources.len());
        d.setMipmapLevelCount(first.mipmapLevelCount());
    }
    d.setTextureType(MTLTextureType::Type2DArray);
    d.setPixelFormat(first.pixelFormat());
    d.setStorageMode(MTLStorageMode::Private);
    d.setUsage(MTLTextureUsage::ShaderRead);
    let array = device
        .newTextureWithDescriptor(&d)
        .context("terrain texture array")?;
    let cb = queue.commandBuffer().context("terrain upload")?;
    let blit = cb.blitCommandEncoder().context("terrain upload encoder")?;
    for (slice, source) in sources.iter().enumerate() {
        for level in 0..first.mipmapLevelCount() {
            unsafe {
                blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(source,0,level,MTLOrigin{x:0,y:0,z:0},MTLSize{width:(first.width()>>level).max(1),height:(first.height()>>level).max(1),depth:1},&array,slice,level,MTLOrigin{x:0,y:0,z:0});
            }
        }
    }
    blit.endEncoding();
    cb.commit();
    Ok(array)
}

#[cfg(test)]
mod temporal_tests {
    use super::*;
    #[test]
    fn frame_packs_height_fog_without_changing_other_channels() {
        use crate::settings::HeightFogSettings;
        let mut original: Frame = bytemuck::Zeroable::zeroed();
        original.grade = [[1., 2., 3., 4.], [5., 6., 7., 8.], [9., 10., 11., 12.]];
        original.params = [13., 14., 15., 16.];
        for enabled in [false, true] {
            let fog = HeightFogSettings {
                enabled,
                base_height: -12.,
                height_falloff: 0.25,
                sky_distance: 800.,
            };
            let mut packed = original.with_height_fog(fog);
            assert_eq!(
                [
                    packed.grade[0][3],
                    packed.grade[1][3],
                    packed.grade[2][3],
                    packed.params[3]
                ],
                fog.uniform()
            );
            for (row, before) in packed.grade.iter_mut().zip(original.grade) {
                row[3] = before[3];
            }
            packed.params[3] = original.params[3];
            assert_eq!(bytemuck::bytes_of(&packed), bytemuck::bytes_of(&original));
        }
    }

    /// Hosted macOS uses a paravirtual GPU. Verify the real fallback there; never
    /// swallow allocation/sampling failures on hardware that advertises support.
    fn counter_device_or_verify_fallback(r: &mut Renderer) -> bool {
        if r.device
            .supportsCounterSampling(MTLCounterSamplingPoint::AtStageBoundary)
        {
            return true;
        }
        assert!(r.set_gpu_counters(true).is_err());
        assert!(!r.gpu_counters_enabled());
        let camera = Camera {
            eye: Vec3::new(0., 20., 20.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        r.draw(camera, &[], None).unwrap();
        r.wait().unwrap();
        let frame = r.take_completed_frames().next().unwrap();
        assert!(!frame.counters_sampled);
        assert!(frame.passes.iter().all(Option::is_none));
        assert!(
            frame.draw_calls > 0,
            "rendering must continue without counters"
        );
        eprintln!(
            "Verified unavailable-counter fallback on {}; timestamp and split-pass assertions require a physical counter-capable GPU",
            r.device.name()
        );
        false
    }

    #[test]
    fn resolution_changes_keep_in_flight_frames_preserve_native_output_and_reset_history() {
        let mut r = Renderer::new(64, 64, 0.75).unwrap();
        r.set_frames_in_flight(3).unwrap();
        let camera = Camera {
            eye: Vec3::new(0., 20., 20.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        r.draw(camera, &[], None).unwrap();
        r.draw(camera, &[], None).unwrap();
        assert!(r.slots.iter().any(|slot| slot.pending.is_some()));
        let output = r.output.clone();
        r.set_render_scale(0.55).unwrap();
        assert!(
            r.slots.iter().any(|slot| slot.pending.is_some()),
            "a scale-only change must not drain in-flight frames"
        );
        assert_eq!(r.last_submitted_frame(), Some(1));
        assert_eq!((r.hdr.width(), r.hdr.height()), (35, 35));
        assert_eq!((r.history[0].width(), r.history[1].height()), (35, 35));
        assert_eq!((r.output.width(), r.output.height()), (64, 64));
        assert!(std::ptr::eq(&*r.output, &*output));
        assert!(!r.history_valid);
        r.draw(camera, &[], None).unwrap();
        r.wait().unwrap();
        assert!(r.history_valid);
        let history = r.history[0].clone();
        r.set_render_scale(0.55).unwrap();
        assert!(std::ptr::eq(&*r.history[0], &*history));
        assert!(r.history_valid, "unchanged scale must not discard history");
        assert!(r.set_render_scale(f32::NAN).is_err());
        assert!(r.set_render_scale(0.1).is_err());
        assert_eq!(r.scale, 0.55);
        r.resize(96, 80).unwrap();
        assert_eq!((r.hdr.width(), r.hdr.height()), (53, 44));
        assert_eq!((r.output.width(), r.output.height()), (96, 80));
        assert!(!r.history_valid);
        r.draw(camera, &[], None).unwrap();
        r.wait().unwrap();
    }

    #[test]
    fn reserved_render_scales_step_between_two_heaps_without_growing_them() {
        let mut r = Renderer::new(64, 64, 0.75).unwrap();
        r.set_frames_in_flight(3).unwrap();
        r.reserve_render_scales(0.75).unwrap();
        let camera = Camera {
            eye: Vec3::new(0., 20., 20.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        let sizes = |r: &Renderer| {
            let heaps = &r.target_heaps.as_ref().unwrap().heaps;
            heaps.each_ref().map(|h| h.as_ref().unwrap().size())
        };
        let reserved = sizes(&r);
        let mut previous = None;
        for scale in [0.75, 0.725, 0.7, 0.725, 0.75, 0.55] {
            r.set_render_scale(scale).unwrap();
            // As in the client, each frame drains its autorelease pool, which releases the
            // passes' last references to the previous targets.
            objc2::rc::autoreleasepool(|_| r.draw(camera, &[], None)).unwrap();
            // Steps are a second apart, so frames that used the other heap have finished.
            r.wait().unwrap();
            let heaps = r.target_heaps.as_ref().unwrap();
            let current = heaps.current().unwrap();
            let mut targets = vec![&r.hdr, &r.msaa, &r.motion, &r.motion_msaa, &r.depth];
            targets.extend(&r.history);
            if let Some(post) = &r.post_targets {
                targets.extend([&post.ao, &post.colour]);
                targets.extend(&post.bloom);
            }
            for t in targets {
                let heap = t.heap().expect("scale-dependent target outside the heaps");
                assert!(std::ptr::eq(&*heap, current));
            }
            if let Some(previous) = previous {
                assert_ne!(previous, heaps.current, "each step uses the other heap");
            }
            previous = Some(heaps.current);
            assert_eq!(sizes(&r), reserved);
        }
        // A larger output grows both heaps once; later steps fit again.
        r.resize(96, 80).unwrap();
        let grown = sizes(&r);
        assert!(grown.iter().zip(reserved).all(|(g, before)| *g > before));
        r.set_render_scale(0.6).unwrap();
        assert_eq!(sizes(&r), grown);
        assert!(r.reserve_render_scales(f32::NAN).is_err());
    }

    #[test]
    fn timestamp_decode_rejects_errors_and_handles_overlapping_stages() {
        assert!(decode_pass([1, 2, 3, u64::MAX], 1.).is_none());
        assert!(decode_pass([0, 2, 3, 4], 1.).is_none());
        assert!(decode_pass([4, 3, 2, 1], 1.).is_none());
        assert!(decode_pass([1, 2, 3, 4], f64::NAN).is_none());
        let timing = decode_pass([10, 30, 20, 50], 0.001).unwrap();
        assert_eq!(timing.elapsed_ms, 0.04);
        assert_eq!(timing.vertex_ms, 0.02);
        assert_eq!(timing.fragment_ms, 0.03);
    }
    #[test]
    fn gpu_counters_follow_completed_slots_and_never_reuse_missing_passes() {
        let mut r = Renderer::new(64, 64, 1.).unwrap();
        r.set_frames_in_flight(3).unwrap();
        if !counter_device_or_verify_fallback(&mut r) {
            return;
        }
        r.set_gpu_counters(true).unwrap();
        let camera = Camera {
            eye: Vec3::new(0., 20., 20.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        let terrain = crate::terrain::valley();
        let draws: Vec<_> = terrain
            .primitives
            .iter()
            .enumerate()
            .map(|(i, _)| Draw {
                dye: None,
                base_map: None,
                instance: i as u64,
                model: terrain.clone(),
                primitive: i,
                transform: Mat4::IDENTITY,
                palette: Arc::from([]),
                hidden: 0,
                base: [1.; 4],
                morph: [0.; 2],
            })
            .collect();
        for _ in 0..7 {
            r.draw(camera, &draws, None).unwrap();
        }
        r.wait().unwrap();
        let mut frames: Vec<_> = r.take_completed_frames().collect();
        frames.sort_by_key(|f| f.frame);
        assert_eq!(
            frames.iter().map(|f| f.frame).collect::<Vec<_>>(),
            (0..7).collect::<Vec<_>>()
        );
        for f in &frames {
            assert_eq!(
                f.skipped_pass_mask,
                if f.frame % 2 == 1 { 1 << 2 } else { 0 }
            );
            assert_eq!(f.passes[2].is_none(), f.frame % 2 == 1);
            assert!(f.gpu_ms > 0.);
            assert!(f.allocated_gpu_bytes > 0);
            for i in [3, 4, 5] {
                let timing = f.passes[i].unwrap_or_else(|| {
                    panic!(
                        "frame {} pass {i} has no timestamps: {:?}",
                        f.frame, f.passes
                    )
                });
                assert!(timing.elapsed_ms >= timing.vertex_ms);
                assert!(timing.elapsed_ms >= timing.fragment_ms);
                assert!(
                    timing.elapsed_ms <= f.gpu_ms + 1.,
                    "pass {i} {timing:?}, frame GPU {}",
                    f.gpu_ms
                );
            }
            assert!(
                f.passes[6].is_none(),
                "absent HUD must not reuse old samples"
            );
        }
        r.set_gpu_counters(false).unwrap();
        r.draw(camera, &[], None).unwrap();
        r.wait().unwrap();
        assert!(
            r.take_completed_frames()
                .next()
                .unwrap()
                .passes
                .iter()
                .all(Option::is_none)
        );
        r.set_gpu_counters(true).unwrap();
        r.set_frames_in_flight(1).unwrap();
        r.resize(96, 64).unwrap();
        r.draw(camera, &[], None).unwrap();
        r.wait().unwrap();
        assert!(r.take_completed_frames().next().unwrap().passes[4].is_some());
        r.wait().unwrap();
        assert_eq!(r.take_completed_frames().count(), 0);
    }

    fn output_bytes(r: &Renderer) -> Vec<u8> {
        let row = r.width as usize * 4;
        let size = row * r.height as usize;
        let buffer = r
            .device
            .newBufferWithLength_options(size, MTLResourceOptions::StorageModeShared)
            .unwrap();
        let cb = r.queue.commandBuffer().unwrap();
        let enc = cb.blitCommandEncoder().unwrap();
        unsafe {
            enc.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                &r.output, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 },
                MTLSize { width: r.width as usize, height: r.height as usize, depth: 1 },
                &buffer, 0, row, size);
        }
        enc.endEncoding();
        cb.commit();
        cb.waitUntilCompleted();
        assert!(cb.error().is_none());
        unsafe {
            std::slice::from_raw_parts(buffer.contents().as_ptr().cast::<u8>(), size).to_vec()
        }
    }
    #[test]
    fn post_toggles_skip_work_and_half_targets_resize_for_each_aa_mode() {
        let camera = Camera {
            eye: Vec3::new(0., 30., 30.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        // Numerical fixtures only, not authored look presets. No capture files are written.
        for mode in [0, 1, 4] {
            let mut r = Renderer::with_samples(65, 47, 1., mode).unwrap();
            let sampled = counter_device_or_verify_fallback(&mut r);
            if sampled {
                r.set_gpu_counters(true).unwrap();
            }
            r.scene_time = Some(0.);
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            let original = output_bytes(&r);
            assert!(
                r.post_targets.is_none(),
                "disabled post allocates no targets"
            );
            let base_calls = r.stats.draw_calls;
            r.look.ao_enabled = true;
            r.look.ao_radius = 1.;
            r.look.ao_strength = 1.;
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            assert_eq!(
                original,
                output_bytes(&r),
                "AO leaves sky untouched (mode {mode})"
            );
            assert_eq!(r.stats.draw_calls, base_calls + 2);
            let targets = r.post_targets.as_ref().unwrap();
            assert_eq!((targets.ao.width(), targets.ao.height()), (33, 24));
            if sampled {
                let f = r.stats.completed.unwrap();
                assert!(f.passes[10].is_some() && f.passes[14].is_some());
                assert!(f.passes[11..14].iter().all(Option::is_none));
            }
            r.look.bloom_enabled = true;
            r.look.bloom_threshold = 100.;
            r.look.bloom_radius = 1.;
            r.look.bloom_strength = 0.5;
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            assert_eq!(
                original,
                output_bytes(&r),
                "sub-threshold bloom preserves colour"
            );
            if sampled {
                assert!(
                    r.stats.completed.unwrap().passes[10..15]
                        .iter()
                        .all(Option::is_some)
                );
            }
            r.look.ao_enabled = false;
            r.look.bloom_threshold = 0.;
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            let bright = output_bytes(&r);
            assert!(
                bright
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(original.as_chunks::<4>().0)
                    .all(|(a, b)| a[0] > b[0] && a[1] > b[1] && a[2] > b[2])
            );
            if sampled {
                let f = r.stats.completed.unwrap();
                assert!(f.passes[10].is_none(), "disabled AO has no stale sample");
                assert!(f.passes[11..15].iter().all(Option::is_some));
            }
            r.resize(97, 63).unwrap();
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            let targets = r.post_targets.as_ref().unwrap();
            assert_eq!((targets.ao.width(), targets.ao.height()), (49, 32));
            r.look.bloom_strength = 0.;
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            assert_eq!(r.stats.draw_calls, base_calls);
            if sampled {
                assert!(
                    r.stats.completed.unwrap().passes[10..15]
                        .iter()
                        .all(Option::is_none)
                );
            }
        }
    }

    #[test]
    fn post_ao_obscures_geometry_and_toggles_reset_temporal_history() {
        let mut r = Renderer::with_samples(128, 128, 1., 1).unwrap();
        r.scene_time = Some(0.);
        let terrain = crate::terrain::valley();
        let draws: Vec<_> = terrain
            .primitives
            .iter()
            .enumerate()
            .map(|(i, _)| Draw {
                dye: None,
                base_map: None,
                instance: i as u64,
                model: terrain.clone(),
                primitive: i,
                transform: Mat4::IDENTITY,
                palette: Arc::from([]),
                hidden: 0,
                base: [1.; 4],
                morph: [0.; 2],
            })
            .collect();
        let camera = Camera {
            eye: Vec3::new(0., 30., 30.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        let before = output_bytes(&r);
        r.look.ao_enabled = true;
        r.look.ao_radius = 10.;
        r.look.ao_strength = 1.;
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        let after = output_bytes(&r);
        assert!(
            after.iter().zip(&before).any(|(a, b)| a < b),
            "AO must affect non-flat geometry"
        );
        assert!(
            after
                .iter()
                .zip(&before)
                .all(|(a, b)| *a <= b.saturating_add(1)),
            "AO cannot brighten geometry"
        );
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        assert_eq!(after, output_bytes(&r), "non-temporal AO must stay stable");
        let ao_bytes = |r: &Renderer| {
            let texture = &r.post_targets.as_ref().unwrap().ao;
            let row = texture.width() * 4; // RG16Float
            let size = row * texture.height();
            let buffer = r
                .device
                .newBufferWithLength_options(size, MTLResourceOptions::StorageModeShared)
                .unwrap();
            let cb = r.queue.commandBuffer().unwrap();
            let enc = cb.blitCommandEncoder().unwrap();
            unsafe {
                enc.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                    texture, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 },
                    MTLSize { width: texture.width(), height: texture.height(), depth: 1 },
                    &buffer, 0, row, size);
            }
            enc.endEncoding();
            cb.commit();
            cb.waitUntilCompleted();
            assert!(cb.error().is_none());
            unsafe {
                std::slice::from_raw_parts(buffer.contents().as_ptr().cast::<u8>(), size).to_vec()
            }
        };
        r.temporal = true;
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        let first = ao_bytes(&r);
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        assert_ne!(first, ao_bytes(&r), "temporal AO must vary its samples");
        let mut r = Renderer::new(64, 64, 1.).unwrap();
        for metalfx in [false, true] {
            if metalfx && !r.enable_metalfx(true).unwrap() {
                continue;
            }
            r.look.bloom_enabled = false;
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            let resets = r.stats.history_resets;
            r.look.bloom_enabled = true;
            r.look.bloom_radius = 1.;
            r.look.bloom_strength = 0.5;
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            assert_eq!(r.stats.history_resets, resets + 1);
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            assert_eq!(
                r.stats.history_resets,
                resets + 1,
                "stable post look keeps history"
            );
            r.look.bloom_knee = 1.;
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            assert_eq!(
                r.stats.history_resets,
                resets + 2,
                "knee changes reset history"
            );
            r.draw(camera, &[], None).unwrap();
            r.wait().unwrap();
            assert_eq!(r.stats.history_resets, resets + 2);
            r.look.bloom_knee = 0.5;
        }
    }

    #[test]
    fn diagnostic_scene_split_preserves_supplied_surfaces_and_hud_samples_expire() {
        let mut r = Renderer::with_samples(256, 256, 1., 1).unwrap();
        if !counter_device_or_verify_fallback(&mut r) {
            return;
        }
        r.scene_time = Some(0.);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        r.look = LookSettings::load(&root.join("catalog/look.json")).unwrap();
        r.load_surface_assets(&root.join("catalog/surfaces.json"), &root)
            .unwrap();
        let terrain = crate::terrain::valley();
        let draws: Vec<_> = terrain
            .primitives
            .iter()
            .enumerate()
            .map(|(i, _)| Draw {
                dye: None,
                base_map: None,
                instance: i as u64,
                model: terrain.clone(),
                primitive: i,
                transform: Mat4::IDENTITY,
                palette: Arc::from([]),
                hidden: 0,
                base: [1.; 4],
                morph: [0.; 2],
            })
            .collect();
        let camera = Camera {
            eye: Vec3::new(0., 30., 30.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        let normal = r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        let before = output_bytes(&r);
        assert_eq!(r.stats.completed.unwrap().frame, 0);
        assert_eq!(r.stats.completed.unwrap().skipped_pass_mask, 0);
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        let reuse = r.stats.completed.unwrap();
        assert_eq!(reuse.frame, 1);
        assert_eq!(reuse.skipped_pass_mask, 1 << 2);
        assert!(
            reuse.passes[2].is_none(),
            "reused far layer has no timestamp"
        );
        r.set_gpu_counters(true).unwrap();
        r.profile_scene = true;
        let split = r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        let after = output_bytes(&r);
        assert_eq!(normal.draw_calls, split.draw_calls);
        assert_eq!(normal.triangles, split.triangles);
        assert_eq!(
            before, after,
            "profiling must not change the rendered pixels"
        );
        let frame = r.stats.completed.unwrap();
        assert_eq!(frame.frame, 2);
        assert_eq!(frame.skipped_pass_mask, 0);
        assert!(frame.passes[2].is_some());
        for i in [7, 8, 9] {
            assert!(frame.passes[i].is_some(), "missing split pass {i}");
        }
        assert!(frame.passes[3].is_none());
        assert!(frame.passes[4].is_none(), "AA disabled, no stale TAA data");
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Performance test");
            });
        });
        r.hud = Some(crate::gui::HudFrame {
            primitives: ctx.tessellate(output.shapes, output.pixels_per_point),
            textures: output.textures_delta,
            pixels_per_point: output.pixels_per_point,
        });
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        assert!(r.stats.completed.unwrap().passes[6].is_some());
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        assert!(r.stats.completed.unwrap().passes[6].is_none());
    }
    #[test]
    fn static_scene_motion_excludes_halton_jitter() {
        let mut r = Renderer::new(64, 64, 1.).unwrap();
        r.prefilter_sky().unwrap(); // Exercise every lighting mip using the existing neutral fallback.
        assert_eq!(r.sky_reflection.mipmapLevelCount(), 9);
        let terrain = crate::terrain::valley();
        let draws: Vec<_> = terrain
            .primitives
            .iter()
            .enumerate()
            .map(|(i, _)| Draw {
                dye: None,
                base_map: None,
                instance: i as u64,
                model: terrain.clone(),
                primitive: i,
                transform: Mat4::IDENTITY,
                palette: Arc::from([]),
                hidden: 0,
                base: [1.; 4],
                morph: [0.; 2],
            })
            .collect();
        let camera = Camera {
            eye: Vec3::new(0., 20., 20.),
            target: Vec3::ZERO,
            fov: 1.,
        };
        r.draw(camera, &draws, None).unwrap();
        r.draw(camera, &draws, None).unwrap();
        r.wait().unwrap();
        let bytes = 64 * 64 * 8;
        let buffer = r
            .device
            .newBufferWithLength_options(bytes, MTLResourceOptions::StorageModeShared)
            .unwrap();
        let cb = r.queue.commandBuffer().unwrap();
        let blit = cb.blitCommandEncoder().unwrap();
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                &r.motion,0,0,MTLOrigin{x:0,y:0,z:0},MTLSize{width:64,height:64,depth:1},&buffer,0,512,bytes);
        }
        blit.endEncoding();
        cb.commit();
        cb.waitUntilCompleted();
        assert!(cb.error().is_none());
        let values = unsafe {
            std::slice::from_raw_parts(buffer.contents().as_ptr().cast::<u16>(), bytes / 2)
        };
        let mut covered = 0;
        for pixel in values.as_chunks::<4>().0 {
            if pixel[3] != 0 {
                covered += 1;
                assert_eq!(pixel[0] & 0x7fff, 0, "static horizontal motion");
                assert_eq!(pixel[1] & 0x7fff, 0, "static vertical motion");
            }
        }
        assert!(covered > 100, "the test must actually rasterise geometry");
    }
    // The original interleaved vertex entry point is compiled only by this regression test.
    const SHADOW_REFERENCE: &str = r#"vertex Vary shadow_vertex_reference(uint id [[vertex_id]],uint instance [[instance_id]],device const Vertex* v [[buffer(0)]],constant Frame& f [[buffer(1)]],constant Draw* instances [[buffer(2)]],device const float4x4* bones [[buffer(3)]],device const float4* morphs [[buffer(4)]],constant uint& cascade [[buffer(5)]],device const float4x4* old_bones [[buffer(6)]]){
    constant Draw& d=instances[instance];Vary o=deform(id,v,f,d,bones,morphs,old_bones);o.instance=instance;o.position=f.shadows[cascade]*float4(o.world,1);int region=int(round(o.region));if(region>=0&&region<31&&(d.flags.x&(1u<<region)))o.position=float4(2,2,2,1);return o;
}
"#;

    fn reference_shadow_pipeline(r: &Renderer, masked: bool) -> Obj<dyn MTLRenderPipelineState> {
        let source = format!("{}\n{}", include_str!("shaders.metal"), SHADOW_REFERENCE);
        let library = r
            .device
            .newLibraryWithSource_options_error(&NSString::from_str(&source), None)
            .unwrap();
        let desc = MTLRenderPipelineDescriptor::new();
        desc.setVertexFunction(
            library
                .newFunctionWithName(&NSString::from_str("shadow_vertex_reference"))
                .as_deref(),
        );
        if masked {
            desc.setFragmentFunction(
                library
                    .newFunctionWithName(&NSString::from_str("shadow_fragment"))
                    .as_deref(),
            );
        }
        desc.setDepthAttachmentPixelFormat(MTLPixelFormat::Depth32Float);
        r.device
            .newRenderPipelineStateWithDescriptor_error(&desc)
            .unwrap()
    }

    // Exercise the real upload and pipelines with instanced draws, with a small depth array read into memory.
    // Each cascade has a different transform; no image files or authored assets are produced.
    fn shadow_depth_bytes(
        r: &mut Renderer,
        draws: &[Draw],
        time: f32,
        reference: bool,
    ) -> Vec<u32> {
        for d in draws {
            r.upload(d).unwrap();
        }
        let uniforms = buffer(&r.device, &[0u8; 4096]).unwrap();
        let bones = buffer(
            &r.device,
            &[
                Mat4::from_rotation_z(0.12),
                Mat4::from_translation(Vec3::new(0.1, 0.03, 0.02)),
                Mat4::from_rotation_y(-0.09),
                Mat4::IDENTITY,
            ]
            .map(|m| m.to_cols_array_2d()),
        )
        .unwrap();
        for (i, d) in draws.iter().enumerate() {
            let p = &d.model.primitives[d.primitive];
            let mut u: DrawUniform = bytemuck::Zeroable::zeroed();
            u.model = d.transform.to_cols_array_2d();
            u.previous_model = u.model;
            u.base = d.base;
            u.params[3] = p.material.alpha_cutoff;
            u.flags = [
                d.hidden,
                (!d.palette.is_empty()) as u32,
                p.material.alpha_mode,
                0,
            ];
            u.morph = [
                d.morph[0],
                d.morph[1],
                (!p.morphs.is_empty()) as u32 as f32,
                0.,
            ];
            u.previous_morph = u.morph;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    ptr(&u).as_ptr().cast::<u8>(),
                    uniforms
                        .contents()
                        .as_ptr()
                        .cast::<u8>()
                        .add(i * std::mem::size_of::<DrawUniform>()),
                    std::mem::size_of::<DrawUniform>(),
                );
            }
        }
        let mut f: Frame = bytemuck::Zeroable::zeroed();
        f.vp = Mat4::IDENTITY.to_cols_array_2d();
        f.previous_vp = f.vp;
        f.clock = [time, time - 0.1, 1.3, 0.];
        for c in 0..3 {
            f.shadows[c] = (Mat4::from_translation(Vec3::new(c as f32 * 0.07, 0., 0.))
                * Mat4::from_scale(Vec3::splat(1. - c as f32 * 0.1)))
            .to_cols_array_2d();
        }
        let desc = MTLTextureDescriptor::new();
        unsafe {
            desc.setWidth(64);
            desc.setHeight(64);
            desc.setArrayLength(3);
        }
        desc.setTextureType(MTLTextureType::Type2DArray);
        desc.setPixelFormat(MTLPixelFormat::Depth32Float);
        desc.setStorageMode(MTLStorageMode::Private);
        desc.setUsage(MTLTextureUsage::RenderTarget);
        let depth = r.device.newTextureWithDescriptor(&desc).unwrap();
        let read = buffer(&r.device, &[0u32; 64 * 64 * 3]).unwrap();
        let cb = r.queue.commandBuffer().unwrap();
        for cascade in 0..3 {
            let pass = MTLRenderPassDescriptor::new();
            let a = pass.depthAttachment();
            a.setTexture(Some(&depth));
            a.setSlice(cascade);
            a.setLoadAction(MTLLoadAction::Clear);
            a.setStoreAction(MTLStoreAction::Store);
            a.setClearDepth(1.);
            let enc = cb.renderCommandEncoderWithDescriptor(&pass).unwrap();
            enc.setDepthStencilState(Some(&r.depth_state));
            enc.setDepthBias_slopeScale_clamp(0.001, 1.25, 0.01);
            let g = &r.meshes[&(draws[0].model.id, draws[0].primitive)];
            enc.setRenderPipelineState(if g.material.alpha_mode == 1 {
                &r.shadow_mask_pipeline
            } else {
                &r.shadow_pipeline
            });
            enc.setCullMode(MTLCullMode::None);
            unsafe {
                enc.setVertexBytes_length_atIndex(ptr(&f), std::mem::size_of::<Frame>(), 1);
                enc.setVertexBytes_length_atIndex(ptr(&(cascade as u32)), 4, 5);
                enc.setVertexBytes_length_atIndex(ptr(&g.shadow_layout), 8, 7);
                enc.setVertexBuffer_offset_atIndex(
                    Some(if reference {
                        &g.vertices
                    } else {
                        &g.shadow_vertices
                    }),
                    0,
                    0,
                );
                enc.setVertexBuffer_offset_atIndex(Some(&uniforms), 0, 2);
                enc.setVertexBuffer_offset_atIndex(Some(&bones), 0, 3);
                enc.setVertexBuffer_offset_atIndex(Some(&g.morphs), 0, 4);
                enc.setVertexBuffer_offset_atIndex(Some(&bones), 0, 6);
                enc.setFragmentBuffer_offset_atIndex(Some(&uniforms), 0, 2);
                enc.setFragmentTexture_atIndex(Some(&g.maps[0]), 0);
                enc.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount(
                    MTLPrimitiveType::Triangle,g.count,MTLIndexType::UInt32,&g.indices,0,draws.len());
            }
            enc.endEncoding();
        }
        let enc = cb.blitCommandEncoder().unwrap();
        for c in 0..3 {
            unsafe {
                enc.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                &depth,c,0,MTLOrigin{x:0,y:0,z:0},MTLSize{width:64,height:64,depth:1},&read,c*64*64*4,256,64*64*4);
            }
        }
        enc.endEncoding();
        cb.commit();
        cb.waitUntilCompleted();
        assert!(cb.error().is_none());
        unsafe {
            std::slice::from_raw_parts(read.contents().as_ptr().cast::<u32>(), 64 * 64 * 3).to_vec()
        }
    }

    #[test]
    fn compact_shadow_streams_match_interleaved_depth_in_every_cascade() {
        use crate::model::{Model, Primitive, Vertex};
        let mut r = Renderer::with_samples(64, 64, 1., 1).unwrap();
        let compact = (r.shadow_pipeline.clone(), r.shadow_mask_pipeline.clone());
        let reference = (
            reference_shadow_pipeline(&r, false),
            reference_shadow_pipeline(&r, true),
        );
        // Independent and combined skin/morph/wind/mask inputs, plus hidden-region clipping.
        for bits in 0..32 {
            let skinned = bits & 1 != 0;
            let morphed = bits & 2 != 0;
            let windy = bits & 4 != 0;
            let masked = bits & 8 != 0;
            let hidden = bits & 16 != 0;
            let mut vertices: Vec<_> = [
                Vec3::new(-0.7, -0.6, 0.5),
                Vec3::new(0.7, -0.6, 0.5),
                Vec3::new(0., 0.7, 0.5),
            ]
            .into_iter()
            .map(|p| Vertex::plain(p, Vec3::Z, [1.; 4]))
            .collect();
            for (i, v) in vertices.iter_mut().enumerate() {
                if skinned {
                    v.joints = [0, 1, 2, 3];
                    v.weights = [0.4, 0.3, 0.2, 0.1];
                }
                if windy {
                    v.normal[3] = 0.2 + i as f32 * 0.3;
                }
                if hidden {
                    v.position[3] = 2.;
                }
                v.uv = [i as f32 * 0.4, 0.7, 0., 0.];
                v.color[3] = if i == 0 { 0.05 } else { 1. };
            }
            let mut material = Material {
                alpha_mode: if masked { 1 } else { 0 },
                double_sided: true,
                ..Default::default()
            };
            if masked {
                material.maps[0] = Some(Arc::new(Texture {
                    key: format!("test-shadow-mask:{bits}"),
                    width: 2,
                    height: 2,
                    rgba: Mutex::new(Some(vec![
                        255, 255, 255, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                        255,
                    ])),
                    compressed: None,
                }));
            }
            let p = Primitive {
                bounds: Bounds::from_points(
                    vertices.iter().map(|v| Vec3::from_slice(&v.position[..3])),
                ),
                vertices,
                indices: vec![0, 1, 2],
                material,
                node: 0,
                skin: skinned.then_some(0),
                morphs: if morphed {
                    vec![vec![[0.04, 0.03, 0.01]; 3], vec![[-0.01, 0.06, -0.02]; 3]]
                } else {
                    vec![]
                },
                morph_names: if morphed {
                    vec!["arrow".into(), "draw".into()]
                } else {
                    vec![]
                },
            };
            let (words, layout) = shadow_stream(&p);
            assert_eq!(words.len(), p.vertices.len() * layout[0] as usize);
            assert_eq!(
                layout[0],
                3 + hidden as u32 + windy as u32 + 8 * skinned as u32 + 3 * masked as u32
            );
            let model = Arc::new(Model {
                id: 10000 + bits,
                key: format!("test:{bits}"),
                nodes: vec![],
                rest_world: vec![],
                skins: vec![],
                primitives: vec![p],
                clips: HashMap::new(),
            });
            let draws: Vec<_> = (0..2)
                .map(|i| Draw {
                    model: model.clone(),
                    primitive: 0,
                    instance: i,
                    transform: Mat4::from_translation(Vec3::new(i as f32 * 0.15, 0., 0.))
                        * Mat4::from_rotation_y(0.07),
                    palette: if skinned {
                        Arc::from([Mat4::IDENTITY; 4])
                    } else {
                        Arc::from([])
                    },
                    hidden: if hidden { 1 << 2 } else { 0 },
                    base: [1.; 4],
                    morph: [0.6, 0.3],
                    dye: None,
                    base_map: None,
                })
                .collect();
            for time in [0., 1.75] {
                r.shadow_pipeline = reference.0.clone();
                r.shadow_mask_pipeline = reference.1.clone();
                let before = shadow_depth_bytes(&mut r, &draws, time, true);
                r.shadow_pipeline = compact.0.clone();
                r.shadow_mask_pipeline = compact.1.clone();
                let after = shadow_depth_bytes(&mut r, &draws, time, false);
                for c in 0..3 {
                    let range = c * 4096..(c + 1) * 4096;
                    if let Some((pixel, (before, after))) = before[range.clone()]
                        .iter()
                        .zip(&after[range.clone()])
                        .enumerate()
                        .find(|(_, (a, b))| a != b)
                    {
                        panic!(
                            "shadow depth differs: bits={bits},time={time},cascade={c},pixel={pixel},before={before:#010x},after={after:#010x}"
                        );
                    }
                    let covered = after[range]
                        .iter()
                        .filter(|&&z| z != 1f32.to_bits())
                        .count();
                    if hidden {
                        assert_eq!(covered, 0);
                    } else {
                        assert!(
                            covered > 100,
                            "test must rasterise, bits={bits},cascade={c}"
                        );
                    }
                }
            }
        }
    }
}
