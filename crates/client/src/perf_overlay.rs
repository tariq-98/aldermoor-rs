//! F3 diagnostics and completed-frame reporting, separate from gameplay HUD and bindings.
use render::{GPU_PASS_NAMES, GpuFrame, Renderer};
use serde_json::{Value, json};
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
};

#[derive(Default)]
pub struct Options {
    pub disabled: bool,
    pub visible: bool,
    pub split_scene: bool,
    pub overhead_check: bool,
}
#[derive(Default)]
pub struct Overlay {
    visible: bool,
    cpu_ms: f64,
    frames: Vec<GpuFrame>,
    latest: Option<GpuFrame>,
    /// This frame's character animation, and every recorded frame's.
    animation: Animation,
    animations: Vec<Animation>,
}
/// A frame's character animation on the CPU (#215).
#[derive(Clone, Copy, Default)]
pub struct Animation {
    /// Posing the characters, which `anim::Crowd` keeps inside its budget, in milliseconds.
    pub pose_ms: f64,
    /// Building their skinning palettes and draws, in milliseconds.
    pub palettes_ms: f64,
    /// How many characters rebuild every frame, every other frame and every fourth.
    pub tiers: [usize; 3],
}
impl Options {
    pub fn validate(&self, samples: usize) -> anyhow::Result<()> {
        anyhow::ensure!(
            !(self.disabled && (self.split_scene || self.overhead_check)),
            "profiling requires GPU counters"
        );
        anyhow::ensure!(
            !(self.overhead_check && (self.split_scene || self.visible)),
            "counter overhead requires the normal scene layout and overlay off"
        );
        anyhow::ensure!(
            !self.split_scene || samples <= 1,
            "split scene profiling requires single-sample rendering"
        );
        Ok(())
    }
}
impl Overlay {
    pub fn new(options: &Options) -> Self {
        Self {
            visible: options.visible,
            ..Default::default()
        }
    }
    pub fn configure(renderer: &mut Renderer, options: &Options) {
        renderer.profile_scene = options.split_scene;
        if let Err(error) = renderer.set_gpu_counters(!options.disabled) {
            eprintln!("GPU_COUNTERS unavailable: {error:#}");
        } else {
            eprintln!(
                "GPU_COUNTERS {}",
                if options.disabled {
                    "disabled"
                } else {
                    "stage-boundary"
                }
            );
        }
    }
    pub fn begin_frame(renderer: &mut Renderer, frame: u64, options: &Options) {
        // Balance ABBA/BAAB blocks across jitter phases without reallocating or draining.
        renderer.sample_gpu_counters = !options.overhead_check || counter_sampled(frame);
    }
    pub fn event(&mut self, event: &WindowEvent) -> bool {
        if let WindowEvent::KeyboardInput { event, .. } = event
            && event.physical_key == PhysicalKey::Code(KeyCode::F3)
        {
            if event.state == ElementState::Pressed && !event.repeat {
                self.visible = !self.visible;
            }
            return true;
        }
        false
    }
    pub fn collect(&mut self, renderer: &mut Renderer, cpu_ms: f64, warmup: u64, record: bool) {
        self.cpu_ms = cpu_ms;
        for sample in renderer.take_completed_frames() {
            if self.latest.is_none_or(|last| sample.frame > last.frame) {
                self.latest = Some(sample);
            }
            if record && sample.frame >= warmup as usize {
                self.frames.push(sample);
            }
        }
    }
    pub fn draw(
        &self,
        ctx: &egui::Context,
        sim_ms: f64,
        enabled: bool,
        pacing: &crate::pacing::Stats,
    ) {
        if !self.visible {
            return;
        }
        egui::Window::new("Performance · F3")
            .default_pos([12., 220.])
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("CPU encode + slot wait: {:.3} ms", self.cpu_ms));
                ui.label(format!("Sim step (rolling mean): {sim_ms:.3} ms"));
                let (mean, spread) = pacing.interval_ms();
                let [none, one, more] = pacing.steps();
                ui.label(format!("Frame interval: {mean:.2} ms ± {spread:.2} ms"));
                ui.label(format!(
                    "Sim steps per frame (last 2 s): 0 × {none} · 1 × {one} · 2+ × {more}"
                ));
                let a = self.animation;
                ui.label(format!(
                    "Animation: posing {:.3} ms / {} ms · palettes {:.3} ms",
                    a.pose_ms,
                    anim::lod::BUDGET_MS,
                    a.palettes_ms
                ));
                ui.label(format!(
                    "Characters: {} every frame · {} every other · {} every fourth",
                    a.tiers[0], a.tiers[1], a.tiers[2]
                ));
                if let Some(frame) = self.latest {
                    ui.label(format!("GPU frame: {:.3} ms / 12 ms", frame.gpu_ms));
                    ui.label(format!(
                        "{} draws · {} visible triangles",
                        frame.draw_calls, frame.visible_triangles
                    ));
                    ui.label(format!(
                        "Metal allocated: {:.1} MiB",
                        frame.allocated_gpu_bytes as f64 / 1048576.
                    ));
                    ui.separator();
                    if !enabled {
                        ui.label("GPU counters disabled or unavailable");
                    }
                    for (name, timing) in GPU_PASS_NAMES.iter().zip(frame.passes) {
                        ui.label(match timing {
                            Some(t) => format!(
                                "{name}: {:.3} ms (V {:.3}, F {:.3})",
                                t.elapsed_ms, t.vertex_ms, t.fragment_ms
                            ),
                            None => format!("{name}: unavailable / not encoded"),
                        });
                    }
                    ui.label("Scene combined by default; split profiling changes tile costs.");
                    ui.label("Pass intervals can overlap; do not sum them.");
                } else {
                    ui.label("Waiting for a completed GPU frame");
                }
            });
    }
    /// Takes in this frame's character animation, to show and, when `record`, to sum up at the
    /// end of the run.
    pub fn animate(&mut self, animation: Animation, record: bool) {
        self.animation = animation;
        if record {
            self.animations.push(animation);
        }
    }
    /// The ANIM line a run ends with: posing and palettes a frame, median and 95th percentile,
    /// and how many characters rebuilt at each rate on average.
    pub fn animation_report(&self) -> Option<String> {
        let n = self.animations.len();
        if n == 0 {
            return None;
        }
        let spread = |ms: fn(&Animation) -> f64| {
            let mut v: Vec<_> = self.animations.iter().map(ms).collect();
            v.sort_by(f64::total_cmp);
            format!(
                "median={:.3}ms p95={:.3}ms",
                v[n / 2],
                v[(n * 95 / 100).min(n - 1)]
            )
        };
        let mean = |tier: usize| {
            self.animations.iter().map(|a| a.tiers[tier]).sum::<usize>() as f64 / n as f64
        };
        Some(format!(
            "ANIM characters={} pose {} palettes {} full={:.1} half={:.1} quarter={:.1} samples={n}",
            self.animation.tiers.iter().sum::<usize>(),
            spread(|a| a.pose_ms),
            spread(|a| a.palettes_ms),
            mean(0),
            mean(1),
            mean(2)
        ))
    }
    pub fn report(&mut self, counters_enabled: bool) {
        self.frames.sort_by_key(|f| f.frame);
        eprintln!(
            "PERF {}",
            report(&self.frames, counters_enabled, self.visible)
        );
    }
}
fn counter_sampled(frame: u64) -> bool {
    matches!(frame % 4, 1 | 2) ^ ((frame / 4).count_ones() % 2 == 1)
}
fn distribution(values: impl Iterator<Item = f64>) -> Value {
    let mut values: Vec<_> = values.filter(|v| v.is_finite() && *v >= 0.).collect();
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n == 0 {
        return Value::Null;
    }
    json!({"median": values[n / 2], "p95": values[(n * 95 / 100).min(n - 1)], "samples": n})
}
fn report(frames: &[GpuFrame], counters_enabled: bool, overlay_visible: bool) -> Value {
    let mut passes = serde_json::Map::new();
    for (index, name) in GPU_PASS_NAMES.iter().enumerate() {
        passes.insert((*name).to_owned(), json!({
            "elapsed_ms": distribution(frames.iter().filter_map(|f| f.passes[index].map(|t| t.elapsed_ms))),
            "vertex_ms": distribution(frames.iter().filter_map(|f| f.passes[index].map(|t| t.vertex_ms))),
            "fragment_ms": distribution(frames.iter().filter_map(|f| f.passes[index].map(|t| t.fragment_ms))),
            "missing_samples": frames.iter().filter(|f| f.counters_sampled && f.passes[index].is_none()).count(),
        }));
    }
    let start = frames.first().map_or(0., |f| f.elapsed_seconds);
    let mut minutes: std::collections::BTreeMap<u64, Vec<f64>> = Default::default();
    for frame in frames {
        let minute = ((frame.elapsed_seconds - start).max(0.) / 60.).floor() as u64;
        minutes.entry(minute).or_default().push(frame.gpu_ms);
    }
    let sampled = frames.iter().filter(|f| f.counters_sampled).count();
    let mut blocks: std::collections::BTreeMap<usize, Vec<&GpuFrame>> = Default::default();
    for frame in frames {
        blocks.entry(frame.frame / 4).or_default().push(frame);
    }
    let paired_deltas: Vec<f64> = blocks
        .values()
        .filter_map(|block| {
            let on: Vec<_> = block.iter().filter(|f| f.counters_sampled).collect();
            let off: Vec<_> = block.iter().filter(|f| !f.counters_sampled).collect();
            (on.len() == 2 && off.len() == 2).then(|| {
                (on.iter().map(|f| f.gpu_ms).sum::<f64>()
                    - off.iter().map(|f| f.gpu_ms).sum::<f64>())
                    / 2.
            })
        })
        .collect();
    let comparison = json!({
        "on_gpu_ms": distribution(frames.iter().filter(|f| f.counters_sampled).map(|f| f.gpu_ms)),
        "off_gpu_ms": distribution(frames.iter().filter(|f| !f.counters_sampled).map(|f| f.gpu_ms)),
        "paired_block_deltas_ms": paired_deltas,
        "method": "Balanced ABBA/BAAB when --counter-overhead is set; on minus off block means; buffers preallocated in both groups"
    });
    json!({
        "counter_sampled_frames": sampled,
        "counter_comparison": comparison,
        "counters_enabled": counters_enabled, "overlay_visible": overlay_visible,
        "samples": frames.len(), "first_frame": frames.first().map(|f| f.frame),
        "last_frame": frames.last().map(|f| f.frame),
        "gpu_ms": distribution(frames.iter().map(|f| f.gpu_ms)),
        "passes": passes,
        "gpu_by_minute": minutes.iter().map(|(minute, values)| json!({"minute": minute, "gpu_ms": distribution(values.iter().copied())})).collect::<Vec<_>>(),
        "draw_calls": distribution(frames.iter().map(|f| f.draw_calls as f64)),
        "visible_triangles": distribution(frames.iter().map(|f| f.visible_triangles as f64)),
        "allocated_gpu_bytes": distribution(frames.iter().map(|f| f.allocated_gpu_bytes as f64)),
        "peak_allocated_gpu_bytes": frames.iter().map(|f| f.allocated_gpu_bytes).max(),
        "memory_measure": "MTLDevice.currentAllocatedSize; allocations, not physical residency",
        "pass_measure": "stage timestamps; elapsed vertex-start to fragment-end; intervals may overlap",
        "scene_measure": "combined by default; --gpu-profile-scene splits sky/opaque/water-transparent encoders and changes tile load/store costs",
        "metalfx_measure": "opaque framework encoders are not instrumented; taa is null when MetalFX is active"
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overhead_blocks_balance_sampling_across_jitter_phases() {
        for block in 0..32 {
            assert_eq!(
                (block * 4..block * 4 + 4)
                    .filter(|f| counter_sampled(*f))
                    .count(),
                2
            );
        }
        for phase in 0..16 {
            assert_eq!(
                (0..8)
                    .filter(|cycle| counter_sampled(cycle * 16 + phase))
                    .count(),
                4
            );
        }
    }
    #[test]
    fn summaries_keep_missing_passes_missing_and_bucket_wall_time() {
        let mut a = GpuFrame {
            frame: 120,
            elapsed_seconds: 20.,
            gpu_ms: 10.,
            ..Default::default()
        };
        a.passes[3] = Some(render::PassTiming {
            elapsed_ms: 4.,
            vertex_ms: 1.,
            fragment_ms: 2.,
        });
        let b = GpuFrame {
            frame: 121,
            elapsed_seconds: 80.01,
            gpu_ms: 12.,
            ..a
        };
        let r = report(&[a, b], true, false);
        assert_eq!(r["passes"]["taa"]["elapsed_ms"], Value::Null);
        assert_eq!(r["passes"][GPU_PASS_NAMES[3]]["elapsed_ms"]["samples"], 2);
        assert_eq!(r["gpu_by_minute"][0]["gpu_ms"]["median"], 10.);
        assert_eq!(r["gpu_by_minute"][1]["gpu_ms"]["median"], 12.);
        assert_eq!(r["first_frame"], 120);
        assert_eq!(
            distribution([f64::NAN, -1., f64::INFINITY].into_iter()),
            Value::Null
        );
    }
    #[test]
    fn overlay_builds_real_egui_geometry_without_authored_style() {
        let ctx = egui::Context::default();
        let overlay = Overlay {
            visible: true,
            latest: Some(GpuFrame::default()),
            ..Default::default()
        };
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            overlay.draw(ctx, 1., true, &crate::pacing::Stats::default())
        });
        assert!(!output.shapes.is_empty());
    }
}
