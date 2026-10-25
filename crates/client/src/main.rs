//! A windowed Metal client; the simulation remains entirely behind Host and protocol messages.
mod combat;
mod hud;
mod interpolate;
mod perf_overlay;
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
    dpi::{PhysicalPosition, PhysicalSize},
    event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Fullscreen, Window, WindowId},
};

struct Options {
    perf: perf_overlay::Options,
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
    metalfx: bool,
    direct_drawable: bool,
    legacy_cpu: bool,
    look_churn: bool,
    reset_on_look: bool,
    scheme: controls::Scheme,
}
impl Options {
    fn read() -> Result<Self> {
        let mut o = Self {
            perf: perf_overlay::Options::default(),
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
            in_flight: 2,
            fps: None,
            benchmark: false,
            inline_constants: true,
            fullscreen: false,
            look_settings: None,
            surface_assets: None,
            hud_font: None,
            metalfx: false,
            direct_drawable: true,
            legacy_cpu: false,
            look_churn: false,
            reset_on_look: false,
            scheme: controls::Scheme::default(),
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
                        Some("metalfx") => {
                            o.metalfx = true;
                            0
                        }
                        _ => anyhow::bail!("--aa expects taa, metalfx, none or msaa"),
                    }
                }
                "--in-flight" => {
                    o.in_flight = args.next().context("--in-flight needs 1..3")?.parse()?
                }
                "--fps" => o.fps = Some(args.next().context("--fps needs a number")?.parse()?),
                "--legacy-cpu" => o.legacy_cpu = true,
                "--look-churn" => o.look_churn = true,
                "--reset-on-look" => o.reset_on_look = true,
                "--output-blit" => o.direct_drawable = false,
                "--benchmark" => o.benchmark = true,
                "--no-gpu-counters" => o.perf.disabled = true,
                "--gpu-profile-scene" => o.perf.split_scene = true,
                "--counter-overhead" => o.perf.overhead_check = true,
                "--perf-overlay" => o.perf.visible = true,
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
                "--scheme" => {
                    o.scheme = match args.next().as_deref() {
                        Some("classic") => controls::Scheme::Classic,
                        Some("action") => controls::Scheme::Action,
                        _ => anyhow::bail!("--scheme expects classic or action"),
                    }
                }
                "--inspect" => {
                    o.inspect = Some(args.next().context("--inspect needs an art id")?);
                    o.showcase = true;
                }
                "--help" => {
                    println!(
                        "Aldermoor Metal client\nMouse looks and turns you (crosshair) · WASD run and strafe · hold Ctrl for the pointer\nShift walk · Space jump · Alt, V or mouse 4 roll · Tab target · T or Num Lock autorun\nClick target under the crosshair · right-click attack · 1–9/0 skills · F guard · ` swap · G revive · R respawn · Esc quit\n--scheme classic: WoW-style instead (A/D turn, Q/E strafe, left-drag look, right-drag steer, both buttons run)\n\n--size WIDTHxHEIGHT --scale 0.25..1 --assets PATH\n--capture FILE.png --frames N --offscreen --showcase --inspect ART --smoke\n--fullscreen --aa taa|metalfx|none|msaa --fps N --benchmark --warmup N --in-flight 1..3\n--look-settings FILE --surface-assets FILE --hud-font FILE --scheme classic|action\nF3 performance overlay; --perf-overlay starts visible; --no-gpu-counters disables timestamps\n--gpu-profile-scene separates scene timings (diagnostic); --counter-overhead alternates sampling\nLook and surfaces default to the selected asset root’s catalog/look.json and catalog/surfaces.json when present.\nShowcase is a catalogue review scene; smoke sends movement/jump/dodge/combat commands."
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
        o.perf.validate(o.samples)?;
        o.look_settings = catalog_file(&o.assets, "look.json", o.look_settings);
        o.surface_assets = catalog_file(&o.assets, "surfaces.json", o.surface_assets);
        Ok(o)
    }
}
fn catalog_file(root: &std::path::Path, name: &str, explicit: Option<PathBuf>) -> Option<PathBuf> {
    explicit.or_else(|| {
        let path = root.join("catalog").join(name);
        path.is_file().then_some(path)
    })
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
    orbit: controls::Orbit,
    pilot: controls::Pilot,
    keys: HashSet<KeyCode>,
    mouse_look: bool,
    orbiting: bool,
    /// Ctrl held in the Action scheme: the pointer is out for the HUD and the mouse doesn't look.
    pointer_free: bool,
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
    smoke_checks: [bool; 4],
    perf: perf_overlay::Overlay,
    hud_context: egui::Context,
    hud_input: Option<egui::RawInput>,
    platform_output: Option<egui::PlatformOutput>,
    fight: Arc<protocol::CombatView>,
    feedback: combat::Feedback,
    cursor: Vec2,
    viewport: Vec2,
    aim_slot: Option<u8>,
    valley: world::Valley,
}
impl Game {
    fn new(options: Options) -> Result<Self> {
        let catalog = assets::Catalog::load(&options.assets)?;
        let host = server::Host::start(0xa1de, "Traveller");
        let orbit = controls::Orbit {
            shoulder: match options.scheme {
                controls::Scheme::Action => 0.6,
                controls::Scheme::Classic => 0.,
            },
            ..Default::default()
        };
        let pilot = controls::Pilot::new(options.scheme, orbit.yaw);
        let first = host.snapshot().unwrap();
        let smoke_start = first.unit(host.player).unwrap().position;
        let mut g = Self {
            perf: perf_overlay::Overlay::new(&options.perf),
            options,
            host,
            snaps: interpolate::Snapshots::new(first),
            catalog,
            cache: ModelCache::default(),
            characters: HashMap::new(),
            terrain: render::terrain::valley(),
            scenery: None,
            review: Vec::new(),
            orbit,
            pilot,
            keys: HashSet::new(),
            mouse_look: false,
            orbiting: false,
            pointer_free: false,
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
            smoke_checks: [false; 4],
            hud_context: egui::Context::default(),
            hud_input: None,
            platform_output: None,
            fight: Arc::new(protocol::CombatView::default()),
            feedback: combat::Feedback::default(),
            cursor: Vec2::ZERO,
            viewport: Vec2::new(1600., 1000.),
            aim_slot: None,
            valley: world::Valley::new(),
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
    fn held(&self) -> controls::Held {
        let down = |k| self.keys.contains(&k);
        controls::Held {
            forward: down(KeyCode::KeyW),
            back: down(KeyCode::KeyS),
            left: down(KeyCode::KeyA),
            right: down(KeyCode::KeyD),
            strafe_left: down(KeyCode::KeyQ),
            strafe_right: down(KeyCode::KeyE),
            walk: down(KeyCode::ShiftLeft) || down(KeyCode::ShiftRight),
            mouse_look: self.mouse_look,
            orbiting: self.orbiting,
        }
    }
    fn direction(&self) -> Vec2 {
        self.pilot.direction(&self.held(), self.orbit.yaw)
    }
    fn controls(&mut self, dt: f32) {
        let held = self.held();
        let c = self.pilot.steer(&held, &mut self.orbit, dt);
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
        let target = self.orbit.target(p);
        let eye = self.orbit.eye(target, |target, eye| match &self.scenery {
            Some(scenery) => scenery.camera_eye(target, eye, 0.25),
            None => eye,
        });
        Camera {
            eye,
            target,
            fov: controls::camera::FOV_DEGREES.to_radians(),
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
            .filter(|u| u.id != me.id)
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
    /// Aiming at the crosshair in the middle of the screen, rather than at the pointer.
    fn aiming(&self) -> bool {
        self.mouse_look || (self.pilot.scheme == controls::Scheme::Action && !self.pointer_free)
    }
    fn aim_point(&self) -> Option<Vec3> {
        let me = self.snaps.current.unit(self.host.player)?;
        combat::ground_point(
            self.camera(me.position)
                .matrix(self.viewport.x / self.viewport.y),
            if self.aiming() {
                self.viewport * 0.5
            } else {
                self.cursor
            },
            self.viewport,
            &self.valley,
        )
    }
    fn use_slot(&mut self, slot: u8) -> Command {
        self.aim_slot = Some(slot);
        Command::UseSlot {
            slot,
            ground: self.aim_point(),
        }
    }
    fn revive_target(&self) -> Option<u32> {
        let me = self.snaps.current.unit(self.host.player)?;
        let downed: HashSet<_> = self
            .fight
            .units
            .iter()
            .filter(|c| c.downed)
            .map(|c| c.id)
            .collect();
        self.snaps
            .current
            .units
            .iter()
            .filter(|u| {
                u.id != me.id
                    && !u.hostile
                    && (u.health <= 0. || downed.contains(&u.id))
                    && u.position.distance(me.position) <= 4.
            })
            .min_by(|a, b| {
                (Some(a.id) != me.target)
                    .cmp(&(Some(b.id) != me.target))
                    .then_with(|| {
                        a.position
                            .distance_squared(me.position)
                            .total_cmp(&b.position.distance_squared(me.position))
                    })
            })
            .map(|u| u.id)
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
            (9.0, 7),
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
                    slot: 1,
                    ground: None,
                }),
                7 => self.host.send(Command::SwapWeapons),
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
        perf_overlay::Overlay::begin_frame(r, self.frame, &self.options.perf);
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.viewport = Vec2::new(r.width as f32, r.height as f32);
        // Host publishes under separate locks; accept a matching pair, otherwise retain the
        // previous pair for this display frame rather than mix actions from different ticks.
        for _ in 0..3 {
            if let (Some(s), Some(c)) = (self.host.snapshot(), self.host.combat())
                && s.tick == c.tick
            {
                self.snaps.push(s);
                self.fight = c;
                break;
            }
        }
        self.feedback.update(dt);
        if self.options.smoke {
            self.smoke()
        } else {
            self.controls(dt)
        }
        let unit_index: HashMap<_, _> =
            self.snaps.current.units.iter().map(|u| (u.id, u)).collect();
        for e in self.host.take_events() {
            self.events += 1;
            self.feedback.event(&e, &unit_index);
            if let protocol::Event::Rallied { unit } | protocol::Event::Revived { unit } = &e
                && let Some(c) = self.characters.get_mut(unit)
            {
                c.rallied();
            }
            match &e {
                protocol::Event::Jumped { unit } if *unit == self.host.player => {
                    self.smoke_checks[0] = true
                }
                protocol::Event::Dodged { unit } if *unit == self.host.player => {
                    self.smoke_checks[1] = true
                }
                protocol::Event::WeaponsSwapped { unit } if *unit == self.host.player => {
                    self.smoke_checks[3] = true;
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
            self.snaps.units(!self.options.legacy_cpu)
        };
        let time = self.frame as f32 / 60.;
        if self.options.showcase {
            for u in &mut units {
                u.pose_time = time;
            }
        }
        if self.options.look_churn && self.options.showcase {
            units[0].look.gear = if (self.frame / 60).is_multiple_of(2) {
                vec![
                    ("main_hand".into(), "sword.arming".into()),
                    ("off_hand".into(), "shield.round".into()),
                ]
            } else {
                vec![("main_hand".into(), "sword.great".into())]
            };
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
                palette: Arc::from([]),
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
        let live: HashSet<_> = units.iter().map(|u| u.id).collect();
        self.characters.retain(|id, _| {
            if self.options.legacy_cpu {
                units.iter().any(|u| u.id == *id)
            } else {
                live.contains(id)
            }
        });
        let combat_index: HashMap<_, _> = self.fight.units.iter().map(|c| (c.id, c)).collect();
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
                old != look
            });
            if rebuild {
                if self.options.reset_on_look && self.characters.contains_key(&u.id) {
                    r.reset_temporal_history();
                }
                self.characters.insert(
                    u.id,
                    Character::load_lod(&self.catalog, &mut self.cache, &u.look, lod)?,
                );
            }
            let c = self.characters.get_mut(&u.id).unwrap();
            c.partial_updates = !self.options.legacy_cpu;
            c.set_lod(&self.catalog, &mut self.cache, lod)?;
            c.update_combat(
                u,
                if self.options.showcase {
                    None
                } else {
                    combat_index.get(&u.id).copied()
                },
                if self.options.showcase { 1. / 60. } else { dt },
            );
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
        let aim = self.aim_point();
        let radius = self
            .aim_slot
            .and_then(|i| self.snaps.current.player.as_ref()?.bar.get(i as usize))
            .filter(|s| s.ground && s.radius > 0.)
            .map(|s| s.radius);
        let projection = combat::Projection {
            matrix: camera.matrix(r.width as f32 / r.height as f32),
            size: self.viewport
                / raw
                    .viewports
                    .get(&egui::ViewportId::ROOT)
                    .and_then(|v| v.native_pixels_per_point)
                    .unwrap_or(1.),
        };
        let crosshair = self.aiming() && !self.options.showcase;
        let hint = match self.pilot.scheme {
            controls::Scheme::Action => {
                "WASD · Shift walk · Space jump · Alt roll · Hold F guard · G revive · Hold Ctrl for the pointer"
            }
            controls::Scheme::Classic => {
                "W/S · A/D turn · Q/E strafe · Space jump · Alt roll · Hold F guard · G revive · Left-drag look · Right-drag steer"
            }
        };
        let output = ctx.run(raw, |ctx| {
            if !self.options.showcase {
                combat::paint(
                    ctx,
                    &projection,
                    &self.fight,
                    &units,
                    &self.feedback,
                    aim.zip(radius),
                    &self.valley,
                );
            }
            actions = hud::draw(ctx, &self.snaps.current, &self.fight, aim, hint);
            if crosshair {
                paint_crosshair(ctx);
            }
            self.perf
                .draw(ctx, self.host.stats().step_ms, r.gpu_counters_enabled());
        });
        for action in actions {
            match action {
                hud::Action::Command(c) => {
                    if let Command::UseSlot { slot, .. } = c {
                        self.aim_slot = Some(slot);
                    }
                    self.host.send(c);
                }
                hud::Action::AimSlot(slot) => self.aim_slot = Some(slot),
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
        r.direct_drawable = self.options.direct_drawable;
        r.target_fps = self.options.fps.unwrap_or(60.);
        let mut stats = r.draw(self.camera(player.position), &draws, capture)?;
        if self.options.benchmark && self.options.in_flight == 1 {
            r.wait()?;
            stats = r.stats;
        }
        let finished = Instant::now();
        self.perf.collect(
            r,
            (finished - now).as_secs_f64() * 1000.,
            self.options.warmup,
            self.options.benchmark,
        );
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
            self.perf.collect(
                r,
                (finished - now).as_secs_f64() * 1000.,
                self.options.warmup,
                self.options.benchmark,
            );
            if self.options.benchmark {
                self.perf.report(r.gpu_counters_enabled());
            }
            eprintln!("HISTORY resets={}", r.stats.history_resets);
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
                        if self.options.benchmark && self.options.in_flight == 1 {
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
                    "smoke did not exercise simulation: moved={moved}, events={}, jump/dodge/damage/swap={:?}",
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
/// A small ring with four ticks at the centre of the screen, where aimed skills go.
fn paint_crosshair(ctx: &egui::Context) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("crosshair"),
    ));
    let c = ctx.content_rect().center();
    let shadow = egui::Stroke::new(3f32, egui::Color32::from_black_alpha(110));
    let line = egui::Stroke::new(1.5f32, egui::Color32::from_white_alpha(220));
    for stroke in [shadow, line] {
        painter.circle_stroke(c, 3., stroke);
        for d in [
            egui::vec2(1., 0.),
            egui::vec2(-1., 0.),
            egui::vec2(0., 1.),
            egui::vec2(0., -1.),
        ] {
            painter.line_segment([c + d * 7., c + d * 13.], stroke);
        }
    }
}
/// Mouse travel, in pixels, that turns a click into a drag.
const DRAG_THRESHOLD: f64 = 4.;
/// A press of the left or right button, until both are up again.
struct Drag {
    from: (f64, f64),
    travelled: f64,
    hidden: bool,
}
struct App {
    game: Game,
    renderer: Option<Renderer>,
    window: Option<Window>,
    cursor: (f64, f64),
    drag: Option<Drag>,
    /// Whether the pointer is locked and hidden for the Action scheme's mouse-look.
    pointer_held: bool,
    error: Option<anyhow::Error>,
    next_redraw: Instant,
    egui_state: Option<egui_winit::State>,
}
impl App {
    /// Left drags the camera, right steers; a press that barely moves is a click instead,
    /// which targets (and with the right button, attacks) the unit under the cursor.
    fn button(&mut self, button: MouseButton, pressed: bool) {
        match button {
            MouseButton::Left => self.game.orbiting = pressed,
            MouseButton::Right => self.game.mouse_look = pressed,
            // The thumb button rolls, the way the keys point.
            MouseButton::Back if pressed => self.game.host.send(Command::Dodge {
                direction: self.game.direction(),
            }),
            MouseButton::Forward => self.game.host.send(Command::Guard(pressed)),
            _ => return,
        }
        if !matches!(button, MouseButton::Left | MouseButton::Right) {
            return;
        }
        if self.pointer_held {
            // Action: every press is a click on whatever is under the crosshair.
            if pressed && let Some(w) = &self.window {
                let s = w.inner_size();
                let (x, y) = (s.width as f64 / 2., s.height as f64 / 2.);
                if let Some(id) = self.game.pick(x, y, s.width, s.height) {
                    self.game.host.send(Command::SetTarget(Some(id)));
                    if button == MouseButton::Right {
                        self.game.host.send(Command::SetAutoAttack(true));
                    }
                }
            }
            return;
        }
        if pressed {
            self.drag.get_or_insert(Drag {
                from: self.cursor,
                travelled: 0.,
                hidden: false,
            });
            return;
        }
        if self
            .drag
            .as_ref()
            .is_some_and(|d| d.travelled < DRAG_THRESHOLD)
            && let Some(w) = &self.window
        {
            let s = w.inner_size();
            if let Some(id) = self
                .game
                .pick(self.cursor.0, self.cursor.1, s.width, s.height)
            {
                self.game.host.send(Command::SetTarget(Some(id)));
                if button == MouseButton::Right {
                    self.game.host.send(Command::SetAutoAttack(true));
                }
            }
        }
        if !self.game.orbiting && !self.game.mouse_look {
            self.end_drag();
        }
    }
    /// Locks and hides the pointer while the Action scheme is looking with the mouse, and
    /// gives it back when the window loses focus or Ctrl is held.
    fn sync_pointer(&mut self, focused: bool) {
        let want = focused
            && self.game.pilot.scheme == controls::Scheme::Action
            && !self.game.pointer_free
            && !self.game.options.showcase;
        if want == self.pointer_held {
            return;
        }
        self.end_drag();
        if let Some(w) = &self.window {
            if want {
                let _ = w
                    .set_cursor_grab(CursorGrabMode::Locked)
                    .or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined));
            } else {
                let _ = w.set_cursor_grab(CursorGrabMode::None);
            }
            w.set_cursor_visible(!want);
        }
        self.pointer_held = want;
    }
    /// Gives the pointer back where the drag began.
    fn end_drag(&mut self) {
        if let Some(d) = self.drag.take()
            && d.hidden
            && let Some(w) = &self.window
        {
            let _ = w.set_cursor_grab(CursorGrabMode::None);
            w.set_cursor_visible(true);
            let _ = w.set_cursor_position(PhysicalPosition::new(d.from.0, d.from.1));
        }
    }
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
            perf_overlay::Overlay::configure(&mut r, &self.game.options.perf);
            r.enable_metalfx(self.game.options.metalfx)?;
            if let Some(path) = &self.game.options.look_settings {
                r.look = render::settings::LookSettings::load(path)?;
                eprintln!("LOOK {}", path.display());
            }
            if let Some(path) = &self.game.options.surface_assets {
                r.load_surface_assets(path, &self.game.options.assets)?;
                eprintln!("SURFACES {}", path.display());
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
            self.sync_pointer(true);
            Ok(())
        })();
        if let Err(e) = result {
            self.error = Some(e);
            event_loop.exit();
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if self.game.perf.event(&event) {
            return;
        }
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
                    if key == KeyCode::KeyF {
                        self.game.host.send(Command::Guard(false));
                    }
                    if matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight) {
                        self.game.pointer_free = false;
                        self.sync_pointer(true);
                    }
                }
                // A drag that ends over the HUD must still let go.
                if let WindowEvent::MouseInput {
                    state: ElementState::Released,
                    button,
                    ..
                } = event
                {
                    self.button(button, false);
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
                self.game.host.send(Command::Guard(false));
                self.game.mouse_look = false;
                self.game.orbiting = false;
                self.game.pointer_free = false;
                self.end_drag();
                self.sync_pointer(false);
                self.game.controls(0.);
            }
            WindowEvent::Focused(true) => {
                // Grab again: a grab made before the window had focus may not have held.
                self.pointer_held = false;
                self.sync_pointer(true)
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                self.game.cursor = Vec2::new(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 25.,
                };
                self.game.orbit.zoom(y);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.button(button, state == ElementState::Pressed)
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight) {
                        self.game.pointer_free = event.state == ElementState::Pressed;
                        self.sync_pointer(true);
                    }
                    if event.state == ElementState::Pressed {
                        self.game.keys.insert(key);
                        if !event.repeat {
                            let command = match key {
                                KeyCode::Escape => {
                                    event_loop.exit();
                                    None
                                }
                                KeyCode::Space => Some(Command::Jump),
                                KeyCode::KeyV | KeyCode::AltLeft | KeyCode::AltRight => {
                                    Some(Command::Dodge {
                                        direction: self.game.direction(),
                                    })
                                }
                                KeyCode::Tab => Some(Command::SetTarget(self.game.target(true))),
                                key if slot_key(key).is_some() => {
                                    Some(self.game.use_slot(slot_key(key).unwrap()))
                                }
                                KeyCode::KeyF => Some(Command::Guard(true)),
                                KeyCode::Backquote => Some(Command::SwapWeapons),
                                // T for MacBook keyboards, which have no Num Lock.
                                KeyCode::KeyT | KeyCode::NumLock => {
                                    self.game.pilot.toggle_autorun();
                                    None
                                }
                                KeyCode::KeyG => self
                                    .game
                                    .revive_target()
                                    .map(|target| Command::Revive { target }),
                                KeyCode::KeyR => Some(Command::Respawn),
                                _ => None,
                            };
                            if let Some(c) = command {
                                self.game.host.send(c);
                            }
                        }
                    } else {
                        self.game.keys.remove(&key);
                        if key == KeyCode::KeyF {
                            self.game.host.send(Command::Guard(false));
                        }
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
                        w.set_title(&format!("Aldermoor · HP {:.0}/{:.0} · GPU {:.1} ms · mouse aims · Ctrl pointer · WASD · Space jump · Alt roll · Tab target · 1–9 skills",p.health,p.max_health,r.stats.gpu_ms));
                    }
                }
            }
            _ => {}
        }
    }
    fn device_event(&mut self, _: &ActiveEventLoop, _: winit::event::DeviceId, event: DeviceEvent) {
        if (self.game.mouse_look || self.game.orbiting || self.pointer_held)
            && let DeviceEvent::MouseMotion { delta } = event
        {
            self.game.orbit.look(delta.0, delta.1);
            if let Some(d) = &mut self.drag {
                d.travelled += delta.0.hypot(delta.1);
                if !d.hidden
                    && d.travelled >= DRAG_THRESHOLD
                    && let Some(w) = &self.window
                {
                    // Hold the pointer still while dragging, so it can't run off the window.
                    let _ = w
                        .set_cursor_grab(CursorGrabMode::Locked)
                        .or_else(|_| w.set_cursor_grab(CursorGrabMode::Confined));
                    w.set_cursor_visible(false);
                    d.hidden = true;
                }
            }
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
        perf_overlay::Overlay::configure(&mut renderer, &game.options.perf);
        renderer.enable_metalfx(game.options.metalfx)?;
        if let Some(path) = &game.options.look_settings {
            renderer.look = render::settings::LookSettings::load(path)?;
            eprintln!("LOOK {}", path.display());
        }
        if let Some(path) = &game.options.surface_assets {
            renderer.load_surface_assets(path, &game.options.assets)?;
            eprintln!("SURFACES {}", path.display());
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
            drag: None,
            pointer_held: false,
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

fn slot_key(key: KeyCode) -> Option<u8> {
    [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
        KeyCode::Digit0,
    ]
    .iter()
    .position(|k| *k == key)
    .map(|i| i as u8)
}

#[cfg(test)]
mod catalog_defaults_tests {
    use super::*;
    #[test]
    fn optional_catalog_defaults_preserve_explicit_overrides() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        for name in ["look.json", "surfaces.json"] {
            assert_eq!(
                catalog_file(&root, name, None),
                Some(root.join("catalog").join(name))
            );
            let explicit = PathBuf::from("explicit-missing-file.json");
            assert_eq!(
                catalog_file(&root, name, Some(explicit.clone())),
                Some(explicit)
            );
            assert_eq!(catalog_file(&root.join("absent-root"), name, None), None);
        }
    }
}
