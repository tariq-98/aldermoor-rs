//! The effects pass on Metal (crate::fx). Decals are drawn at the end of the scene pass; quads
//! and effect meshes at the end of the composite pass, over the tone-mapped output. Neither adds
//! a render pass, so a frame without effects costs nothing extra.
use super::{Frame, Obj, Renderer, buffer, image_texture, ptr};
use crate::fx::{self, Decal, MeshInstance, Quad, RAMP_WIDTH};
use anyhow::{Context, Result, ensure};
use glam::Vec3;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::*;
use std::{ffi::c_void, ptr::NonNull};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    output: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
}

struct Mesh {
    vertices: Obj<dyn MTLBuffer>,
    indices: Obj<dyn MTLBuffer>,
    count: usize,
}

struct Art {
    atlas: Obj<dyn MTLTexture>,
    tiles: Obj<dyn MTLTexture>,
    ramps: Obj<dyn MTLTexture>,
    meshes: Vec<Mesh>,
}

pub(super) struct FxPass {
    quad: Obj<dyn MTLRenderPipelineState>,
    mesh: Obj<dyn MTLRenderPipelineState>,
    decal: Obj<dyn MTLRenderPipelineState>,
    always: Obj<dyn MTLDepthStencilState>,
    art: Option<Art>,
    /// Instance data, one buffer per frame in flight (fenced with the frame's slot).
    rings: Vec<Option<Obj<dyn MTLBuffer>>>,
    /// Effects need single-sample depth; the MSAA comparison mode draws none.
    single_sample: bool,
}

/// One frame's effects, already in its slot's ring.
#[derive(Default)]
pub(super) struct Prepared {
    ring: Option<Obj<dyn MTLBuffer>>,
    quads: (usize, usize),
    decals: (usize, usize),
    /// Mesh, offset, count.
    meshes: Vec<(usize, usize, usize)>,
    time: f32,
}

const QUAD: usize = std::mem::size_of::<Quad>();
const DECAL: usize = std::mem::size_of::<Decal>();
const MESH: usize = std::mem::size_of::<MeshInstance>();

fn align(n: usize) -> usize {
    (n + 255) & !255
}

fn premultiplied(c: &MTLRenderPipelineColorAttachmentDescriptor) {
    c.setBlendingEnabled(true);
    c.setSourceRGBBlendFactor(MTLBlendFactor::One);
    c.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    c.setSourceAlphaBlendFactor(MTLBlendFactor::One);
    c.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
}

impl FxPass {
    pub(super) fn new(device: &ProtocolObject<dyn MTLDevice>, samples: usize) -> Result<Self> {
        let library = device
            .newLibraryWithSource_options_error(
                &NSString::from_str(include_str!("../fx.metal")),
                None,
            )
            .map_err(|e| anyhow::anyhow!("effects shaders: {e}"))?;
        let function = |name: &str| -> Result<Obj<dyn MTLFunction>> {
            library
                .newFunctionWithName(&NSString::from_str(name))
                .with_context(|| format!("effects shader {name}"))
        };
        // Quads and meshes: over the composite's output.
        let overlay = |vs: &str, fs: &str| -> Result<Obj<dyn MTLRenderPipelineState>> {
            let p = MTLRenderPipelineDescriptor::new();
            let (vertex, fragment) = (function(vs)?, function(fs)?);
            p.setVertexFunction(Some(&vertex));
            p.setFragmentFunction(Some(&fragment));
            let c = unsafe { p.colorAttachments().objectAtIndexedSubscript(0) };
            c.setPixelFormat(MTLPixelFormat::BGRA8Unorm_sRGB);
            premultiplied(&c);
            device
                .newRenderPipelineStateWithDescriptor_error(&p)
                .map_err(|e| anyhow::anyhow!("effects pipeline {vs}: {e}"))
        };
        let quad = overlay("quad_vertex", "quad_fragment")?;
        let mesh = overlay("mesh_vertex", "mesh_fragment")?;
        // Decals: inside the scene pass, adding to its colour and leaving its motion alone.
        let p = MTLRenderPipelineDescriptor::new();
        let (vertex, fragment) = (function("decal_vertex")?, function("decal_fragment")?);
        p.setVertexFunction(Some(&vertex));
        p.setFragmentFunction(Some(&fragment));
        p.setRasterSampleCount(samples);
        for i in 0..2 {
            let c = unsafe { p.colorAttachments().objectAtIndexedSubscript(i) };
            c.setPixelFormat(MTLPixelFormat::RGBA16Float);
            if i == 0 {
                premultiplied(&c);
            } else {
                c.setWriteMask(MTLColorWriteMask::None);
            }
        }
        p.setDepthAttachmentPixelFormat(MTLPixelFormat::Depth32Float);
        let decal = device
            .newRenderPipelineStateWithDescriptor_error(&p)
            .map_err(|e| anyhow::anyhow!("effects pipeline decal_vertex: {e}"))?;
        let d = MTLDepthStencilDescriptor::new();
        d.setDepthCompareFunction(MTLCompareFunction::Always);
        d.setDepthWriteEnabled(false);
        let always = device
            .newDepthStencilStateWithDescriptor(&d)
            .context("effects depth state")?;
        Ok(Self {
            quad,
            mesh,
            decal,
            always,
            art: None,
            rings: Vec::new(),
            single_sample: samples == 1,
        })
    }

