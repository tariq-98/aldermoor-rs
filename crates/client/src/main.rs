//! A windowed Metal client; the simulation remains entirely behind Host and protocol messages.
mod hud;
mod interpolate;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, Vec3};
use protocol::{Command, Look, Pose, UnitState};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use render::{
    Camera, Renderer,
    character::{Character, Draw},
    model::{Model, ModelCache},
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Fullscreen, Window, WindowId},
};

struct Options {
    assets: PathBuf,
    width: u32,
    height: u32,
    scale: f32,
    capture: Option<PathBuf>,
    frames: Option<u64>,
    offscreen: bool,
    showcase: bool,
    smoke: bool,
    inspect: Option<String>,
    warmup: u64,
    samples: usize,
    in_flight: usize,
    fps: Option<f64>,
    benchmark: bool,
    inline_constants: bool,
    fullscreen: bool,
    look_settings: Option<PathBuf>,
    surface_assets: Option<PathBuf>,
    hud_font: Option<PathBuf>,
}
impl Options {
    fn read() -> Result<Self> {
        let mut o = Self {
            assets: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
            width: 1600,
            height: 1000,
            scale: 0.8,
            capture: None,
            frames: None,
            offscreen: false,
            showcase: false,
            smoke: false,
            inspect: None,
            warmup: 120,
            samples: 0,
            in_flight: 1,
            fps: None,
            benchmark: false,
            inline_constants: true,
            fullscreen: false,
            look_settings: None,
            surface_assets: None,
            hud_font: None,
        };
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--assets" => {
                    o.assets = PathBuf::from(args.next().context("--assets needs a path")?)
                }
                "--size" => {
                    let s = args.next().context("--size needs WIDTHxHEIGHT")?;
                    let (w, h) = s.split_once('x').context("expected WIDTHxHEIGHT")?;
                    o.width = w.parse()?;
                    o.height = h.parse()?;
                }
                "--scale" => o.scale = args.next().context("--scale needs a number")?.parse()?,
                "--capture" => {
                    o.capture = Some(PathBuf::from(
                        args.next().context("--capture needs a .png path")?,
                    ))
                }
                "--frames" => {
                    o.frames = Some(args.next().context("--frames needs a count")?.parse()?)
                }
                "--aa" => {
                    o.samples = match args.next().as_deref() {
                        Some("none") => 1,
                        Some("msaa") => 4,
                        Some("taa") => 0,
                        _ => anyhow::bail!("--aa expects taa, none or msaa"),
                    }
                }
                "--in-flight" => {
                    o.in_flight = args.next().context("--in-flight needs 1..3")?.parse()?
                }
                "--fps" => o.fps = Some(args.next().context("--fps needs a number")?.parse()?),
                "--benchmark" => o.benchmark = true,
                "--shared-constants" => o.inline_constants = false,
                "--fullscreen" => o.fullscreen = true,
                "--look-settings" => {
                    o.look_settings =
                        Some(args.next().context("--look-settings needs a file")?.into())
                }
                "--surface-assets" => {
                    o.surface_assets =
                        Some(args.next().context("--surface-assets needs a file")?.into())
                }
                "--hud-font" => {
                    o.hud_font = Some(args.next().context("--hud-font needs a file")?.into())
                }
                "--offscreen" => o.offscreen = true,
                "--warmup" => o.warmup = args.next().context("--warmup needs a count")?.parse()?,
                "--showcase" => o.showcase = true,
                "--smoke" => o.smoke = true,
                "--inspect" => {
                    o.inspect = Some(args.next().context("--inspect needs an art id")?);
                    o.showcase = true;
                }
                "--help" => {
                    println!(
                        "Aldermoor Metal client\nWASD move · Shift walk · Space jump · V dodge · Tab target\nRight-drag orbit/strafe · Left-click target · 1/2 attacks · R respawn · Esc quit\n\n--size WIDTHxHEIGHT --scale 0.25..1 --assets PATH\n--capture FILE.png --frames N --offscreen --showcase --inspect ART --smoke\n--fullscreen --aa taa|none|msaa --fps N --benchmark --warmup N --in-flight 1..3\n--look-settings FILE --surface-assets FILE --hud-font FILE\nShowcase is a catalogue review scene; smoke sends movement/jump/dodge/combat commands."
                    );
                    std::process::exit(0)
                }
                _ => anyhow::bail!("unknown option {a}"),
            }
        }
        ensure!(
            o.width > 0 && o.height > 0 && o.width <= 8192 && o.height <= 8192,
            "size must be between 1 and 8192 pixels"
        );
        if o.offscreen && o.frames.is_none() {
            o.frames = Some(120)
        }
        if let Some(p) = &o.capture
            && let Some(parent) = p.parent()
        {
            std::fs::create_dir_all(parent)?;
        }
        ensure!(
            o.fps
                .is_none_or(|fps| fps.is_finite() && (1.0..=240.).contains(&fps)),
            "--fps must be 1..240"
        );
        Ok(o)
    }
}
struct Game {
    options: Options,
    host: server::Host,
    snaps: interpolate::Snapshots,
    catalog: assets::Catalog,
    cache: ModelCache,
    characters: HashMap<u32, Character>,
    terrain: Arc<Model>,
    scenery: Option<render::scenery::Scenery>,
    review: Vec<UnitState>,
    yaw: f32,
    pitch: f32,
    distance: f32,
    keys: HashSet<KeyCode>,
    mouse_look: bool,
    last_move: Option<Command>,
    last: Instant,
    start: Instant,
    frame: u64,
    gpu: Vec<f64>,
    frame_times: Vec<f64>,
    intervals: Vec<f64>,
    finished_at: Instant,
    smoke_stage: usize,
    events: usize,
    smoke_start: Vec3,
    smoke_checks: [bool; 3],
    hud_context: egui::Context,
    hud_input: Option<egui::RawInput>,
    platform_output: Option<egui::PlatformOutput>,
}
impl Game {
    fn new(options: Options) -> Result<Self> {
        let catalog = assets::Catalog::load(&options.assets)?;
        let host = server::Host::start(0xa1de, "Traveller");
        let first = host.snapshot().unwrap();
        let smoke_start = first.unit(host.player).unwrap().position;
        let mut g = Self {
            options,
            host,
            snaps: interpolate::Snapshots::new(first),
            catalog,
            cache: ModelCache::default(),
            characters: HashMap::new(),
            terrain: render::terrain::valley(),
            scenery: None,
            review: Vec::new(),
            yaw: std::f32::consts::PI,
            pitch: 0.23,
            distance: 7.,
            keys: HashSet::new(),
            mouse_look: false,
            last_move: None,
            last: Instant::now(),
            start: Instant::now(),
            frame: 0,
            gpu: Vec::new(),
            frame_times: Vec::new(),
            intervals: Vec::new(),
            finished_at: Instant::now(),
            smoke_stage: 0,
            events: 0,
            smoke_start,
            smoke_checks: [false; 3],
            hud_context: egui::Context::default(),
            hud_input: None,
            platform_output: None,
        };
        g.scenery = Some(render::scenery::Scenery::new(&g.catalog, &mut g.cache)?);
        if g.options.showcase {
            g.review = showcase(g.snaps.current.unit(g.host.player).unwrap().clone());
            if let Some(goblin) = g
                .snaps
                .current
                .units
                .iter()
                .find(|u| u.look.art == "goblin_male")
                && let Some(review) = g.review.iter_mut().find(|u| u.look.art == "goblin_male")
            {
                review.look = goblin.look.clone();
            }
            if let Some(art) = &g.options.inspect {
                let mut u=g.review.iter().find(|u|u.look.art==*art).cloned().context("--inspect supports male, female, elf_female, dwarf_male, goblin_male, wolf.grey")?;
                u.id = g.host.player;
                u.position = Vec3::new(6., world::height(6., 28.), 28.);
                u.yaw = 0.;
                g.review = vec![u];
            }
        }
        let units = if g.options.showcase {
            g.review.clone()
        } else {
            g.snaps.current.units.clone()
        };
        for u in units {
            g.characters
                .insert(u.id, Character::load(&g.catalog, &mut g.cache, &u.look)?);
        }
        if let Some(path) = &g.options.hud_font {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "supplied".into(),
                egui::FontData::from_owned(std::fs::read(path)?).into(),
            );
            fonts
                .families
                .insert(egui::FontFamily::Proportional, vec!["supplied".into()]);
            g.hud_context.set_fonts(fonts);
        }
        g.start = Instant::now();
        g.last = g.start;
        Ok(g)
    }
    fn direction(&self) -> Vec2 {
        let axis = |positive, negative| {
            (self.keys.contains(&positive) as i32 - self.keys.contains(&negative) as i32) as f32
        };
        let forward = Vec2::new(self.yaw.sin(), self.yaw.cos());
        let right = Vec2::new(-self.yaw.cos(), self.yaw.sin());
        (forward * axis(KeyCode::KeyW, KeyCode::KeyS) + right * axis(KeyCode::KeyD, KeyCode::KeyA))
            .normalize_or_zero()
    }
    fn controls(&mut self) {
        let c = Command::Move {
            direction: self.direction(),
            walk: self.keys.contains(&KeyCode::ShiftLeft)
                || self.keys.contains(&KeyCode::ShiftRight),
            face_yaw: self.mouse_look.then_some(self.yaw),
        };
        if self.last_move.as_ref() != Some(&c) {
            self.host.send(c.clone());
            self.last_move = Some(c);
        }
    }
    fn camera(&self, p: Vec3) -> Camera {
        if self.options.inspect.is_some() {
            let u = &self.review[0];
            let target = u.position + Vec3::Y * 0.95;
            return Camera {
                eye: target + Vec3::new(1.8, 0.6, 4.),
                target,
                fov: 36f32.to_radians(),
            };
        }
        if self.options.showcase {
            let target = Vec3::new(2., world::height(2., 22.) + 1.25, 22.);
            return Camera {
                eye: target + Vec3::new(8., 3.5, 14.),
                target,
                fov: 45f32.to_radians(),
            };
        }
        let target = p + Vec3::Y * 1.25;
        let offset = Vec3::new(
            -self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            -self.yaw.cos() * self.pitch.cos(),
        ) * self.distance;
        let desired = target + offset;
        let mut eye = desired;
        for i in 1..=40 {
            let t = i as f32 / 40.;
            let at = target + offset * t;
            if at.y < world::height(at.x, at.z) + 0.35 {
                eye = target + offset * ((i - 1) as f32 / 40.);
                break;
            }
        }
        if let Some(scenery) = &self.scenery {
            eye = scenery.camera_eye(target, eye, 0.25);
        }
        eye.y = eye.y.max(world::height(eye.x, eye.z) + 0.35);
        Camera {
            eye,
            target,
            fov: 52f32.to_radians(),
        }
    }
    fn target(&self, cycle: bool) -> Option<u32> {
        let me = self.snaps.current.unit(self.host.player)?;
        let mut enemies: Vec<_> = self
            .snaps
            .current
            .units
            .iter()
            .filter(|u| u.hostile && u.health > 0. && u.position.distance(me.position) < 55.)
            .collect();
        enemies.sort_by(|a, b| {
            a.position
                .distance_squared(me.position)
                .total_cmp(&b.position.distance_squared(me.position))
        });
        if cycle && let Some(i) = enemies.iter().position(|u| Some(u.id) == me.target) {
            return enemies.get((i + 1) % enemies.len()).map(|u| u.id);
        }
        enemies.first().map(|u| u.id)
    }
    fn pick(&self, x: f64, y: f64, w: u32, h: u32) -> Option<u32> {
        let me = self.snaps.current.unit(self.host.player)?;
        let matrix = self.camera(me.position).matrix(w as f32 / h as f32);
        self.snaps
            .current
            .units
            .iter()
            .filter(|u| u.id != me.id && u.health > 0.)
            .filter_map(|u| {
                let p = matrix * (u.position + Vec3::Y).extend(1.);
                if p.w <= 0. {
                    return None;
                }
                let ndc = p.truncate() / p.w;
                let screen =
                    Vec2::new((ndc.x + 1.) * 0.5 * w as f32, (1. - ndc.y) * 0.5 * h as f32);
                let d = screen.distance(Vec2::new(x as f32, y as f32));
                (d < 65.).then_some((d, u.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|v| v.1)
    }
    fn smoke(&mut self) {
        let elapsed = self.start.elapsed().as_secs_f32();
        let schedule = [
            (0., 0),
            (0.8, 1),
            (2.0, 2),
            (3.0, 3),
            (6.0, 4),
            (7.0, 5),
            (8.0, 6),
        ];
        while self.smoke_stage < schedule.len() && elapsed >= schedule[self.smoke_stage].0 {
            match schedule[self.smoke_stage].1 {
                0 => self.host.send(Command::Move {
                    direction: Vec2::new(-1., 0.),
                    walk: false,
                    face_yaw: None,
                }),
                1 => self.host.send(Command::Jump),
                2 => self.host.send(Command::Dodge {
                    direction: Vec2::new(-1., 0.),
                }),
                3 => {
                    self.host.send(Command::SetTarget(self.target(false)));
                    self.host.send(Command::SetAutoAttack(true));
                }
                4 | 5 => self.host.send(Command::UseSlot {
                    slot: 0,
                    ground: None,
                }),
                _ => self.host.send(Command::Move {
                    direction: Vec2::ZERO,
                    walk: false,
                    face_yaw: None,
                }),
            }
            self.smoke_stage += 1;
        }
    }
    fn draw(&mut self, r: &mut Renderer) -> Result<bool> {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        if let Some(s) = self.host.snapshot() {
            self.snaps.push(s)
        }
        if self.options.smoke {
            self.smoke()
        } else {
            self.controls()
        }
        for e in self.host.take_events() {
            self.events += 1;
            match &e {
                protocol::Event::Jumped { unit } if *unit == self.host.player => {
                    self.smoke_checks[0] = true
                }
                protocol::Event::Dodged { unit } if *unit == self.host.player => {
                    self.smoke_checks[1] = true
                }
                protocol::Event::Damage { source, .. } if *source == self.host.player => {
                    self.smoke_checks[2] = true
                }
                _ => {}
            }
            if self.options.smoke {
                eprintln!("event {e:?}");
            }
        }
        let mut units = if self.options.showcase {
            self.review.clone()
        } else {
            self.snaps.units()
        };
        let time = self.frame as f32 / 60.;
        if self.options.showcase {
            for u in &mut units {
                u.pose_time = time;
            }
        }
        let me = units
            .iter()
            .find(|u| u.id == self.host.player)
            .context("no local player")?;
        let camera = self.camera(me.position);
        let mut draws = Vec::new();
        for i in 0..self.terrain.primitives.len() {
            draws.push(Draw {
                dye: None,
                base_map: None,
                instance: 0x8000_0000_0000_0000 | i as u64,
                model: self.terrain.clone(),
                primitive: i,
                transform: Mat4::IDENTITY,
                palette: Vec::new(),
                hidden: 0,
                base: [1.; 4],
                morph: [0.; 2],
            });
        }
        let scene_time = if self.options.showcase {
            time
        } else {
            self.snaps.current.time as f32
                + self.snaps.received.elapsed().as_secs_f32().min(1. / 60.)
        };
        if let Some(scenery) = &mut self.scenery {
            draws.extend(scenery.draws(
                &self.catalog,
                &mut self.cache,
                camera,
                r.height,
                scene_time,
            )?);
        }
        self.characters
            .retain(|id, _| units.iter().any(|u| u.id == *id));
        for u in &units {
            let mut look = u.look.clone();
            look.drawn = false;
            let height = self
                .characters
                .get(&u.id)
                .map(|c| c.rig_height)
                .or_else(|| {
                    self.catalog
                        .humanoid
                        .body_types
                        .get(&u.look.art)
                        .map(|b| b.height)
                })
                .unwrap_or(1.5);
            let depth = (u.position - camera.eye).dot((camera.target - camera.eye).normalize());
            let pixels = render::visibility::screen_height(height, depth, camera.fov, r.height);
            let lod = render::visibility::lod_for_height(
                pixels,
                self.characters.get(&u.id).map(|c| c.lod),
            );
            let rebuild = self.characters.get(&u.id).is_none_or(|c| {
                let mut old = c.look.clone();
                old.drawn = false;
                old != look || c.lod != lod
            });
            if rebuild {
                if self.characters.get(&u.id).is_some_and(|c| {
                    let mut previous = c.look.clone();
                    previous.drawn = false;
                    previous != look
                }) {
                    r.reset_temporal_history();
                }
                self.characters.insert(
                    u.id,
                    Character::load_lod(&self.catalog, &mut self.cache, &u.look, lod)?,
                );
            }
            let c = self.characters.get_mut(&u.id).unwrap();
            c.update(u, if self.options.showcase { 1. / 60. } else { dt });
            draws.extend(c.draws(u.position, u.yaw).into_iter().map(|mut d| {
                d.instance |= (u.id as u64) << 32;
                d
            }));
        }
        let player = units
            .iter()
            .find(|u| u.id == self.host.player)
            .context("no local player in snapshot")?;
        self.frame += 1;
        let done = self.options.frames.is_some_and(|n| self.frame >= n);
        let capture = if done {
            self.options.capture.as_deref()
        } else {
            None
        };
        let raw = self.hud_input.take().unwrap_or_else(|| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(r.width as f32, r.height as f32),
            )),
            time: Some(time as f64),
            ..Default::default()
        });
        let ctx = self.hud_context.clone();
        let mut actions = Vec::new();
        let output = ctx.run(raw, |ctx| {
            actions = hud::draw(ctx, &self.snaps.current, None);
        });
        for action in actions {
            match action {
                hud::Action::Command(c) => self.host.send(c),
                hud::Action::CycleTarget => self.host.send(Command::SetTarget(self.target(true))),
            }
        }
        r.hud = Some(render::gui::HudFrame {
            primitives: ctx.tessellate(output.shapes, output.pixels_per_point),
            textures: output.textures_delta,
            pixels_per_point: output.pixels_per_point,
        });
        self.platform_output = Some(output.platform_output);
        r.scene_time = Some(scene_time);
        r.inline_constants = self.options.inline_constants;
        r.target_fps = self.options.fps.unwrap_or(60.);
        let mut stats = r.draw(self.camera(player.position), &draws, capture)?;
        if self.options.benchmark {
            r.wait()?;
            stats = r.stats;
        }
        let finished = Instant::now();
        if self.frame > self.options.warmup && !done {
            self.gpu.push(stats.gpu_ms);
            self.intervals
                .push((finished - self.finished_at).as_secs_f64() * 1000.);
            self.frame_times.push(if self.options.benchmark {
                (finished - now).as_secs_f64() * 1000.
            } else {
                (finished - self.finished_at).as_secs_f64() * 1000.
            });
        }
        self.finished_at = finished;
        if self.frame.is_multiple_of(120) || done {
            eprintln!(
                "frame={} gpu={:.2}ms draws={} triangles={} sim={:.3}ms units={}",
                self.frame,
                stats.gpu_ms,
                stats.draw_calls,
                stats.triangles,
                self.host.stats().step_ms,
                units.len()
            );
        }
        if done {
            r.wait()?;
            let presentations = r.presentation_times(self.options.warmup as usize);
            if presentations.len() > 2 {
                let mut intervals: Vec<_> = presentations
                    .windows(2)
                    .map(|w| (w[1] - w[0]) * 1000.)
                    .filter(|t| *t > 0.)
                    .collect();
                let mean = intervals.iter().sum::<f64>() / intervals.len() as f64;
                intervals.sort_by(f64::total_cmp);
                eprintln!(
                    "PRESENT mean_fps={:.3} p95={:.3}ms samples={}",
                    1000. / mean,
                    intervals[intervals.len() * 95 / 100],
                    intervals.len()
                );
            }
            self.gpu.sort_by(f64::total_cmp);
            if !self.intervals.is_empty() {
                let fps =
                    1000. / (self.intervals.iter().sum::<f64>() / self.intervals.len() as f64);
                self.intervals.sort_by(f64::total_cmp);
                eprintln!(
                    "INTERVAL mean_fps={fps:.3} p95={:.3}ms",
                    self.intervals[self.intervals.len() * 95 / 100]
                );
            }
            self.frame_times.sort_by(f64::total_cmp);
            if !self.frame_times.is_empty() {
                let n = self.frame_times.len();
                eprintln!(
                    "FRAME median={:.3}ms p95={:.3}ms samples={} mode={}",
                    self.frame_times[n / 2],
                    self.frame_times[(n * 95 / 100).min(n - 1)],
                    n,
                    if self.options.offscreen {
                        if self.options.benchmark {
                            "offscreen-synchronized"
                        } else {
                            "offscreen-throughput"
                        }
                    } else {
                        "windowed"
                    }
                );
            }
            if !self.gpu.is_empty() {
                eprintln!(
                    "GPU median={:.3}ms p95={:.3}ms samples={}",
                    self.gpu[self.gpu.len() / 2],
                    self.gpu[(self.gpu.len() * 95 / 100).min(self.gpu.len() - 1)],
                    self.gpu.len()
                );
            }
            if self.options.smoke {
                let moved = player.position.distance(self.smoke_start);
                ensure!(
                    moved > 2. && self.smoke_checks.iter().all(|x| *x),
                    "smoke did not exercise simulation: moved={moved}, events={}, jump/dodge/damage={:?}",
                    self.events,
                    self.smoke_checks
                );
                eprintln!("SMOKE PASS moved={moved:.2}m events={}", self.events);
            }
        }
        Ok(done)
    }
}
fn showcase(mut base: UnitState) -> Vec<UnitState> {
    let outfits = [
        (
            "male",
            vec!["peasant.top", "peasant.trousers", "peasant.shoes"],
            "sword.arming",
            Some("shield.round"),
            false,
        ),
        (
            "elf_female",
            vec!["ranger.top", "ranger.trousers", "ranger.boots"],
            "bow.hunting",
            None,
            true,
        ),
        (
            "dwarf_male",
            vec![
                "peasant.top",
                "peasant.trousers",
                "peasant.shoes",
                "plate.cuirass",
                "plate.legs",
                "plate.gauntlets",
                "plate.pauldrons",
            ],
            "sword.great",
            None,
            true,
        ),
        (
            "female",
            vec!["robe.healer", "peasant.shoes"],
            "staff.healer",
            None,
            true,
        ),
        (
            "goblin_male",
            vec!["ranger.top", "ranger.trousers", "ranger.boots"],
            "dagger.hunting",
            Some("shield.round"),
            true,
        ),
    ];
    let first = base.id;
    let mut units = Vec::new();
    for (i, (art, pieces, weapon, shield, drawn)) in outfits.into_iter().enumerate() {
        base.id = first + i as u32;
        base.position = Vec3::new(-4. + i as f32 * 2.8, 0., 22.);
        base.position.y = world::height(base.position.x, base.position.z);
        base.yaw = 0.25;
        base.look = Look {
            art: art.into(),
            pieces: pieces.iter().map(|s| s.to_string()).collect(),
            gear: vec![("main_hand".into(), weapon.into())],
            drawn,
            ..Default::default()
        };
        if let Some(s) = shield {
            base.look.gear.push(("off_hand".into(), s.into()));
        }
        if art == "male" {
            base.look.hair = Some("hair.simple_parted".into());
            base.look.brows = Some("brows.strong".into());
            base.look.beard = Some("beard.full".into());
        }
        if art == "elf_female" {
            base.look.hair = Some("hair.long".into());
        }
        if art == "dwarf_male" {
            base.look
                .gear
                .push(("back".into(), "cape.traveller".into()));
            base.look.beard = Some("beard.dwarven".into());
        }
        base.pose = Pose::Idle;
        units.push(base.clone());
    }
    base.id = first + 5;
    base.look = Look {
        art: "wolf.grey".into(),
        ..Default::default()
    };
    base.position = Vec3::new(8., world::height(8., 24.), 24.);
    units.push(base);
    units
}
struct App {
    game: Game,
    renderer: Option<Renderer>,
    window: Option<Window>,
    cursor: (f64, f64),
    error: Option<anyhow::Error>,
    next_redraw: Instant,
    egui_state: Option<egui_winit::State>,
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let result = (|| -> Result<()> {
            // Pick the requested display rather than macOS's current main display. A laptop
            // can remain primary while the performance target is an external ultrawide.
            let monitor = event_loop.available_monitors().min_by_key(|m| {
                let size = m.size();
                size.width.abs_diff(self.game.options.width) as u64
                    + size.height.abs_diff(self.game.options.height) as u64
            });
            let w = event_loop.create_window(
                Window::default_attributes()
                    .with_title("Aldermoor · loading catalogue")
                    .with_fullscreen(
                        self.game
                            .options
                            .fullscreen
                            .then_some(Fullscreen::Borderless(monitor)),
                    )
                    .with_inner_size(PhysicalSize::new(
                        self.game.options.width,
                        self.game.options.height,
                    )),
            )?;
            let size = w.inner_size();
            let mut r = Renderer::with_samples(
                size.width,
                size.height,
                self.game.options.scale,
                self.game.options.samples,
            )?;
            if let RawWindowHandle::AppKit(h) = w.window_handle()?.as_raw() {
                let view = unsafe { &*h.ns_view.as_ptr().cast::<objc2_app_kit::NSView>() };
                r.attach(view);
            } else {
                anyhow::bail!("Aldermoor requires macOS")
            };
            r.set_frames_in_flight(self.game.options.in_flight)?;
            if let Some(path) = &self.game.options.look_settings {
                r.look = render::settings::LookSettings::load(path)?;
            }
            if let Some(path) = &self.game.options.surface_assets {
                r.load_surface_assets(path, &self.game.options.assets)?;
            }
            self.egui_state = Some(egui_winit::State::new(
                self.game.hud_context.clone(),
                egui::ViewportId::ROOT,
                &w,
                Some(w.scale_factor() as f32),
                w.theme(),
                None,
            ));
            w.focus_window();
            self.renderer = Some(r);
            self.window = Some(w);
            Ok(())
        })();
        if let Err(e) = result {
            self.error = Some(e);
            event_loop.exit();
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if let (Some(state), Some(window)) = (&mut self.egui_state, &self.window) {
            let response = state.on_window_event(window, &event);
            if response.consumed
                && matches!(
                    event,
                    WindowEvent::MouseInput { .. }
                        | WindowEvent::MouseWheel { .. }
                        | WindowEvent::KeyboardInput { .. }
                )
            {
                if let WindowEvent::KeyboardInput { event, .. } = &event
                    && event.state == ElementState::Released
                    && let PhysicalKey::Code(key) = event.physical_key
                {
                    self.game.keys.remove(&key);
                }
                return;
            }
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(s) => {
                if let Some(r) = &mut self.renderer
                    && let Err(e) = r.resize(s.width, s.height)
                {
                    self.error = Some(e);
                    event_loop.exit();
                }
            }
            WindowEvent::Focused(false) => {
                self.game.keys.clear();
                self.game.mouse_look = false;
                self.game.controls();
            }
            WindowEvent::CursorMoved { position, .. } => self.cursor = (position.x, position.y),
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 25.,
                };
                self.game.distance = (self.game.distance - y * 0.6).clamp(2., 24.);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Right {
                    self.game.mouse_look = state == ElementState::Pressed;
                }
                if state == ElementState::Pressed
                    && (button == MouseButton::Left || button == MouseButton::Right)
                    && let Some(w) = &self.window
                {
                    let s = w.inner_size();
                    if let Some(id) =
                        self.game
                            .pick(self.cursor.0, self.cursor.1, s.width, s.height)
                    {
                        self.game.host.send(Command::SetTarget(Some(id)));
                        if button == MouseButton::Right {
                            self.game.host.send(Command::SetAutoAttack(true));
                        }
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        self.game.keys.insert(key);
                        if !event.repeat {
                            let command = match key {
                                KeyCode::Escape => {
                                    event_loop.exit();
                                    None
                                }
                                KeyCode::Space => Some(Command::Jump),
                                KeyCode::KeyV => Some(Command::Dodge {
                                    direction: self.game.direction(),
                                }),
                                KeyCode::Tab => Some(Command::SetTarget(self.game.target(true))),
                                KeyCode::Digit1 => Some(Command::UseSlot {
                                    slot: 0,
                                    ground: None,
                                }),
                                KeyCode::Digit2 => Some(Command::UseSlot {
                                    slot: 1,
                                    ground: None,
                                }),
                                KeyCode::KeyR => Some(Command::Respawn),
                                _ => None,
                            };
                            if let Some(c) = command {
                                self.game.host.send(c);
                            }
                        }
                    } else {
                        self.game.keys.remove(&key);
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(r) = &mut self.renderer {
                    if let (Some(state), Some(window)) = (&mut self.egui_state, &self.window) {
                        self.game.hud_input = Some(state.take_egui_input(window));
                    }
                    let result = objc2::rc::autoreleasepool(|_| self.game.draw(r));
                    if let (Some(state), Some(window), Some(output)) = (
                        &mut self.egui_state,
                        &self.window,
                        self.game.platform_output.take(),
                    ) {
                        state.handle_platform_output(window, output);
                    }
                    match result {
                        Ok(true) => event_loop.exit(),
                        Ok(false) => {}
                        Err(e) => {
                            self.error = Some(e);
                            event_loop.exit();
                        }
                    }
                    if self.game.frame.is_multiple_of(30)
                        && let (Some(w), Some(p)) = (
                            &self.window,
                            self.game.snaps.current.unit(self.game.host.player),
                        )
                    {
                        w.set_title(&format!("Aldermoor · HP {:.0}/{:.0} · GPU {:.1} ms · WASD · Space jump · V roll · Tab target · 1/2 attack",p.health,p.max_health,r.stats.gpu_ms));
                    }
                }
            }
            _ => {}
        }
    }
    fn device_event(&mut self, _: &ActiveEventLoop, _: winit::event::DeviceId, event: DeviceEvent) {
        if self.game.mouse_look
            && let DeviceEvent::MouseMotion { delta } = event
        {
            self.game.yaw -= delta.0 as f32 * 0.004;
            self.game.pitch = (self.game.pitch + delta.1 as f32 * 0.003).clamp(-0.1, 1.1);
        }
    }
    fn about_to_wait(&mut self, e: &ActiveEventLoop) {
        let now = Instant::now();
        let period = Duration::from_secs_f64(1. / self.game.options.fps.unwrap_or(60.));
        if now >= self.next_redraw {
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            self.next_redraw += period;
            if self.next_redraw < now {
                self.next_redraw = now + period;
            }
        }
        e.set_control_flow(ControlFlow::WaitUntil(self.next_redraw));
    }
}
fn main() -> Result<()> {
    let options = Options::read()?;
    let offscreen = options.offscreen;
    let mut game = Game::new(options)?;
    if offscreen {
        let mut renderer = Renderer::with_samples(
            game.options.width,
            game.options.height,
            game.options.scale,
            game.options.samples,
        )?;
        renderer.set_frames_in_flight(game.options.in_flight)?;
        if let Some(path) = &game.options.look_settings {
            renderer.look = render::settings::LookSettings::load(path)?;
        }
        if let Some(path) = &game.options.surface_assets {
            renderer.load_surface_assets(path, &game.options.assets)?;
        }
        let mut deadline = Instant::now();
        loop {
            let start = Instant::now();
            if objc2::rc::autoreleasepool(|_| game.draw(&mut renderer))? {
                break;
            }
            if let Some(fps) = game.options.fps {
                let period = Duration::from_secs_f64(1. / fps);
                deadline += period;
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now)
                } else if now - deadline > period {
                    deadline = now;
                }
            } else if game.options.smoke {
                std::thread::sleep(Duration::from_millis(16).saturating_sub(start.elapsed()));
            }
        }
    } else {
        let event_loop = EventLoop::new()?;
        let mut app = App {
            game,
            renderer: None,
            window: None,
            cursor: (0., 0.),
            error: None,
            next_redraw: Instant::now(),
            egui_state: None,
        };
        event_loop.run_app(&mut app)?;
        if let Some(e) = app.error {
            return Err(e);
        }
    }
    Ok(())
}
