//! Metal painter for egui's stock functional HUD. Artwork/fonts are supplied separately by Claude.
use anyhow::{Context, Result, ensure};
use egui::{ClippedPrimitive, TextureId, TexturesDelta, epaint::Primitive};
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::*;
use std::{collections::HashMap, ffi::c_void, ptr::NonNull};
type Obj<T> = Retained<ProtocolObject<T>>;
pub struct HudFrame {
    pub primitives: Vec<ClippedPrimitive>,
    pub textures: TexturesDelta,
    pub pixels_per_point: f32,
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    colour: [f32; 4],
}
struct Ring {
    vertices: Obj<dyn MTLBuffer>,
    indices: Obj<dyn MTLBuffer>,
}
pub(crate) struct Gui {
    pipeline: Obj<dyn MTLRenderPipelineState>,
    textures: HashMap<TextureId, Obj<dyn MTLTexture>>,
    rings: Vec<Ring>,
    depth: Obj<dyn MTLDepthStencilState>,
}
const CAPACITY: usize = 2 * 1024 * 1024;
impl Gui {
    pub fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        library: &ProtocolObject<dyn MTLLibrary>,
    ) -> Result<Self> {
        let p = MTLRenderPipelineDescriptor::new();
        p.setVertexFunction(
            library
                .newFunctionWithName(&objc2_foundation::NSString::from_str("gui_vertex"))
                .as_deref(),
        );
        p.setFragmentFunction(
            library
                .newFunctionWithName(&objc2_foundation::NSString::from_str("gui_fragment"))
                .as_deref(),
        );
        let c = unsafe { p.colorAttachments().objectAtIndexedSubscript(0) };
        c.setPixelFormat(MTLPixelFormat::BGRA8Unorm_sRGB);
        c.setBlendingEnabled(true);
        c.setSourceRGBBlendFactor(MTLBlendFactor::One);
        c.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        c.setSourceAlphaBlendFactor(MTLBlendFactor::One);
        c.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
        let pipeline = device
            .newRenderPipelineStateWithDescriptor_error(&p)
            .map_err(|e| anyhow::anyhow!("HUD pipeline: {e}"))?;
        let ds = MTLDepthStencilDescriptor::new();
        ds.setDepthCompareFunction(MTLCompareFunction::Always);
        ds.setDepthWriteEnabled(false);
        let depth = device
            .newDepthStencilStateWithDescriptor(&ds)
            .context("HUD depth state")?;
        let mut rings = Vec::new();
        for _ in 0..3 {
            rings.push(Ring {
                vertices: device
                    .newBufferWithLength_options(CAPACITY, MTLResourceOptions::StorageModeShared)
                    .context("HUD vertices")?,
                indices: device
                    .newBufferWithLength_options(CAPACITY, MTLResourceOptions::StorageModeShared)
                    .context("HUD indices")?,
            });
        }
        Ok(Self {
            pipeline,
            textures: HashMap::new(),
            rings,
            depth,
        })
    }
    pub fn upload(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        frame: &HudFrame,
    ) -> Result<()> {
        for (id, delta) in &frame.textures.set {
            let egui::ImageData::Color(image) = &delta.image;
            let size = image.size;
            let pixels: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            if delta.pos.is_none() {
                let desc = MTLTextureDescriptor::new();
                unsafe {
                    desc.setWidth(size[0]);
                    desc.setHeight(size[1]);
                }
                desc.setPixelFormat(MTLPixelFormat::RGBA8Unorm);
                desc.setStorageMode(MTLStorageMode::Shared);
                desc.setUsage(MTLTextureUsage::ShaderRead);
                self.textures.insert(
                    *id,
                    device
                        .newTextureWithDescriptor(&desc)
                        .context("HUD texture")?,
                );
            }
            let t = self
                .textures
                .get(id)
                .context("HUD partial update without a texture")?;
            let pos = delta.pos.unwrap_or([0, 0]);
            ensure!(
                pos[0] + size[0] <= t.width() && pos[1] + size[1] <= t.height(),
                "HUD texture update exceeds bounds"
            );
            unsafe {
                t.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                    MTLRegion {
                        origin: MTLOrigin {
                            x: pos[0],
                            y: pos[1],
                            z: 0,
                        },
                        size: MTLSize {
                            width: size[0],
                            height: size[1],
                            depth: 1,
                        },
                    },
                    0,
                    NonNull::new(pixels.as_ptr() as *mut c_void).unwrap(),
                    size[0] * 4,
                );
            }
        }
        Ok(())
    }
    pub(crate) fn paint(
        &mut self,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        output: &ProtocolObject<dyn MTLTexture>,
        frame: &HudFrame,
        number: usize,
        counters: Option<&crate::metal::PassCounters>,
    ) -> Result<usize> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut batches = Vec::new();
        let ppp = frame.pixels_per_point;
        for clipped in &frame.primitives {
            let Primitive::Mesh(mesh) = &clipped.primitive else {
                continue;
            };
            let base = vertices.len() as u32;
            let start = indices.len();
            vertices.extend(mesh.vertices.iter().map(|v| Vertex {
                position: [v.pos.x, v.pos.y],
                uv: [v.uv.x, v.uv.y],
                colour: v.color.to_array().map(|c| c as f32 / 255.),
            }));
            indices.extend(mesh.indices.iter().map(|i| i + base));
            let rect = clipped.clip_rect;
            let x = (rect.min.x * ppp).floor().clamp(0., output.width() as f32) as usize;
            let y = (rect.min.y * ppp).floor().clamp(0., output.height() as f32) as usize;
            let x1 = (rect.max.x * ppp)
                .ceil()
                .clamp(x as f32, output.width() as f32) as usize;
            let y1 = (rect.max.y * ppp)
                .ceil()
                .clamp(y as f32, output.height() as f32) as usize;
            batches.push((
                mesh.texture_id,
                start,
                mesh.indices.len(),
                MTLScissorRect {
                    x,
                    y,
                    width: x1 - x,
                    height: y1 - y,
                },
            ));
        }
        if vertices.is_empty() {
            for id in &frame.textures.free {
                self.textures.remove(id);
            }
            return Ok(0);
        }
        let vbytes = bytemuck::cast_slice::<Vertex, u8>(&vertices);
        let ibytes = bytemuck::cast_slice::<u32, u8>(&indices);
        ensure!(
            vbytes.len() <= CAPACITY && ibytes.len() <= CAPACITY,
            "HUD geometry budget exceeded"
        );
        let ring = &self.rings[number % 3];
        unsafe {
            std::ptr::copy_nonoverlapping(
                vbytes.as_ptr(),
                ring.vertices.contents().as_ptr().cast::<u8>(),
                vbytes.len(),
            );
            std::ptr::copy_nonoverlapping(
                ibytes.as_ptr(),
                ring.indices.contents().as_ptr().cast::<u8>(),
                ibytes.len(),
            );
        }
        let pass = MTLRenderPassDescriptor::new();
        if let Some(counters) = counters {
            counters.attach(&pass, 6);
        }
        let c = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        c.setTexture(Some(output));
        c.setLoadAction(MTLLoadAction::Load);
        c.setStoreAction(MTLStoreAction::Store);
        let enc = cb
            .renderCommandEncoderWithDescriptor(&pass)
            .context("HUD encoder")?;
        enc.setRenderPipelineState(&self.pipeline);
        enc.setDepthStencilState(Some(&self.depth));
        enc.setCullMode(MTLCullMode::None);
        let logical = [
            output.width() as f32 / ppp,
            output.height() as f32 / ppp,
            0.,
            0.,
        ];
        unsafe {
            enc.setVertexBuffer_offset_atIndex(Some(&ring.vertices), 0, 0);
            enc.setVertexBytes_length_atIndex(NonNull::from(&logical).cast(), 16, 1);
        }
        let mut count = 0;
        for (id, start, n, clip) in batches {
            if clip.width == 0 || clip.height == 0 {
                continue;
            }
            let tex = self.textures.get(&id).context("HUD texture is missing")?;
            enc.setScissorRect(clip);
            unsafe {
                enc.setFragmentTexture_atIndex(Some(tex), 0);
                enc.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset(
                    MTLPrimitiveType::Triangle,
                    n,
                    MTLIndexType::UInt32,
                    &ring.indices,
                    start * 4,
                );
            }
            count += 1;
        }
        enc.endEncoding();
        for id in &frame.textures.free {
            self.textures.remove(id);
        }
        Ok(count)
    }
}