    /// Sorts the frame's quads farthest first and writes everything into the slot's ring.
    pub(super) fn prepare(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        slot: usize,
        frame: Option<&fx::Frame>,
        eye: Vec3,
    ) -> Result<Prepared> {
        let Some(frame) = frame.filter(|f| !f.is_empty() && self.art.is_some()) else {
            return Ok(Prepared::default());
        };
        let art = self.art.as_ref().unwrap();
        let mut quads = frame.quads.clone();
        let distance = |q: &Quad| {
            let at = Vec3::from_slice(&q.position[..3]);
            if q.position[3] == Quad::BEAM {
                ((at + Vec3::from_slice(&q.a[..3])) * 0.5).distance_squared(eye)
            } else {
                at.distance_squared(eye)
            }
        };
        quads.sort_by(|a, b| distance(b).total_cmp(&distance(a)));
        let mut meshes: Vec<_> = frame
            .meshes
            .iter()
            .filter(|(m, _)| *m < art.meshes.len())
            .collect();
        meshes.sort_by_key(|(m, _)| *m);
        let quad_bytes = align(quads.len() * QUAD);
        let decal_bytes = align(frame.decals.len() * DECAL);
        let need = quad_bytes + decal_bytes + align(meshes.len() * MESH) + 256;
        if self.rings.len() <= slot {
            self.rings.resize(slot + 1, None);
        }
        let ring = match &self.rings[slot] {
            Some(r) if r.length() >= need => r.clone(),
            _ => {
                let r = device
                    .newBufferWithLength_options(
                        need.next_power_of_two(),
                        MTLResourceOptions::StorageModeShared,
                    )
                    .context("effects ring")?;
                self.rings[slot] = Some(r.clone());
                r
            }
        };
        let base = ring.contents().as_ptr().cast::<u8>();
        let put = |offset: usize, bytes: &[u8]| unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), base.add(offset), bytes.len());
        };
        put(0, bytemuck::cast_slice(&quads));
        put(quad_bytes, bytemuck::cast_slice(&frame.decals));
        let mut groups: Vec<(usize, usize, usize)> = Vec::new();
        let start = quad_bytes + decal_bytes;
        for (i, (m, instance)) in meshes.iter().enumerate() {
            put(start + i * MESH, bytemuck::bytes_of(instance));
            match groups.last_mut() {
                Some(g) if g.0 == *m => g.2 += 1,
                _ => groups.push((*m, start + i * MESH, 1)),
            }
        }
        Ok(Prepared {
            ring: Some(ring),
            quads: (0, quads.len()),
            decals: (quad_bytes, frame.decals.len()),
            meshes: groups,
            time: frame.time,
        })
    }

    fn bind_art(&self, enc: &ProtocolObject<dyn MTLRenderCommandEncoder>, art: &Art) {
        unsafe {
            enc.setFragmentTexture_atIndex(Some(&art.atlas), 0);
            enc.setFragmentTexture_atIndex(Some(&art.tiles), 1);
            enc.setFragmentTexture_atIndex(Some(&art.ramps), 2);
        }
    }

    /// Ground marks, at the end of the scene pass.
    pub(super) fn decals(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        p: &Prepared,
        f: &Frame,
    ) -> usize {
        let (Some(art), Some(ring)) = (&self.art, &p.ring) else {
            return 0;
        };
        if p.decals.1 == 0 || !self.single_sample {
            return 0;
        }
        enc.setRenderPipelineState(&self.decal);
        enc.setDepthStencilState(Some(&self.always));
        enc.setCullMode(MTLCullMode::Front);
        enc.setFrontFacingWinding(MTLWinding::CounterClockwise);
        unsafe {
            enc.setVertexBytes_length_atIndex(ptr(f), std::mem::size_of::<Frame>(), 1);
            enc.setVertexBuffer_offset_atIndex(Some(ring), p.decals.0, 2);
            enc.setFragmentBytes_length_atIndex(ptr(f), std::mem::size_of::<Frame>(), 1);
            enc.setFragmentBuffer_offset_atIndex(Some(ring), p.decals.0, 2);
        }
        self.bind_art(enc, art);
        unsafe {
            enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                MTLPrimitiveType::Triangle,
                0,
                36,
                p.decals.1,
            );
        }
        1
    }

    /// Effect meshes, then sprites and beams, at the end of the composite pass.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn overlay(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        p: &Prepared,
        f: &Frame,
        depth: &ProtocolObject<dyn MTLTexture>,
        output: (u32, u32),
        eye: Vec3,
        target: Vec3,
    ) -> usize {
        let (Some(art), Some(ring)) = (&self.art, &p.ring) else {
            return 0;
        };
        if (p.quads.1 == 0 && p.meshes.is_empty()) || !self.single_sample {
            return 0;
        }
        let forward = (target - eye).normalize_or(Vec3::Z);
        let right = forward.cross(Vec3::Y).normalize_or(Vec3::X);
        let up = right.cross(forward);
        let (w, h) = (output.0 as f32, output.1 as f32);
        let g = Globals {
            output: [w, h, 1. / w, 1. / h],
            right: right.extend(p.time).to_array(),
            up: up.extend(0.).to_array(),
        };
        let mut calls = 0;
        let bind = |offset: usize| unsafe {
            enc.setVertexBytes_length_atIndex(ptr(f), std::mem::size_of::<Frame>(), 1);
            enc.setVertexBuffer_offset_atIndex(Some(ring), offset, 2);
            enc.setVertexBytes_length_atIndex(ptr(&g), std::mem::size_of::<Globals>(), 3);
            enc.setFragmentBytes_length_atIndex(ptr(f), std::mem::size_of::<Frame>(), 1);
            enc.setFragmentBuffer_offset_atIndex(Some(ring), offset, 2);
            enc.setFragmentBytes_length_atIndex(ptr(&g), std::mem::size_of::<Globals>(), 3);
            enc.setFragmentTexture_atIndex(Some(depth), 3);
        };
        enc.setCullMode(MTLCullMode::None);
        if !p.meshes.is_empty() {
            enc.setRenderPipelineState(&self.mesh);
            self.bind_art(enc, art);
            for &(m, offset, count) in &p.meshes {
                let mesh = &art.meshes[m];
                bind(offset);
                unsafe {
                    enc.setVertexBuffer_offset_atIndex(Some(&mesh.vertices), 0, 0);
                    enc.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount(
                        MTLPrimitiveType::Triangle,
                        mesh.count,
                        MTLIndexType::UInt32,
                        &mesh.indices,
                        0,
                        count,
                    );
                }
                calls += 1;
            }
        }
        if p.quads.1 > 0 {
            enc.setRenderPipelineState(&self.quad);
            self.bind_art(enc, art);
            bind(p.quads.0);
            unsafe {
                enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                    MTLPrimitiveType::Triangle,
                    0,
                    6,
                    p.quads.1,
                );
            }
            calls += 1;
        }
        calls
    }
}

