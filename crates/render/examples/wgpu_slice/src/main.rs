//! Isolated renderer comparison. No files in the asset catalogues are written.
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use serde::Serialize;
use std::{path::PathBuf, sync::Arc, time::Instant};

#[cfg(target_os = "macos")]
pub use render::{
    Camera, appearance, character, ktx, model, scenery, settings, terrain, visibility,
};
// Reuse the existing portable decoding and scene code, without making the shipping crate
// depend on wgpu or changing its Metal-only public Camera while other issues own that API.
#[cfg(not(target_os = "macos"))]
#[path = "../../../src/appearance.rs"]
pub mod appearance;
#[cfg(not(target_os = "macos"))]
pub mod character {
    // Scenery's rigid draw contract. Skinning and character animation are outside this spike.
    pub struct Draw {
        pub dye: Option<std::sync::Arc<crate::appearance::Dye>>,
        pub base_map: Option<std::sync::Arc<crate::model::Texture>>,
        pub instance: u64,
        pub model: std::sync::Arc<crate::model::Model>,
        pub primitive: usize,
        pub transform: glam::Mat4,
        pub palette: std::sync::Arc<[glam::Mat4]>,
        pub hidden: u32,
        pub base: [f32; 4],
        pub morph: [f32; 2],
    }
}
#[cfg(not(target_os = "macos"))]
#[path = "../../../src/ktx.rs"]
pub mod ktx;
#[cfg(not(target_os = "macos"))]
#[path = "../../../src/model.rs"]
pub mod model;
#[cfg(not(target_os = "macos"))]
#[path = "../../../src/scenery.rs"]
pub mod scenery;
#[cfg(not(target_os = "macos"))]
#[path = "../../../src/settings.rs"]
pub mod settings;
#[path = "../../../src/surfaces.rs"]
pub mod surfaces;
#[cfg(not(target_os = "macos"))]
#[path = "../../../src/terrain.rs"]
pub mod terrain;
#[cfg(not(target_os = "macos"))]
#[path = "../../../src/visibility.rs"]
pub mod visibility;

#[cfg(not(target_os = "macos"))]
#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub fov: f32,
}
#[cfg(not(target_os = "macos"))]
impl Camera {
    pub fn matrix(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective(self.fov, aspect, 0.1, 800.)
            * glam::camera::rh::view::look_at_mat4(self.eye, self.target, Vec3::Y)
    }
}

mod backend;

#[derive(Serialize)]
pub struct Options {
    assets: PathBuf,
    backend: String,
    capture: Option<PathBuf>,
    width: u32,
    height: u32,
    scale: f32,
    frames: usize,
    warmup: usize,
    validate: bool,
}
impl Options {
    fn parse() -> Result<Self> {
        let mut o = Self {
            assets: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../assets"),
            backend: "wgpu".into(),
            capture: None,
            width: 2560,
            height: 1600,
            scale: 0.75,
            frames: 480,
            warmup: 120,
            validate: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--validate" {
                o.validate = true;
                continue;
            }
            if arg == "--help" {
                println!(
                    "wgpu-slice [--backend wgpu|metal] [--assets PATH] [--width N] [--height N] [--scale F] [--frames N] [--warmup N] [--validate] [--capture PATH]\nOffscreen, fixed camera, one frame in flight. JSON report on stdout. Hold /tmp/aldermoor-gpu.lock for every run."
                );
                std::process::exit(0);
            }
            let value = args
                .next()
                .with_context(|| format!("missing value for {arg}"))?;
            match arg.as_str() {
                "--assets" => o.assets = value.into(),
                "--capture" => o.capture = Some(value.into()),
                "--backend" => o.backend = value,
                "--width" => o.width = value.parse()?,
                "--height" => o.height = value.parse()?,
                "--scale" => o.scale = value.parse()?,
                "--frames" => o.frames = value.parse()?,
                "--warmup" => o.warmup = value.parse()?,
                _ => anyhow::bail!("unknown argument {arg}"),
            }
        }
        ensure!(
            matches!(o.backend.as_str(), "wgpu" | "metal"),
            "unknown backend"
        );
        ensure!(
            o.width > 0 && o.height > 0 && o.width <= 8192 && o.height <= 8192,
            "invalid dimensions"
        );
        ensure!(
            o.scale.is_finite() && (0.25..=1.).contains(&o.scale),
            "invalid scale"
        );
        ensure!(o.frames > o.warmup, "frames must exceed warmup");
        Ok(o)
    }
}

