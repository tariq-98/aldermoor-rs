//! Direct Metal: temporal AA, stable cascades, visibility and bounded frames in flight.
use crate::{
    character::Draw,
    model::{Material, Texture},
    settings::{LookSettings, ToneMap},
    visibility::{
        Bounds, Frustum, ShadowCasterVolume, stable_cascades, terrain_lod, terrain_shadow_lod,
    },
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
    grade: [[f32; 4]; 3],
    params: [f32; 4],
    clock: [f32; 4],
    viewport: [f32; 4],
    splits: [f32; 4],
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
struct GpuMesh {
    terrain_indices: Vec<(Obj<dyn MTLBuffer>, usize)>,
    vertices: Obj<dyn MTLBuffer>,
    indices: Obj<dyn MTLBuffer>,
    morphs: Obj<dyn MTLBuffer>,
    count: usize,
    maps: [Obj<dyn MTLTexture>; 5],
    material: Material,
}
impl GpuMesh {
    fn indices_at(&self, level: usize) -> (&Obj<dyn MTLBuffer>, usize) {
        match level
            .checked_sub(1)
            .and_then(|i| self.terrain_indices.get(i))
        {
            Some((indices, count)) => (indices, *count),
            None => (&self.indices, self.count),
        }
    }
}
pub const GPU_PASS_NAMES: [&str; 10] = [
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
    terrain_lod: Option<(usize, usize)>,
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
    terrain_lod: usize,
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
    depth_state: Obj<dyn MTLDepthStencilState>,
    read_depth: Obj<dyn MTLDepthStencilState>,
    hdr: Obj<dyn MTLTexture>,
    msaa: Obj<dyn MTLTexture>,
    motion: Obj<dyn MTLTexture>,
    motion_msaa: Obj<dyn MTLTexture>,
    depth: Obj<dyn MTLTexture>,
    shadow: Obj<dyn MTLTexture>,
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
fn texture(
    d: &ProtocolObject<dyn MTLDevice>,
    w: usize,
    h: usize,
    format: MTLPixelFormat,
    samples: usize,
    usage: MTLTextureUsage,
) -> Result<Obj<dyn MTLTexture>> {
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
    d.newTextureWithDescriptor(&desc)
        .context("Metal texture allocation")
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
            depth_state,
            read_depth,
            hdr,
            msaa,
            motion,
            motion_msaa,
            depth,
            shadow,
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
        if width == 0 || height == 0 {
            return Ok(());
        }
        self.wait()?;
        self.width = width;
        self.height = height;
        let (w, h) = (
            (width as f32 * self.scale).round().max(1.) as usize,
            (height as f32 * self.scale).round().max(1.) as usize,
        );
        let rt = MTLTextureUsage::RenderTarget;
        let read = MTLTextureUsage::ShaderRead;
        self.hdr = texture(
            &self.device,
            w,
            h,
            MTLPixelFormat::RGBA16Float,
            1,
            rt | read,
        )?;
        self.msaa = texture(
            &self.device,
            w,
            h,
            MTLPixelFormat::RGBA16Float,
            self.samples,
            rt,
        )?;
        self.motion = texture(
            &self.device,
            w,
            h,
            MTLPixelFormat::RGBA16Float,
            1,
            rt | read,
        )?;
        self.motion_msaa = texture(
            &self.device,
            w,
            h,
            MTLPixelFormat::RGBA16Float,
            self.samples,
            rt,
        )?;
        self.depth = texture(
            &self.device,
            w,
            h,
            MTLPixelFormat::Depth32Float,
            self.samples,
            rt | read,
        )?;
        self.history = [
            texture(
                &self.device,
                w,
                h,
                MTLPixelFormat::RGBA16Float,
                1,
                rt | read,
            )?,
            texture(
                &self.device,
                w,
                h,
                MTLPixelFormat::RGBA16Float,
                1,
                rt | read,
            )?,
        ];
        self.output = texture(
            &self.device,
            width as usize,
            height as usize,
            MTLPixelFormat::BGRA8Unorm_sRGB,
            1,
            rt | read,
        )?;
        self.history_valid = false;
        self.previous.clear();
        if let Some(l) = &self.layer {
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
        let mut terrain_indices = Vec::new();
        if draw.model.key == "world:neutral-topology" && p.material.name == "__terrain_placeholder"
        {
            for level in 1..=2 {
                let indices = crate::terrain::lod_indices(draw.primitive, level);
                terrain_indices.push((buffer(&self.device, &indices)?, indices.len()));
            }
        }
        self.meshes.insert(
            key,
            GpuMesh {
                terrain_indices,
                vertices: buffer(&self.device, &p.vertices)?,
                indices: buffer(&self.device, &p.indices)?,
                morphs: buffer(&self.device, &morphs)?,
                count: p.indices.len(),
                maps,
                material: p.material.clone(),
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
        let sun = Vec3::from_array(self.look.sun_direction).normalize();
        let cascades = stable_cascades(
            camera.eye,
            camera.target,
            camera.fov,
            aspect,
            sun,
            SHADOW_SIZE as u32,
        );
        let view = Frustum::new(unjittered);
        let time = self
            .scene_time
            .unwrap_or_else(|| self.started.elapsed().as_secs_f32());
        let f = Frame {
            vp: unjittered.to_cols_array_2d(),
            previous_vp: previous_vp.to_cols_array_2d(),
            inverse_vp: vp.inverse().to_cols_array_2d(),
            jitter: [jitter.x, jitter.y, jitter.x * 0.5, -jitter.y * 0.5],
            shadows: cascades.map(|m| m.to_cols_array_2d()),
            eye: camera.eye.extend(1.).to_array(),
            sun: sun.extend(self.look.sun_intensity).to_array(),
            sun_colour: Vec3::from_array(self.look.sun_colour).extend(1.).to_array(),
            ambient: Vec3::from_array(self.look.ambient).extend(1.).to_array(),
            sky: Vec3::from_array(self.look.sky).extend(1.).to_array(),
            fog: Vec3::from_array(self.look.fog_colour)
                .extend(self.look.fog_density)
                .to_array(),
            grade: self.look.grade.map(|r| [r[0], r[1], r[2], 0.]),
            params: [
                self.look.exposure,
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
        };
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
            let mesh = &self.meshes[&(d.model.id, d.primitive)];
            let terrain_level = (!mesh.terrain_indices.is_empty()).then(|| {
                terrain_lod(
                    p.bounds.transformed(d.transform),
                    camera.eye,
                    old.and_then(|o| o.terrain_lod)
                        .filter(|(primitive, _)| *primitive == d.primitive)
                        .map(|(_, level)| level),
                )
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
                terrain_lod: terrain_level.unwrap_or(0),
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
                    terrain_lod: terrain_level.map(|level| (d.primitive, level)),
                    model_key: d.model.id,
                    transform: d.transform,
                    palette: d.palette.clone(),
                    morph: d.morph,
                },
            );
            if scene {
                triangles += mesh.indices_at(terrain_level.unwrap_or(0)).1 / 3;
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
        for cascade in 0..3 {
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
        let history_index = self.frame % 2;
        if let Some(scaler) = &self.metalfx {
            unsafe {
                scaler.setColorTexture(Some(&self.hdr));
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
                enc.setFragmentTexture_atIndex(Some(&self.hdr), 0);
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
                    &self.hdr
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
                let index = *rigid
                    .entry((d.model.id, d.primitive, p.terrain_lod))
                    .or_insert_with(|| {
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
            let level = if g.terrain_indices.is_empty() {
                0
            } else {
                cascade.map_or(p.terrain_lod, |c| terrain_shadow_lod(p.terrain_lod, c))
            };
            let (indices, index_count) = g.indices_at(level);
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
                enc.setVertexBuffer_offset_atIndex(Some(&g.vertices), 0, 0);
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
                    index_count,
                    MTLIndexType::UInt32,
                    indices,
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
    fn terrain_lods_upload_once_and_report_selected_scene_triangles() {
        let mut renderer = Renderer::new(128, 80, 1.).unwrap();
        let terrain = crate::terrain::valley();
        let draws: Vec<_> = terrain
            .primitives
            .iter()
            .enumerate()
            .map(|(primitive, _)| Draw {
                dye: None,
                base_map: None,
                instance: primitive as u64,
                model: terrain.clone(),
                primitive,
                transform: Mat4::IDENTITY,
                palette: Arc::from([]),
                hidden: 0,
                base: [1.; 4],
                morph: [0.; 2],
            })
            .collect();
        let camera = Camera {
            eye: Vec3::new(0., 30., 100.),
            target: Vec3::new(0., 0., -180.),
            fov: 1.,
        };
        let stats = renderer.draw(camera, &draws, None).unwrap();
        renderer.wait().unwrap();
        let view = Frustum::new(camera.matrix(128. / 80.));
        let mut expected = 0;
        let mut full = 0;
        let mut levels = [false; 3];
        for (primitive, p) in terrain.primitives.iter().enumerate() {
            let bounds = p.bounds.padded(0.2 + renderer.look.wind_strength * 0.12);
            if !view.intersects(bounds) {
                continue;
            }
            let mesh = &renderer.meshes[&(terrain.id, primitive)];
            let level = if p.material.name == "__terrain_placeholder" {
                let level = terrain_lod(p.bounds, camera.eye, None);
                levels[level] = true;
                assert_eq!(
                    renderer.previous[&(primitive as u64)].terrain_lod,
                    Some((primitive, level))
                );
                assert_eq!(mesh.terrain_indices.len(), 2);
                for lod in 1..=2 {
                    assert_eq!(
                        mesh.indices_at(lod).1,
                        crate::terrain::lod_indices(primitive, lod).len()
                    );
                }
                level
            } else {
                assert!(mesh.terrain_indices.is_empty());
                0
            };
            expected += mesh.indices_at(level).1 / 3;
            full += p.indices.len() / 3;
        }
        assert!(levels.into_iter().all(|used| used));
        assert_eq!(stats.triangles, expected);
        assert!(expected < full);
        let allocations: Vec<_> = renderer
            .meshes
            .iter()
            .flat_map(|(key, mesh)| {
                mesh.terrain_indices
                    .iter()
                    .map(move |(buffer, _)| (*key, buffer.contents().as_ptr() as usize))
            })
            .collect();
        renderer.draw(camera, &draws, None).unwrap();
        renderer.wait().unwrap();
        let repeated: Vec<_> = renderer
            .meshes
            .iter()
            .flat_map(|(key, mesh)| {
                mesh.terrain_indices
                    .iter()
                    .map(move |(buffer, _)| (*key, buffer.contents().as_ptr() as usize))
            })
            .collect();
        assert_eq!(allocations, repeated);
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
}