/// Layers of one size and format in one array texture, mips included.
fn layers(
    device: &ProtocolObject<dyn MTLDevice>,
    queue: &ProtocolObject<dyn MTLCommandQueue>,
    sources: &[Obj<dyn MTLTexture>],
) -> Result<Obj<dyn MTLTexture>> {
    let first = sources.first().context("effects need at least one tile")?;
    ensure!(
        sources.iter().all(|t| t.width() == first.width()
            && t.height() == first.height()
            && t.mipmapLevelCount() == first.mipmapLevelCount()),
        "effect tiles must share one size"
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
        .context("effect tiles")?;
    let cb = queue.commandBuffer().context("effect tiles upload")?;
    let blit = cb.blitCommandEncoder().context("effect tiles encoder")?;
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

impl Renderer {
    /// Uploads the art effects are drawn with. Until this is called, effects are skipped.
    pub fn load_effects(&mut self, library: &fx::Library) -> Result<()> {
        self.wait()?;
        let atlas = image_texture(&self.device, &self.queue, &library.atlas, false)?;
        let mut sources = Vec::new();
        for t in &library.tiles {
            sources.push(image_texture(&self.device, &self.queue, t, false)?);
        }
        let tiles = layers(&self.device, &self.queue, &sources)?;
        ensure!(!library.ramps.is_empty(), "effects need at least one ramp");
        let d = MTLTextureDescriptor::new();
        unsafe {
            d.setWidth(RAMP_WIDTH);
            d.setHeight(library.ramps.len());
        }
        d.setPixelFormat(MTLPixelFormat::RGBA32Float);
        d.setStorageMode(MTLStorageMode::Shared);
        d.setUsage(MTLTextureUsage::ShaderRead);
        let ramps = self
            .device
            .newTextureWithDescriptor(&d)
            .context("effect ramps")?;
        let rows: Vec<[f32; 4]> = library.ramps.iter().flatten().copied().collect();
        unsafe {
            ramps.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                MTLRegion {
                    origin: MTLOrigin { x: 0, y: 0, z: 0 },
                    size: MTLSize {
                        width: RAMP_WIDTH,
                        height: library.ramps.len(),
                        depth: 1,
                    },
                },
                0,
                NonNull::new(rows.as_ptr() as *mut c_void).unwrap(),
                RAMP_WIDTH * 16,
            );
        }
        let mut meshes = Vec::new();
        for model in &library.meshes {
            let p = model
                .primitives
                .first()
                .with_context(|| format!("effect mesh {} has no geometry", model.key))?;
            meshes.push(Mesh {
                vertices: buffer(&self.device, &p.vertices)?,
                indices: buffer(&self.device, &p.indices)?,
                count: p.indices.len(),
            });
        }
        self.fx_pass.art = Some(Art {
            atlas,
            tiles,
            ramps,
            meshes,
        });
        Ok(())
    }
}