pub const PASSES: [&str; 8] = [
    "shadow_0",
    "shadow_1",
    "shadow_2",
    "sky",
    "opaque",
    "water_transparent",
    "taa",
    "composite",
];
#[derive(Serialize)]
pub struct Sample {
    frame: usize,
    cpu_encode_ms: f64,
    frame_work_ms: f64,
    gpu_ms: f64,
    passes_ms: [f64; 8],
    draw_calls: usize,
}
#[derive(Serialize)]
struct Distribution {
    median: f64,
    p95: f64,
}
fn distribution(values: impl Iterator<Item = f64>) -> Distribution {
    let mut v: Vec<_> = values.collect();
    v.sort_by(f64::total_cmp);
    Distribution {
        median: v[v.len() / 2],
        p95: v[((v.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)],
    }
}
fn scene(o: &Options, camera: Camera) -> Result<Vec<character::Draw>> {
    let catalog = assets::Catalog::load(&o.assets)?;
    let mut cache = model::ModelCache::default();
    let terrain = terrain::valley();
    let mut draws: Vec<_> = (0..terrain.primitives.len())
        .map(|primitive| character::Draw {
            model: terrain.clone(),
            primitive,
            instance: 0x8000_0000_0000_0000 | primitive as u64,
            transform: Mat4::IDENTITY,
            palette: Arc::from([]),
            hidden: 0,
            base: [1.; 4],
            morph: [0.; 2],
            dye: None,
            base_map: None,
        })
        .collect();
    draws.extend(
        scenery::Scenery::new(&catalog, &mut cache)?
            .draws(&catalog, &mut cache, camera, o.height, 0.)?,
    );
    Ok(draws)
}
#[cfg(target_os = "macos")]
fn metal(o: &Options, camera: Camera, draws: &[character::Draw]) -> Result<Vec<Sample>> {
    let mut r = render::Renderer::new(o.width, o.height, o.scale)?;
    r.look = settings::LookSettings::load(&o.assets.join("catalog/look.json"))?;
    r.load_surface_assets(&o.assets.join("catalog/surfaces.json"), &o.assets)?;
    r.set_frames_in_flight(1)?;
    r.set_gpu_counters(true)?;
    r.profile_scene = true;
    let mut samples = Vec::new();
    for frame in 0..o.frames {
        r.scene_time = Some(frame as f32 / 60.);
        let start = Instant::now();
        r.draw(
            camera,
            draws,
            if frame + 1 == o.frames {
                o.capture.as_deref()
            } else {
                None
            },
        )?;
        let cpu_encode_ms = start.elapsed().as_secs_f64() * 1000.;
        r.wait()?;
        let frame_work_ms = start.elapsed().as_secs_f64() * 1000.;
        for f in r.take_completed_frames() {
            if frame < o.warmup {
                continue;
            }
            let mut passes_ms = [0.; 8];
            for (i, name) in PASSES.iter().enumerate() {
                let index = render::GPU_PASS_NAMES
                    .iter()
                    .position(|n| n == name)
                    .unwrap();
                passes_ms[i] = f.passes[index]
                    .context("Metal pass timestamp missing")?
                    .elapsed_ms;
            }
            samples.push(Sample {
                frame,
                cpu_encode_ms,
                frame_work_ms,
                gpu_ms: f.gpu_ms,
                passes_ms,
                draw_calls: f.draw_calls,
            });
        }
    }
    Ok(samples)
}
fn main() -> Result<()> {
    let o = Options::parse()?;
    let started = Instant::now();
    // The supplied river review camera, identical for both backends and all runs.
    let camera = Camera {
        eye: Vec3::new(12., 9., -41.),
        target: Vec3::new(2., -0.8, -59.),
        fov: 1.,
    };
    let draws = scene(&o, camera)?;
    let (device, samples) = if o.backend == "metal" {
        #[cfg(target_os = "macos")]
        {
            ("native Metal".to_string(), metal(&o, camera, &draws)?)
        }
        #[cfg(not(target_os = "macos"))]
        anyhow::bail!("native Metal comparison requires macOS");
    } else {
        backend::run(&o, camera, &draws)?
    };
    ensure!(!samples.is_empty(), "no measured samples");
    let report = serde_json::json!({
        "settings": o, "device": device, "passes": PASSES,
        "debug_build": cfg!(debug_assertions),
        "gpu_ms": distribution(samples.iter().map(|s| s.gpu_ms)),
        "cpu_encode_ms": distribution(samples.iter().map(|s| s.cpu_encode_ms)),
        "frame_work_ms": distribution(samples.iter().map(|s| s.frame_work_ms)),
        "wall_seconds": started.elapsed().as_secs_f64(), "samples": samples,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
