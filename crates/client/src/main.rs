//! A windowed Metal client; the simulation remains entirely behind Host and protocol messages.
mod bag;
mod combat;
mod course;
mod craft;
mod crowd;
mod effects;
mod gait;
mod holds;
mod hud;
mod interpolate;
mod latency;
mod menu;
mod pacing;
mod perf_overlay;
mod stage;
use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec2, Vec3};
use protocol::{Command, HitWeight, Look, Pose, UnitKind, UnitState};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use render::{
    Camera, Renderer,
    character::{Character, Draw, Ground, HoldAt},
    model::{Model, ModelCache},
};
use std::{
    collections::{HashMap, HashSet},
    f32::consts::{FRAC_PI_2, PI, TAU},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::{PhysicalPosition, PhysicalSize},
    event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy},
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
    /// Play on a server across the network (`--connect ADDR`), trusting the certificate at
    /// `server_cert`, instead of the built-in one.
    connect: Option<String>,
    /// Log in to this account on the server (#88); the password comes from ALDERMOOR_PASSWORD.
    account: Option<String>,
    new_account: bool,
    server_cert: PathBuf,
    /// The character's name on the server.
    name: String,
    effects_review: bool,
    /// The effects review as a captioned reel of close shots, one effect at a time, from this
    /// many seconds in.
    reel: bool,
    reel_from: f32,
    /// The showcase camera: 0 is the whole scene; 1–3 come in close on parts of the staged
    /// fight (the left of the lineup, its right, the missiles' lanes).
    view: usize,
    /// Every frame after the warmup (or every `sequence_step`th) is captured into this folder
    /// (for review videos and contact sheets).
    sequence: Option<PathBuf>,
    sequence_step: u64,
    /// Plays the scripted run in `gait.rs` instead of taking input, for animation captures.
    gait: bool,
    /// Plays the scripted course in `course.rs` instead of taking input, for traversal captures
    /// (#213).
    traverse: bool,
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
    /// Staged blows (#218): see `Game::hit_churn`.
    hit_churn: bool,
    reset_on_look: bool,
    /// Overrides the settings file's scheme.
    scheme: Option<controls::Scheme>,
    controls: Option<PathBuf>,
    /// How characters blend from one clip to the next (#210). The others are for comparisons.
    blend: anim::Method,
    /// More characters round the player, to measure animation with a crowd in view (#215).
    crowd: usize,
    /// Whether characters' poses rebuild at the rates `anim::Crowd` gives them (#215), or all
    /// every frame, to compare.
    anim_tiers: bool,
    /// Whether characters' feet go on the ground (#214), or stay as the clips have them, to
    /// compare.
    foot_ik: bool,
    /// Whether characters lean with their acceleration (#217), or run upright, to compare.
    lean: bool,
    /// Whether blows tip characters and hitstops hold their poses (#218), or the hit clip plays
    /// in full, to compare.
    flinch: bool,
    /// Whether characters play rolls, leaps, charges and climbs by how far they've come and
    /// swimmers swim (#213), or the clips go by the clock and swimmers run, to compare.
    traversal: bool,
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
            connect: None,
            account: None,
            new_account: false,
            server_cert: PathBuf::from("aldermoor.der"),
            name: "Traveller".into(),
            effects_review: false,
            reel: false,
            reel_from: 0.,
            view: 0,
            sequence: None,
            sequence_step: 1,
            gait: false,
            traverse: false,
            inspect: None,
            warmup: 120,
            samples: 0,
            // One frame in flight: a frame sooner on screen, and on the M1 Air it held the
            // same frame rate as two with a lower GPU p95 (#219).
            in_flight: 1,
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
            hit_churn: false,
            reset_on_look: false,
            scheme: None,
            controls: None,
            blend: anim::Method::default(),
            crowd: 0,
            anim_tiers: true,
            foot_ik: true,
            lean: true,
            flinch: true,
            traversal: true,
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
                "--view" => {
                    o.view = args.next().context("--view needs 0..3")?.parse()?;
                    ensure!(o.view <= 3, "--view needs 0..3");
                }
                "--sequence-step" => {
                    o.sequence_step = args
                        .next()
                        .context("--sequence-step needs a count")?
                        .parse()?;
                    ensure!(o.sequence_step > 0, "--sequence-step needs a count above 0");
                }
                "--sequence" => {
                    o.sequence = Some(PathBuf::from(
                        args.next().context("--sequence needs a folder")?,
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
                "--hit-churn" => o.hit_churn = true,
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
                "--connect" => o.connect = Some(args.next().context("--connect needs HOST:PORT")?),
                "--server-cert" => {
                    o.server_cert = args.next().context("--server-cert needs a file")?.into()
                }
                "--name" => o.name = args.next().context("--name needs a name")?,
                "--account" => o.account = Some(args.next().context("--account needs a name")?),
                "--new-account" => o.new_account = true,
                "--effects-review" => {
                    o.effects_review = true;
                    o.showcase = true;
                }
                "--effects-reel" => {
                    o.effects_review = true;
                    o.reel = true;
                    o.showcase = true;
                }
                "--reel-from" => {
                    o.reel_from = args.next().context("--reel-from needs seconds")?.parse()?
                }
                "--gait" => o.gait = true,
                "--traverse" => o.traverse = true,
                "--scheme" => {
                    o.scheme = Some(match args.next().as_deref() {
                        Some("classic") => controls::Scheme::Classic,
                        Some("action") => controls::Scheme::Action,
                        _ => anyhow::bail!("--scheme expects classic or action"),
                    })
                }
                "--controls" => {
                    o.controls = Some(args.next().context("--controls needs a file")?.into())
                }
                "--blend" => {
                    o.blend = match args.next().as_deref() {
                        Some("inertialize") => anim::Method::Inertialize,
                        Some("dead") => anim::Method::DeadBlend,
                        Some("off") => anim::Method::Off,
                        _ => anyhow::bail!("--blend expects inertialize, dead or off"),
                    }
                }
                "--crowd" => o.crowd = args.next().context("--crowd needs a count")?.parse()?,
                "--anim-tiers" => {
                    o.anim_tiers = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => anyhow::bail!("--anim-tiers expects on or off"),
                    }
                }
                "--foot-ik" => {
                    o.foot_ik = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => anyhow::bail!("--foot-ik expects on or off"),
                    }
                }
                "--lean" => {
                    o.lean = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => anyhow::bail!("--lean expects on or off"),
                    }
                }
                "--flinch" => {
                    o.flinch = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => anyhow::bail!("--flinch expects on or off"),
                    }
                }
                "--traversal" => {
                    o.traversal = match args.next().as_deref() {
                        Some("on") => true,
                        Some("off") => false,
                        _ => anyhow::bail!("--traversal expects on or off"),
                    }
                }
                "--inspect" => {
                    o.inspect = Some(args.next().context("--inspect needs an art id")?);
                    o.showcase = true;
                }
                "--help" => {
                    println!(
                        "Aldermoor Metal client\nMouse looks and turns you (crosshair) · WASD run and strafe · hold Ctrl for the pointer\nShift sprint (out of combat) · / walk · Space jump · Alt, V or mouse 4 roll · Tab target · T or Num Lock autorun\nLeft click attack (hold to keep swinging) · right click guard · 1–9/0 skills · F also guards · ` swap · G revive · R respawn · Esc controls (rebind keys, quit)\n--scheme classic: WoW-style instead (A/D turn, Q/E strafe, left-drag look, right-drag steer, both buttons run)\n\n--size WIDTHxHEIGHT --scale 0.25..1 --assets PATH\n--capture FILE.png --frames N --offscreen --showcase --inspect ART --smoke --effects-review --gait --traverse\n--effects-reel (each effect close up and captioned, from the end of --warmup; --frames for all of it: warmup + 60 × the seconds it prints) --reel-from SECONDS\n--view 0..3 (showcase camera) --sequence DIR (every frame after --warmup) --sequence-step N\n--fullscreen --aa taa|metalfx|none|msaa --fps N --benchmark --warmup N --in-flight 1..3\n--look-settings FILE --surface-assets FILE --hud-font FILE --scheme classic|action --controls FILE.json (default ~/Library/Application Support/Aldermoor/controls.json)\n--connect HOST:PORT --server-cert FILE --name NAME: play on a server over the network (QUIC), trusting its certificate (the server's --cert file, default aldermoor.der)\n--blend inertialize|dead|off: how characters blend between clips (dead blending and cuts are for comparisons)\n--crowd N: N more characters round you, standing, walking, running and fighting, to measure animation\n--anim-tiers on|off: off rebuilds every character's pose every frame, to compare with the rates by importance\n--foot-ik on|off: off leaves characters' feet as the clips have them (sliding, and level on slopes), to compare\n--lean on|off: off keeps characters upright setting off, stopping and turning, to compare\n--flinch on|off: off plays the hit clip in full on every blow, without the hitstop, to compare\n--traversal on|off: off plays rolls, leaps, charges and climbs by the clock and swimmers as runners, and climbers' clips by the clock with their hands and feet as the clips have them, to compare\n--hit-churn: staged blows every 0.75 s, from every side, light, heavy and big in turn: in the showcase the first character swings and the rest take them; with --gait the player takes them on the move\nF3 performance overlay; --perf-overlay starts visible; --no-gpu-counters disables timestamps\n--gpu-profile-scene separates scene timings (diagnostic); --counter-overhead alternates sampling\nLook and surfaces default to the selected asset root’s catalog/look.json and catalog/surfaces.json when present.\nShowcase is a catalogue review scene; effects review stages a fight in it showing every combat effect; smoke sends movement/jump/dodge/combat commands; gait runs, strafes, backpedals, walks and sprints for animation captures; traverse rolls, climbs, swims, hangs, shimmies round corners, runs up a wall, ejects, leaps and lunges over a course for traversal captures."
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
    settings: controls::Settings,
    /// Where the settings load from and the controls menu saves to.
    settings_path: PathBuf,
    /// The controls menu, while it is open.
    menu: Option<menu::Menu>,
    /// Quit was clicked in the menu.
    quit: bool,
    orbit: controls::Orbit,
    /// The eased camera; `view` is where it was last drawn from.
    follow: controls::Follow,
    view: Option<controls::camera::View>,
    pilot: controls::Pilot,
    /// Keys and mouse buttons 3-5 down right now.
    keys: HashSet<controls::Input>,
    mouse_look: bool,
    orbiting: bool,
    /// Ctrl held in the Action scheme: the pointer is out for the HUD and the mouse doesn't look.
    pointer_free: bool,
    /// The Specialties screen is open, and how much quest XP its Give buttons send.
    specialties: bool,
    assign_xp: u32,
    /// The bag is open (it also opens at a shop's counter).
    bag: bool,
    /// The gathering and crafting skills panel is open.
    skills_panel: bool,
    /// The local player's last level up (specialty, level, seconds left on its banner).
    level_up: Option<(String, u8, f32)>,
    /// The left button held in the Action scheme: keep swinging, and when the next press of the
    /// basic attack goes out.
    attacking: Option<f32>,
    /// The right button holds the guard in the Action scheme.
    guard_button: bool,
    last_move: Option<Command>,
    /// The aim pitch last sent.
    last_aim: Option<i8>,
    /// The camera's jolt from blows landing.
    kick: controls::camera::Kick,
    last: Instant,
    start: Instant,
    /// Frame pacing (#223): the client's own clock starts here.
    epoch: Instant,
    frame_clock: pacing::FrameClock,
    sim_clock: pacing::SimClock,
    pacing: pacing::Stats,
    /// The last frame's time on that clock, and the newest step it showed.
    paced: Option<(f64, u64)>,
    /// The last frame's clock reading and the sim times it drew, by this pacing and by
    /// arrival time as before #223, for the judder comparison printed at exit.
    judder: Option<(f64, f64, f64)>,
    /// Press-to-photon times for jumps, rolls and skills (#219).
    latency: latency::Latency,
    /// Seconds per frame when frames come from the display's refresh (capped by `--fps`).
    frame_period: Option<f64>,
    frame: u64,
    gpu: Vec<f64>,
    frame_times: Vec<f64>,
    intervals: Vec<f64>,
    /// Ranks the characters each frame for how often their poses rebuild, inside the animation
    /// budget (#215).
    animation: anim::Crowd,
    finished_at: Instant,
    smoke_stage: usize,
    /// Sim tick the last smoke stage went out on.
    smoke_tick: Option<u64>,
    events: usize,
    smoke_start: Vec3,
    smoke_checks: [bool; 4],
    /// Sim tick the `--gait` run started on, and the leg it is on.
    gait_tick: Option<u64>,
    gait_leg: Option<usize>,
    /// The `--traverse` course, the sim tick it started on, and the step last logged.
    course: course::Course,
    course_tick: Option<u64>,
    course_step: Option<usize>,
    perf: perf_overlay::Overlay,
    hud_context: egui::Context,
    hud_input: Option<egui::RawInput>,
    platform_output: Option<egui::PlatformOutput>,
    fight: Arc<protocol::CombatView>,
    feedback: combat::Feedback,
    /// Messages from the sim ("You can't fight while swimming."), with seconds left on screen.
    messages: Vec<(String, f32)>,
    cursor: Vec2,
    viewport: Vec2,
    aim_slot: Option<u8>,
    /// A ground skill's key held down with Hold to aim: it casts on release.
    held_ground: Option<u8>,
    valley: Arc<world::Valley>,
    /// The valley's ground, which characters put their feet on (#214), and the hold each climber
    /// hangs from, which they put their hands on (#248).
    ground: Ground,
    holds: HoldAt,
    effects: Option<effects::Effects>,
    /// The effects' art, for each renderer to upload.
    effects_art: Option<render::fx::Library>,
    /// The fight staged for `--effects-review`.
    stage: Option<stage::Stage>,
    /// Seconds into the effects reel (it starts as the warmup ends).
    reel_time: f32,
}
impl Game {
    fn new(options: Options) -> Result<Self> {
        let catalog = assets::Catalog::load(&options.assets)?;
        let host = match &options.connect {
            None => server::Host::start(0xa1de, &options.name),
            Some(addr) => {
                let cert = std::fs::read(&options.server_cert).with_context(|| {
                    format!(
                        "the server's certificate {} (--server-cert)",
                        options.server_cert.display()
                    )
                })?;
                let cert = server::net::CertificateDer::from(cert);
                match &options.account {
                    None => server::Host::connect(addr.as_str(), &cert, &options.name),
                    Some(account) => {
                        let password = std::env::var("ALDERMOOR_PASSWORD")
                            .context("set ALDERMOOR_PASSWORD to log in with --account")?;
                        server::Host::login(
                            addr.as_str(),
                            &cert,
                            account,
                            &password,
                            &options.name,
                            options.new_account,
                        )
                    }
                }
                .with_context(|| format!("connecting to {addr}"))?
            }
        };
        let valley = Arc::new(world::Valley::new());
        let ground: Ground = {
            let valley = Arc::clone(&valley);
            Arc::new(move |x, z| valley.ground(x, z))
        };
        let holds: HoldAt = {
            let filed = holds::Holds::new(Arc::clone(&valley));
            Arc::new(move |p| filed.hung_from(p))
        };
        let settings_path = options.controls.clone().unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join("Library/Application Support/Aldermoor/controls.json")
        });
        let mut settings = controls::Settings::load(&settings_path).map_err(anyhow::Error::msg)?;
        if let Some(scheme) = options.scheme {
            settings.scheme = scheme;
        }
        let orbit = settings.orbit();
        let pilot = settings.pilot(orbit.yaw);
        let mut cache = ModelCache::default();
        let (effects, effects_art) = match catalog.effects.clone() {
            Some(table) => {
                let (fx, art) = effects::Effects::load(table, &options.assets, &mut cache)?;
                (Some(fx), Some(art))
            }
            None => (None, None),
        };
        let first = host.snapshot().unwrap();
        let smoke_start = first.unit(host.player).unwrap().position;
        let mut g = Self {
            perf: perf_overlay::Overlay::new(&options.perf),
            options,
            host,
            snaps: interpolate::Snapshots::new(first),
            catalog,
            cache,
            characters: HashMap::new(),
            terrain: render::terrain::valley(),
            scenery: None,
            review: Vec::new(),
            settings,
            settings_path,
            menu: None,
            quit: false,
            orbit,
            follow: controls::Follow::default(),
            view: None,
            pilot,
            keys: HashSet::new(),
            mouse_look: false,
            orbiting: false,
            pointer_free: false,
            specialties: false,
            assign_xp: 0,
            bag: false,
            skills_panel: false,
            level_up: None,
            attacking: None,
            guard_button: false,
            last_move: None,
            last_aim: None,
            kick: controls::camera::Kick::default(),
            last: Instant::now(),
            start: Instant::now(),
            epoch: Instant::now(),
            frame_clock: pacing::FrameClock::default(),
            sim_clock: pacing::SimClock::default(),
            pacing: pacing::Stats::default(),
            paced: None,
            judder: None,
            latency: latency::Latency::default(),
            frame_period: None,
            frame: 0,
            gpu: Vec::new(),
            frame_times: Vec::new(),
            intervals: Vec::new(),
            animation: anim::Crowd::default(),
            finished_at: Instant::now(),
            smoke_stage: 0,
            smoke_tick: None,
            events: 0,
            smoke_start,
            smoke_checks: [false; 4],
            gait_tick: None,
            gait_leg: None,
            course: course::Course::default(),
            course_tick: None,
            course_step: None,
            hud_context: egui::Context::default(),
            hud_input: None,
            platform_output: None,
            fight: Arc::new(protocol::CombatView::default()),
            feedback: combat::Feedback::default(),
            messages: Vec::new(),
            cursor: Vec2::ZERO,
            viewport: Vec2::new(1600., 1000.),
            aim_slot: None,
            held_ground: None,
            valley,
            ground,
            holds,
            effects,
            effects_art,
            stage: None,
            reel_time: 0.,
        };
        g.scenery = Some(render::scenery::Scenery::new(&g.catalog, &mut g.cache)?);
        if g.options.reel {
            eprintln!(
                "effects reel: {:.1} s ({} frames after the warmup)",
                stage::reel_seconds(),
                (stage::reel_seconds() * 60.).ceil()
            );
            g.stage = Some(stage::Stage::reel());
        } else if g.options.effects_review {
            g.stage = Some(stage::Stage::default());
        }
        if g.options.showcase {
            g.review = showcase(
                g.snaps.current.unit(g.host.player).unwrap().clone(),
                &g.valley,
            );
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
            let mut c = Character::load(&g.catalog, &mut g.cache, &u.look)?;
            c.set_ground(Arc::clone(&g.ground));
            c.set_holds(Arc::clone(&g.holds));
            g.characters.insert(u.id, c);
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
    /// Whether any input bound to `control` is down.
    fn down(&self, control: controls::Control) -> bool {
        self.settings
            .bindings
            .held(control, |i| self.keys.contains(i))
    }
    fn held(&self) -> controls::Held {
        use controls::Control as C;
        controls::Held {
            forward: self.down(C::Forward),
            back: self.down(C::Back),
            left: self.down(C::Left),
            right: self.down(C::Right),
            strafe_left: self.down(C::StrafeLeft),
            strafe_right: self.down(C::StrafeRight),
            sprint: self.down(C::Sprint),
            mouse_look: self.mouse_look,
            orbiting: self.orbiting,
        }
    }
    /// The ability on bar slot `slot` now, if any.
    fn slot_ability(&self, slot: u8) -> Option<String> {
        let s = self.snaps.current.player.as_ref()?.bar.get(slot as usize)?;
        (!s.ability.is_empty()).then(|| s.ability.clone())
    }
    /// Does what a bound control does when its key goes down.
    fn press(&mut self, control: controls::Control) {
        use controls::Control as C;
        let pressed = match control {
            C::Jump => Some((latency::Kind::Jump, None)),
            C::Roll => Some((latency::Kind::Roll, None)),
            c => c
                .slot()
                .map(|slot| (latency::Kind::Skill, self.slot_ability(slot))),
        };
        if let Some((kind, ability)) = pressed {
            self.latency.pressed(kind, ability, latency::now());
        }
        let command = match control {
            C::Jump => Some(Command::Jump),
            C::Roll => Some(Command::Dodge {
                direction: self.direction(),
            }),
            C::Guard => Some(Command::Guard(true)),
            C::Autorun => {
                self.pilot.toggle_autorun();
                None
            }
            C::Walk => {
                self.pilot.toggle_walk();
                None
            }
            C::FreePointer => {
                self.pointer_free = true;
                None
            }
            C::Target => Some(Command::SetTarget(self.target(true))),
            C::SwapWeapons => Some(Command::SwapWeapons),
            C::Revive => self
                .revive_target()
                .map(|target| Command::Revive { target }),
            C::Respawn => Some(Command::Respawn),
            C::Specialties => {
                self.specialties = !self.specialties;
                None
            }
            C::Bag => {
                self.bag = !self.bag;
                None
            }
            C::Skills => {
                self.skills_panel = !self.skills_panel;
                None
            }
            C::Interact => {
                let snapshot = &self.snaps.current;
                let player = snapshot.player.as_ref();
                if player
                    .and_then(|p| p.work.as_ref())
                    .is_some_and(|w| w.sweet_to > w.sweet_from)
                {
                    Some(Command::Strike)
                } else {
                    snapshot
                        .unit(self.host.player)
                        .zip(player)
                        .and_then(|(me, p)| craft::interact(snapshot, p, me.position))
                        .map(|i| i.command())
                }
            }
            C::Loot => self
                .snaps
                .current
                .unit(self.host.player)
                .zip(self.snaps.current.player.as_ref())
                .and_then(|(me, player)| bag::nearest_pile(player, me.position))
                .map(|pile| Command::Loot { pile: pile.id }),
            c => c.slot().and_then(|slot| {
                if self.settings.ground_cast == controls::GroundCast::HoldToAim
                    && self.ground_slot(slot)
                {
                    self.held_ground = Some(slot);
                    self.aim_slot = Some(slot);
                    None
                } else {
                    Some(self.use_slot(slot))
                }
            }),
        };
        if let Some(c) = command {
            self.host.send(c);
        }
    }
    /// Lets go of a held control once none of its inputs is down any more.
    fn release(&mut self, control: controls::Control) {
        match control {
            controls::Control::Guard if !self.down(control) && !self.guard_button => {
                self.host.send(Command::Guard(false))
            }
            controls::Control::FreePointer => self.pointer_free = self.down(control),
            c if c.slot().is_some() && c.slot() == self.held_ground && !self.down(c) => {
                self.held_ground = None;
                let cast = self.use_slot(c.slot().unwrap());
                self.host.send(cast);
            }
            _ => {}
        }
    }
    /// The crosshair's state, from your target or the enemy under the crosshair and the reach
    /// of the basic attack (the bar's auto-attack slot, else its first).
    fn reticle(&self) -> Reticle {
        let snapshot = &self.snaps.current;
        let Some(me) = snapshot.unit(self.host.player) else {
            return Reticle::Free;
        };
        let reach = snapshot.player.as_ref().map_or(0., |p| {
            p.bar
                .iter()
                .find(|s| s.auto)
                .or(p.bar.first())
                .map_or(0., |s| s.range)
        });
        let enemy = |id| {
            snapshot
                .unit(id)
                .filter(|u| u.hostile && u.health > 0.)
                .map(|u| u.position.distance(me.position))
        };
        let target = me.target.and_then(enemy);
        let under = if target.is_none() {
            let (w, h) = (self.viewport.x as u32, self.viewport.y as u32);
            self.pick(w as f64 / 2., h as f64 / 2., w, h)
                .and_then(enemy)
        } else {
            None
        };
        reticle(target, under, reach)
    }
    /// Is this bar slot aimed at the ground?
    fn ground_slot(&self, slot: u8) -> bool {
        self.snaps
            .current
            .player
            .as_ref()
            .and_then(|p| p.bar.get(slot as usize))
            .is_some_and(|s| s.ground)
    }
    /// Lets go of everything, for losing focus or opening the menu.
    fn release_all(&mut self) {
        self.keys.clear();
        self.host.send(Command::Guard(false));
        self.mouse_look = false;
        self.orbiting = false;
        self.pointer_free = false;
        self.attacking = None;
        self.held_ground = None;
        self.guard_button = false;
        self.controls(0.);
    }
    /// The Action scheme's left button: swing while it's held (the basic attack, toward a
    /// target or wherever you aim), and stop when it's let go.
    fn attack_button(&mut self, pressed: bool) {
        if pressed {
            self.attacking = Some(0.);
            self.keep_swinging(0.);
        } else if self.attacking.take().is_some() {
            self.host.send(Command::SetAutoAttack(false));
        }
    }
    /// Presses the basic attack again every tenth of a second while the button is held, so it
    /// swings at nothing in particular too. Presses mid-swing are ignored by the sim.
    fn keep_swinging(&mut self, dt: f32) {
        let Some(wait) = self.attacking.as_mut() else {
            return;
        };
        *wait -= dt;
        if *wait > 0. {
            return;
        }
        *wait = SWING_REPEAT;
        let basic = self
            .snaps
            .current
            .player
            .as_ref()
            .and_then(|p| p.bar.iter().position(|s| s.auto));
        if let Some(slot) = basic {
            let c = self.use_slot(slot as u8);
            self.host.send(c);
        }
    }
    /// The Action scheme's right button holds the guard, like the guard key.
    fn guard_button(&mut self, pressed: bool) {
        self.guard_button = pressed;
        if pressed {
            self.host.send(Command::Guard(true));
        } else if !self.down(controls::Control::Guard) {
            self.host.send(Command::Guard(false));
        }
    }
    fn open_menu(&mut self) {
        self.release_all();
        self.menu = Some(menu::Menu::default());
    }
    fn change(&mut self, change: menu::Change) {
        let Some(m) = &mut self.menu else {
            return;
        };
        match change {
            menu::Change::Capture(c) => {
                m.capture = Some(c);
                m.notice = Some(format!(
                    "Press a key or mouse button for {}. Esc cancels.",
                    c.name()
                ));
                return;
            }
            menu::Change::Unbind(c, input) => {
                self.settings.bindings.unbind(c, &input);
                m.notice = Some(format!("{} no longer does {}.", input.label(), c.name()));
            }
            menu::Change::Reset => {
                self.settings.bindings.reset();
                m.notice = Some("Keys are back to the defaults.".into());
            }
            menu::Change::Scheme(scheme) => {
                self.settings.scheme = scheme;
                self.pilot.scheme = scheme;
                self.orbit.shoulder = self.settings.shoulder();
                m.notice = None;
            }
            menu::Change::GroundCast(cast) => {
                self.settings.ground_cast = cast;
                self.held_ground = None;
                m.notice = None;
            }
            menu::Change::ScreenShake(shake) => {
                self.settings.screen_shake = shake;
                m.notice = None;
            }
            menu::Change::Close => {
                self.menu = None;
                return;
            }
            menu::Change::Quit => {
                self.quit = true;
                return;
            }
        }
        self.save_settings();
    }
    /// Binds the key or button pressed while the menu was waiting for one.
    fn bind_captured(&mut self, input: controls::Input) {
        let Some(m) = &mut self.menu else {
            return;
        };
        if let Some(why) = reserved(&input) {
            // Keep waiting for a key the game will actually see.
            m.notice = Some(why);
            return;
        }
        let Some(control) = m.capture.take() else {
            return;
        };
        let taken = self.settings.bindings.bind(control, input.clone());
        m.notice = Some(if taken.is_empty() {
            format!("{} now does {}.", input.label(), control.name())
        } else {
            let names: Vec<_> = taken.iter().map(|c| c.name()).collect();
            format!(
                "{} now does {}, and no longer {}.",
                input.label(),
                control.name(),
                names.join(" or ")
            )
        });
        self.save_settings();
    }
    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save(&self.settings_path)
            && let Some(m) = &mut self.menu
        {
            m.notice = Some(format!("Couldn't save: {e}"));
        }
    }
    /// The keys as the HUD shows them.
    fn key_labels(&self) -> hud::Keys {
        use controls::Control as C;
        let l = |c| self.settings.bindings.label(c);
        let hint = match self.pilot.scheme {
            controls::Scheme::Action => format!(
                "{}{}{}{} · {} sprint · {} jump · {} roll · Hold {} guard · {} revive · Hold {} for the pointer · Esc controls",
                l(C::Forward),
                l(C::Left),
                l(C::Back),
                l(C::Right),
                l(C::Sprint),
                l(C::Jump),
                l(C::Roll),
                l(C::Guard),
                l(C::Revive),
                l(C::FreePointer),
            ),
            controls::Scheme::Classic => format!(
                "{}/{} · {}/{} turn · {}/{} strafe · {} jump · {} roll · Hold {} guard · {} revive · Left-drag look · Right-drag steer · Esc controls",
                l(C::Forward),
                l(C::Back),
                l(C::Left),
                l(C::Right),
                l(C::StrafeLeft),
                l(C::StrafeRight),
                l(C::Jump),
                l(C::Roll),
                l(C::Guard),
                l(C::Revive),
            ),
        };
        hud::Keys {
            hint,
            slots: std::array::from_fn(|i| {
                C::ALL
                    .into_iter()
                    .find(|c| c.slot() == Some(i as u8))
                    .map_or_else(String::new, l)
            }),
            revive: l(C::Revive),
            target: l(C::Target),
            swap: l(C::SwapWeapons),
            respawn: l(C::Respawn),
            specialties: l(C::Specialties),
            loot: l(C::Loot),
            interact: l(C::Interact),
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
        // The crosshair's pitch, for others to see where this player aims (#244).
        if let Some(v) = self.view {
            let d = v.target - v.eye;
            let pitch = protocol::aim_pitch_byte(d.y.atan2(Vec2::new(d.x, d.z).length()));
            if self.last_aim != Some(pitch) {
                self.host.send(Command::Aim { pitch });
                self.last_aim = Some(pitch);
            }
        }
        self.keep_swinging(dt);
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
            // Target (x, z, height over the ground), and the eye's offset from it.
            let (x, z, up, from) = match self.options.view {
                _ if self.options.reel => {
                    let (x, z, up, from) = stage::shot(self.reel_time).camera;
                    (x, z, up, Vec3::from(from))
                }
                1 => (-1.5, 25.7, 1.0, Vec3::new(3.2, 1.7, 6.8)),
                2 => (5.6, 25.1, 1.0, Vec3::new(2.6, 1.8, 6.6)),
                3 => (0.5, 26.5, 1.5, Vec3::new(1.6, 0.9, 7.5)),
                _ => (2., LINEUP_Z, 1.25, Vec3::new(8., 3.5, 14.)),
            };
            let target = Vec3::new(x, world::height(x, z) + up, z);
            return Camera {
                eye: target + from,
                target,
                fov: 45f32.to_radians(),
            };
        }
        let view = self.view.unwrap_or_else(|| {
            let target = self.orbit.target(p);
            controls::camera::View {
                target,
                eye: self
                    .orbit
                    .eye(target, |target, eye| self.unblocked(target, eye)),
            }
        });
        Camera {
            eye: view.eye,
            target: view.target,
            fov: self.settings.fov_degrees.to_radians(),
        }
    }
    /// The eye pulled in front of the scenery between it and `target`.
    fn unblocked(&self, target: Vec3, eye: Vec3) -> Vec3 {
        match &self.scenery {
            Some(scenery) => scenery.camera_eye(target, eye, 0.25),
            None => eye,
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
    /// A screen that wants the pointer is open: Specialties, the bag, or a shop's counter.
    fn screen_open(&self) -> bool {
        self.specialties
            || self.bag
            || self.skills_panel
            || self
                .snaps
                .current
                .player
                .as_ref()
                .is_some_and(|p| p.shop.is_some() || p.station.is_some())
    }
    /// Aiming at the crosshair in the middle of the screen, rather than at the pointer.
    fn aiming(&self) -> bool {
        self.menu.is_none()
            && (self.mouse_look
                || (self.pilot.scheme == controls::Scheme::Action
                    && !self.pointer_free
                    && !self.screen_open()))
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
    /// Seconds of sim time since the last smoke stage went out.
    fn smoke_wait(&self) -> f32 {
        let tick = self.snaps.current.tick;
        self.smoke_tick
            .map_or(f32::INFINITY, |t| tick.saturating_sub(t) as f32 / 60.)
    }
    fn smoke(&mut self) {
        // Seconds of sim time each stage waits after the one before. Waiting from the previous
        // stage, one stage a frame, means a slow frame (the first can take seconds under GPU
        // load) delays the rest instead of sending several stages on the same tick.
        let schedule = [0., 0.8, 1.2, 1.0, 3.0, 1.0, 1.0, 1.0];
        if self.smoke_stage < schedule.len() && self.smoke_wait() >= schedule[self.smoke_stage] {
            self.smoke_tick = Some(self.snaps.current.tick);
            match self.smoke_stage {
                0 => self.host.send(Command::Move {
                    direction: Vec2::new(-1., 0.),
                    walk: false,
                    sprint: false,
                    face_yaw: None,
                }),
                1 => {
                    self.latency
                        .pressed(latency::Kind::Jump, None, latency::now());
                    self.host.send(Command::Jump)
                }
                2 => {
                    self.latency
                        .pressed(latency::Kind::Roll, None, latency::now());
                    self.host.send(Command::Dodge {
                        direction: Vec2::new(-1., 0.),
                    })
                }
                3 => {
                    self.host.send(Command::SetTarget(self.target(false)));
                    self.host.send(Command::SetAutoAttack(true));
                }
                4 | 5 => {
                    let ability = self.slot_ability(1);
                    self.latency
                        .pressed(latency::Kind::Skill, ability, latency::now());
                    self.host.send(Command::UseSlot {
                        slot: 1,
                        ground: None,
                    })
                }
                7 => self.host.send(Command::SwapWeapons),
                _ => self.host.send(Command::Move {
                    direction: Vec2::ZERO,
                    walk: false,
                    sprint: false,
                    face_yaw: None,
                }),
            }
            self.smoke_stage += 1;
        }
        // The sim drops a jump on a tick the player isn't grounded and a swap it refuses, so keep
        // asking until the event arrives (jump until the dodge, swap until the run ends).
        if (2..=3).contains(&self.smoke_stage) && !self.smoke_checks[0] {
            self.host.send(Command::Jump);
        }
        if self.smoke_stage == schedule.len() && !self.smoke_checks[3] {
            self.host.send(Command::SwapWeapons);
        }
    }
    /// Seconds of sim time since the `--gait` run started.
    fn gait_time(&self) -> f32 {
        let tick = self.snaps.current.tick;
        self.gait_tick
            .map_or(0., |t| tick.saturating_sub(t) as f32 / 60.)
    }
    /// Sends the `--gait` run's move for this frame, and logs each leg as it starts so captures
    /// can be cut by frame.
    fn gait(&mut self) {
        self.gait_tick.get_or_insert(self.snaps.current.tick);
        let Some((leg, command)) = gait::at(self.gait_time()) else {
            return;
        };
        if self.gait_leg != Some(leg) {
            self.gait_leg = Some(leg);
            eprintln!("gait: {} from frame {}", gait::name(leg), self.frame);
            if gait::draws(leg) {
                self.host.send(Command::SetTarget(self.target(false)));
            }
            if gait::jumps(leg) {
                self.host.send(Command::Jump);
            }
        }
        if self.last_move.as_ref() != Some(&command) {
            self.host.send(command.clone());
            self.last_move = Some(command);
        }
    }
    /// Sends the `--traverse` course's commands for this frame (#213), and logs each step as it
    /// starts so captures can be cut by frame.
    fn traverse(&mut self) {
        let start = *self.course_tick.get_or_insert(self.snaps.current.tick);
        let now = self.snaps.current.tick.saturating_sub(start) as f32 / 60.;
        let Some(me) = self.snaps.current.unit(self.host.player) else {
            return;
        };
        let foes: Vec<_> = (self.snaps.current.units.iter())
            .filter(|u| u.hostile && u.health > 0.)
            .map(|u| (u.id, u.position))
            .collect();
        let above = me.position.y - self.valley.ground(me.position.x, me.position.z);
        let commands = self.course.update(now, me.position, above, &foes);
        if let Some((step, name)) = self.course.step()
            && self.course_step != Some(step)
        {
            self.course_step = Some(step);
            eprintln!("traverse: {name} from frame {}", self.frame);
        }
        for c in commands {
            if let Command::Move { .. } = c {
                if self.last_move.as_ref() == Some(&c) {
                    continue;
                }
                self.last_move = Some(c.clone());
            }
            self.host.send(c);
        }
    }
    /// `--hit-churn` (#218): staged blows, every 0.75 s, each held as `Pose::Hit` for a quarter of
    /// a second, as the sim holds them. In the showcase the first character swings, and its blow
    /// lands a quarter of a second in, holding the swing for the hitstop; as it lands, each of the
    /// others takes one, from the front, behind or a side and light, heavy or big in turn. Out of
    /// the showcase (with `--gait`, say), the player takes them on the move.
    fn hit_churn(&mut self, units: &mut [UnitState]) {
        // In frames, which are 1/60 s in the showcase: a round, the swing in it, when the blow
        // lands and when those it lands on recover.
        const ROUND: u64 = 45;
        const SWING: u64 = 30;
        const LANDS: u64 = 15;
        const RECOVERS: u64 = 30;
        // `content::combat::HITSTOP`'s, for blows struck in reach.
        const BLOWS: [(HitWeight, f32); 3] = [
            (HitWeight::Light, 0.04),
            (HitWeight::Heavy, 0.08),
            (HitWeight::Big, 0.12),
        ];
        // The way a blow travels, from the body's facing: from the front, from behind, and from
        // each side.
        const FROM: [f32; 4] = [PI, 0., -FRAC_PI_2, FRAC_PI_2];
        let (round, f) = ((self.frame / ROUND) as usize, self.frame % ROUND);
        let seconds = |frames: u64| frames as f32 / 60.;
        if !self.options.showcase {
            let Some(u) = units.iter_mut().find(|u| u.id == self.host.player) else {
                return;
            };
            if (LANDS..RECOVERS).contains(&f) {
                (u.pose, u.pose_time) = (Pose::Hit, seconds(f - LANDS));
            }
            if f == LANDS
                && let Some(c) = self.characters.get_mut(&u.id)
            {
                let (weight, hitstop) = BLOWS[round % BLOWS.len()];
                c.hit(u.yaw + FROM[round % FROM.len()], weight, hitstop);
            }
            return;
        }
        for (i, u) in units.iter_mut().enumerate() {
            (u.pose, u.pose_time) = match (i, f) {
                (0, ..SWING) => (Pose::Attack, seconds(f)),
                (0, _) => (Pose::Idle, seconds(f - SWING)),
                (_, ..LANDS) => (Pose::Idle, seconds(f + ROUND - RECOVERS)),
                (_, ..RECOVERS) => (Pose::Hit, seconds(f - LANDS)),
                _ => (Pose::Idle, seconds(f - RECOVERS)),
            };
        }
        if let Some(first) = units.first_mut()
            && !first.look.drawn
        {
            Arc::make_mut(&mut first.look).drawn = true;
        }
        if f != LANDS {
            return;
        }
        for (i, u) in units.iter().enumerate() {
            let Some(c) = self.characters.get_mut(&u.id) else {
                continue;
            };
            let (weight, hitstop) = BLOWS[(round + i) % BLOWS.len()];
            if i == 0 {
                c.landed(hitstop);
            } else {
                c.hit(u.yaw + FROM[(round + i) % FROM.len()], weight, hitstop);
            }
        }
    }
    /// MAC CHECK PATCH (uncommitted): ALDER_SCRIPT="t:cmd a b [yaw];..." in seconds of sim time
    /// from the first drawn frame. cmd: run|walk|sprint dx dz [face_yaw], jump, stop [face_yaw].
    fn mac_script(&mut self) {
        use std::sync::Mutex;
        static STATE: Mutex<(Option<f64>, usize)> = Mutex::new((None, 0));
        let script = std::env::var("ALDER_SCRIPT").unwrap();
        let steps: Vec<(f64, Vec<String>)> = script
            .split(';')
            .filter(|s| !s.trim().is_empty())
            .map(|s| {
                let (t, c) = s.split_once(':').unwrap();
                (t.trim().parse().unwrap(), c.split_whitespace().map(String::from).collect())
            })
            .collect();
        let now = self.snaps.current.time;
        // Aim where the crosshair points, as the controls do (#212, #244).
        self.mouse_look = true;
        if let Some(view) = self.view {
            let d = view.target - view.eye;
            let pitch = protocol::aim_pitch_byte(d.y.atan2(Vec2::new(d.x, d.z).length()));
            if self.last_aim != Some(pitch) {
                self.host.send(Command::Aim { pitch });
                self.last_aim = Some(pitch);
            }
        }
        let mut st = STATE.lock().unwrap();
        let start = *st.0.get_or_insert(now);
        while st.1 < steps.len() && now - start >= steps[st.1].0 {
            let c = &steps[st.1].1;
            let v = |i: usize| c.get(i).map_or(0., |s: &String| s.parse::<f32>().unwrap());
            let yaw = |i: usize| c.get(i).map(|s: &String| s.parse::<f32>().unwrap().to_radians());
            eprintln!("script t={:.2} frame={} {:?}", now - start, self.frame, c);
            match c[0].as_str() {
                "cam" | "camto" => {
                    let cur = mac_cam(now).unwrap_or([self.orbit.yaw, self.orbit.pitch, self.orbit.distance]);
                    let secs = if c[0] == "camto" { v(4) as f64 } else { 0. };
                    *MAC_CAM.lock().unwrap() = Some((cur, [v(1), v(2), v(3)], now, secs));
                }
                "target" => {
                    let t = self.target(false);
                    self.host.send(Command::SetTarget(t));
                }
                "untarget" => self.host.send(Command::SetTarget(None)),
                "slot" => {
                    let cmd = self.use_slot(v(1) as u8);
                    self.host.send(cmd);
                }
                "swap" => self.host.send(Command::SwapWeapons),
                "jump" => self.host.send(Command::Jump),
                "dodge" => self.host.send(Command::Dodge { direction: Vec2::new(v(1), v(2)) }),
                "guard" => self.host.send(Command::Guard(v(1) > 0.5)),
                "auto" => self.host.send(Command::SetAutoAttack(v(1) > 0.5)),
                "stop" => self.host.send(Command::Move { direction: Vec2::ZERO, walk: false, sprint: false, face_yaw: yaw(1) }),
                m => self.host.send(Command::Move {
                    direction: Vec2::new(v(1), v(2)).normalize_or_zero(),
                    walk: m == "walk",
                    sprint: m == "sprint",
                    face_yaw: yaw(3),
                }),
            }
            st.1 += 1;
        }
    }
    fn draw(&mut self, r: &mut Renderer) -> Result<bool> {
        perf_overlay::Overlay::begin_frame(r, self.frame, &self.options.perf);
        self.latency.presented(|frame| r.presented_at(frame));
        let now = Instant::now();
        let mut dt = (now - self.last).as_secs_f32().min(0.1);
        if std::env::var("ALDER_LOCKSTEP").is_ok() {
            // MAC CHECK PATCH: each frame is exactly 1/30 s of sim time.
            use std::sync::atomic::Ordering;
            let b = &server::MAC_LOCKSTEP_BUDGET;
            if b.load(Ordering::Acquire) < 0 {
                b.store(0, Ordering::Release);
            }
            let target = self.snaps.current.tick + 2;
            b.fetch_add(2, Ordering::AcqRel);
            let t = Instant::now();
            while self.host.snapshot().is_none_or(|s| s.tick < target) && t.elapsed().as_secs() < 3 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            dt = 1. / 30.;
        }
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
        // Draw the world at a steady sim time on evenly spaced frame times.
        let local = (now - self.epoch).as_secs_f64();
        let frame_time = self.frame_clock.tick(local, self.frame_period);
        if let Some((sim_time, due)) = self.host.clock() {
            self.sim_clock
                .observe(sim_time, seconds_between(self.epoch, due));
        }
        self.snaps.draw_time = self.sim_clock.draw_time(frame_time);
        let tick = self.snaps.current.tick;
        if let Some((last_time, last_tick)) = self.paced {
            self.pacing
                .frame(frame_time - last_time, tick.saturating_sub(last_tick));
        }
        self.paced = Some((frame_time, tick));
        if let Some(drawn) = self.snaps.draw_time {
            let s = &self.snaps;
            let gap = (s.current.time - s.previous.time).max(1. / 60.);
            let by_arrival = s.previous.time + s.received.elapsed().as_secs_f64().min(gap + 0.1);
            // Loading stalls aren't judder.
            if let Some((last_local, last_drawn, last_arrival)) = self.judder
                && local - last_local < 0.1
            {
                // What the display shows between these frames: whole refreshes.
                let period = self.frame_period.unwrap_or(1. / 60.);
                let shown = ((local - last_local) / period).round() * period;
                self.pacing.judder(
                    drawn - last_drawn - shown,
                    by_arrival - last_arrival - shown,
                );
            }
            self.judder = Some((local, drawn, by_arrival));
        }
        self.feedback.update(dt);
        for m in &mut self.messages {
            m.1 -= dt;
        }
        self.messages.retain(|m| m.1 > 0.);
        if let Some(b) = &mut self.level_up {
            b.2 -= dt;
        }
        self.level_up.take_if(|b| b.2 <= 0.);
        if self.options.smoke {
            self.smoke()
        } else if std::env::var("ALDER_SCRIPT").is_ok() {
            self.mac_script()
        } else if self.options.gait {
            self.gait()
        } else if self.options.traverse {
            self.traverse()
        } else {
            self.controls(dt)
        }
        let unit_index: HashMap<_, _> =
            self.snaps.current.units.iter().map(|u| (u.id, u)).collect();
        let event_time = self.snaps.current.time as f32;
        for e in self.host.take_events() {
            self.events += 1;
            self.feedback.event(&e, &unit_index);
            let me = self.host.player;
            let shown = match &e {
                protocol::Event::Jumped { unit } if *unit == me => {
                    Some((latency::Kind::Jump, None))
                }
                protocol::Event::Dodged { unit } if *unit == me => {
                    Some((latency::Kind::Roll, None))
                }
                protocol::Event::AbilityUsed { unit, ability, .. }
                | protocol::Event::CastStarted { unit, ability, .. }
                    if *unit == me =>
                {
                    Some((latency::Kind::Skill, Some(ability.as_str())))
                }
                _ => None,
            };
            if let Some((kind, ability)) = shown {
                self.latency.stepped(kind, ability, self.snaps.current.time);
            }
            if let protocol::Event::Hit {
                attacker,
                victim,
                dir,
                weight,
                ..
            } = e
                && let Some(metres) = kick_for(
                    weight,
                    attacker == self.host.player,
                    victim == self.host.player,
                )
            {
                let yaw = dir as f32 / 256. * std::f32::consts::TAU;
                self.kick.add(
                    Vec2::new(yaw.sin(), yaw.cos()),
                    metres,
                    self.settings.screen_shake,
                );
            }
            // A staged fight makes its own events.
            if let Some(fx) = &mut self.effects
                && self.stage.is_none()
            {
                fx.event(&e, event_time);
            }
            if let protocol::Event::Message { text } = &e {
                // The same refusal repeated just refreshes its timer.
                self.messages.retain(|m| m.0 != *text);
                self.messages.push((text.clone(), 3.));
                if self.messages.len() > 3 {
                    self.messages.remove(0);
                }
            }
            if let protocol::Event::Rallied { unit } | protocol::Event::Revived { unit } = &e
                && let Some(c) = self.characters.get_mut(unit)
            {
                c.rallied();
            }
            // Blows (#218): the victim tips away from the blow, and both bodies' poses hold for
            // the hitstop. The showcase's characters aren't the sim's units.
            if let protocol::Event::Hit {
                attacker,
                victim,
                dir,
                weight,
                hitstop_ms,
            } = &e
                && !self.options.showcase
            {
                let hitstop = f32::from(*hitstop_ms) / 1000.;
                if let Some(c) = self.characters.get_mut(victim) {
                    c.hit(f32::from(*dir) * TAU / 256., *weight, hitstop);
                }
                if attacker != victim
                    && let Some(c) = self.characters.get_mut(attacker)
                {
                    c.landed(hitstop);
                }
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
                protocol::Event::LevelUp {
                    unit,
                    specialty,
                    level,
                } if *unit == self.host.player => {
                    self.level_up = Some((specialty.clone(), *level, 4.));
                }
                protocol::Event::SkillUp { unit, skill, level } if *unit == self.host.player => {
                    self.level_up = Some((skill.clone(), *level, 4.));
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
            let own = OWN_AHEAD.then_some(self.host.player);
            let lead = if OWN_AHEAD { pacing::DRAW_DELAY } else { 0. };
            self.latency.drew(
                r.next_frame(),
                self.snaps.previous.time + self.snaps.since() as f64 + lead,
                latency::now(),
            );
            self.snaps.units(!self.options.legacy_cpu, own)
        };
        let time = self.frame as f32 / 60.;
        self.reel_time =
            self.options.reel_from + (self.frame as f32 - self.options.warmup as f32) / 60.;
        if self.options.showcase {
            for u in &mut units {
                u.pose_time = time;
            }
        }
        if self.options.look_churn && self.options.showcase {
            std::sync::Arc::make_mut(&mut units[0].look).gear =
                if (self.frame / 60).is_multiple_of(2) {
                    vec![
                        ("main_hand".into(), "sword.arming".into()),
                        ("off_hand".into(), "shield.round".into()),
                    ]
                } else {
                    vec![("main_hand".into(), "sword.great".into())]
                };
        }
        if self.options.hit_churn {
            self.hit_churn(&mut units);
        }
        if self.options.crowd > 0
            && !self.options.showcase
            && let Some(base) = units.iter().find(|u| u.id == self.host.player)
        {
            let time = self.start.elapsed().as_secs_f32();
            let valley = &self.valley;
            let others = crowd::units(base, self.options.crowd, time, |x, z| valley.ground(x, z));
            units.extend(others);
        }
        let me = units
            .iter()
            .find(|u| u.id == self.host.player)
            .context("no local player")?;
        if let Some(v) = mac_cam(self.snaps.current.time) {
            // MAC CHECK PATCH: yaw,pitch,distance for the follow camera (ALDER_CAM, cam, camto).
            self.orbit.yaw = v[0];
            self.orbit.pitch = v[1];
            self.orbit.distance = v[2];
        }
        if let Ok(cams) = std::env::var("ALDER_CAMS") {
            // MAC CHECK PATCH: a camera schedule keyed by frame, easing over 20 frames.
            let keys: Vec<(f32, [f32; 3])> = cams
                .split(';')
                .filter(|k| !k.trim().is_empty())
                .map(|k| {
                    let (f, v) = k.split_once(':').unwrap();
                    let v: Vec<f32> = v.split(',').map(|x| x.trim().parse().unwrap()).collect();
                    (f.trim().parse().unwrap(), [v[0], v[1], v[2]])
                })
                .collect();
            let f = self.frame as f32;
            if let Some(i) = keys.iter().rposition(|k| k.0 <= f) {
                let to = keys[i].1;
                let from = if i > 0 { keys[i - 1].1 } else { to };
                let k = ((f - keys[i].0) / 20.).clamp(0., 1.);
                let k = k * k * (3. - 2. * k);
                self.orbit.yaw = from[0] + (to[0] - from[0]) * k;
                self.orbit.pitch = from[1] + (to[1] - from[1]) * k;
                self.orbit.distance = from[2] + (to[2] - from[2]) * k;
            }
        }
        if std::env::var("ALDER_SCRIPT").is_ok() {
            eprintln!(
                "pos frame={} t={:.3} p=({:.3},{:.3},{:.3}) pose={:?}",
                self.frame, self.snaps.current.time, me.position.x, me.position.y, me.position.z, me.pose
            );
        }
        // Hanging, moving between holds or running up a wall: frame the wall (#250).
        self.follow.climbing = match me.traversal {
            protocol::Traversal::Hanging => true,
            protocol::Traversal::Forced(f) => matches!(
                f.kind,
                protocol::ForcedKind::Hold
                    | protocol::ForcedKind::Corner
                    | protocol::ForcedKind::WallRun
            ),
            _ => false,
        };
        let scenery = &self.scenery;
        let view = self
            .follow
            .update(&self.orbit, me.position, dt, |target, eye| match scenery {
                Some(scenery) => scenery.camera_eye(target, eye, 0.25),
                None => eye,
            });
        self.view = Some(self.kick.apply(view, dt));
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
            self.snaps.current.time as f32 + self.snaps.since().min(1. / 60.)
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
        // Where each character looks (#212): you where the crosshair points (the ground under
        // it, or far along it into the sky), anyone with a target at its head, and townsfolk at a
        // player close by.
        let aim = self.aim_point();
        let crosshair_at = (self.aiming() && !self.options.showcase).then(|| {
            aim.unwrap_or(camera.eye + (camera.target - camera.eye).normalize_or_zero() * FAR_GAZE)
        });
        let heads: HashMap<u32, Vec3> = units
            .iter()
            .map(|u| {
                let height = body_height(&self.characters, &self.catalog, u);
                (u.id, u.position + Vec3::Y * EYES * height)
            })
            .collect();
        let gaze = |u: &UnitState| -> Option<Vec3> {
            if self.options.showcase {
                return None;
            }
            if u.id == me.id && crosshair_at.is_some() {
                return crosshair_at;
            }
            if let Some(target) = u.target.filter(|t| *t != u.id) {
                return heads
                    .get(&target)
                    .copied()
                    .filter(|at| at.distance(u.position) < GAZE_RANGE);
            }
            if u.kind != UnitKind::Npc {
                return None;
            }
            units
                .iter()
                .filter(|v| v.kind == UnitKind::Player)
                .map(|v| (v.position.distance_squared(u.position), v.id))
                .filter(|(d, _)| *d < NPC_GAZE_RANGE * NPC_GAZE_RANGE)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .and_then(|(_, id)| heads.get(&id).copied())
        };
        let gazes: Vec<_> = units.iter().map(gaze).collect();
        // How often each character's pose rebuilds (#215): the player, their target and whoever
        // targets them first, then the nearest, as many at the higher rates as the budget allows.
        let tiers: &[anim::Tier] = if self.options.anim_tiers && !self.options.showcase {
            self.animation.assign(units.iter().map(|u| {
                anim::Importance {
                    involved: u.id == me.id || Some(u.id) == me.target || u.target == Some(me.id),
                    distance: u.position.distance_squared(camera.eye),
                    tier: self
                        .characters
                        .get(&u.id)
                        .map_or(anim::Tier::Full, |c| c.tier()),
                }
            }))
        } else {
            &[]
        };
        let (mut posing, mut skinning, mut counts) = (Duration::ZERO, Duration::ZERO, [0; 3]);
        for (i, u) in units.iter().enumerate() {
            let mut look = (*u.look).clone();
            look.drawn = false;
            let height = body_height(&self.characters, &self.catalog, u);
            let depth = (u.position - camera.eye).dot((camera.target - camera.eye).normalize());
            let pixels = render::visibility::screen_height(height, depth, camera.fov, r.height);
            let lod = render::visibility::lod_for_height(
                pixels,
                self.characters.get(&u.id).map(|c| c.lod),
            );
            let changed = self.characters.get(&u.id).map(|c| {
                let mut old = c.look.clone();
                old.drawn = false;
                old != look
            });
            match changed {
                None => {
                    let mut c = Character::load_lod(&self.catalog, &mut self.cache, &u.look, lod)?;
                    c.set_ground(Arc::clone(&self.ground));
                    c.set_holds(Arc::clone(&self.holds));
                    self.characters.insert(u.id, c);
                }
                // New gear or clothes go on in place, so the motion carries on (#239).
                Some(true) => {
                    if self.options.reset_on_look {
                        r.reset_temporal_history();
                    }
                    let c = self.characters.get_mut(&u.id).unwrap();
                    c.set_look(&self.catalog, &mut self.cache, &u.look)?;
                }
                Some(false) => {}
            }
            let c = self.characters.get_mut(&u.id).unwrap();
            c.partial_updates = !self.options.legacy_cpu;
            c.blender.set_method(self.options.blend);
            c.foot_placement = self.options.foot_ik;
            c.leaning = self.options.lean;
            c.flinching = self.options.flinch;
            c.traversing = self.options.traversal;
            c.set_lod(&self.catalog, &mut self.cache, lod)?;
            let tier = tiers.get(i).copied().unwrap_or_default();
            counts[tier as usize] += 1;
            c.set_tier(tier);
            c.look_at(gazes[i]);
            let started = Instant::now();
            c.animate(
                u,
                if self.options.showcase {
                    None
                } else {
                    combat_index.get(&u.id).copied()
                },
                if self.options.showcase { 1. / 60. } else { dt },
                self.frame,
            );
            let posed = Instant::now();
            // Drawn at the body's yaw, which lags the sim's while turning on the spot (#211).
            draws.extend(c.draws(u.position, c.body_yaw()).into_iter().map(|mut d| {
                d.instance |= (u.id as u64) << 32;
                d
            }));
            posing += posed - started;
            skinning += posed.elapsed();
        }
        let animation = perf_overlay::Animation {
            pose_ms: posing.as_secs_f64() * 1000.,
            palettes_ms: skinning.as_secs_f64() * 1000.,
            tiers: counts,
        };
        self.animation.spent(animation.pose_ms as f32);
        let player = units
            .iter()
            .find(|u| u.id == self.host.player)
            .context("no local player in snapshot")?;
        self.frame += 1;
        // A smoke run also lasts until its schedule has played out, plus a second for the last
        // events, so a short --frames or slow frames can't end it early; a gait run or a course
        // until its script has.
        let done = self.options.frames.is_some_and(|n| self.frame >= n)
            && (!self.options.smoke || self.smoke_stage == 8 && self.smoke_wait() >= 1.)
            && (!self.options.gait
                || self.gait_tick.is_some() && gait::at(self.gait_time()).is_none())
            && (!self.options.traverse || self.course.step().is_none());
        self.perf
            .animate(animation, self.frame > self.options.warmup && !done);
        let frame_file = self
            .options
            .sequence
            .as_ref()
            .filter(|_| {
                self.frame > self.options.warmup
                    && (self.frame - self.options.warmup).is_multiple_of(self.options.sequence_step)
            })
            .map(|dir| dir.join(format!("{:05}.png", self.frame)));
        let capture = if done {
            self.options.capture.as_deref()
        } else {
            frame_file.as_deref()
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
        let radius = self
            .aim_slot
            .and_then(|i| self.snaps.current.player.as_ref()?.bar.get(i as usize))
            .filter(|s| s.ground && s.radius > 0.)
            .map(|s| s.radius);
        let world_effects =
            self.effects.is_some() && (!self.options.showcase || self.stage.is_some());
        if world_effects && let Some(fx) = &mut self.effects {
            let characters = &self.characters;
            let valley = &self.valley;
            let ground = |x, z| valley.ground(x, z);
            let stage_time = if self.options.reel {
                self.reel_time
            } else {
                scene_time
            };
            let staged = self
                .stage
                .as_mut()
                .map(|s| s.frame(stage_time, &units, &ground));
            if let Some(f) = &staged {
                // A new shot starts clean: nothing left lying from the last.
                if f.cut {
                    fx.clear();
                }
                for (e, at) in &f.events {
                    fx.event(e, *at);
                }
            }
            let (solids, frame) = fx.draws(&effects::Scene {
                fight: staged.as_ref().map_or(&*self.fight, |f| &f.fight),
                units: &units,
                height: &|id| characters.get(&id).map_or(1.8, |c| c.rig_height),
                ground: &ground,
                since: if staged.is_some() {
                    0.
                } else {
                    self.snaps.since()
                },
                time: staged.as_ref().map_or(scene_time, |f| f.clock),
                aim: staged.as_ref().map_or(aim.zip(radius), |f| f.aim),
            });
            draws.extend(solids);
            r.effects = Some(frame);
        }
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
        let reticle = if crosshair {
            self.reticle()
        } else {
            Reticle::Free
        };
        let keys = self.key_labels();
        let mut changes = Vec::new();
        let mut bag_actions = Vec::new();
        let mut craft_actions = Vec::new();
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
                    world_effects,
                );
            }
            if self.options.reel {
                paint_caption(ctx, stage::shot(self.reel_time).caption);
            } else {
                actions = hud::draw(ctx, &self.snaps.current, &self.fight, aim, &keys);
                hud::messages(ctx, &self.messages);
                if let Some(banner) = &self.level_up {
                    hud::level_up(ctx, banner);
                }
                if self.specialties {
                    actions.extend(hud::specialties(
                        ctx,
                        &self.snaps.current,
                        &mut self.assign_xp,
                    ));
                }
            }
            if let Some(player) = &self.snaps.current.player
                && let Some(me) = self.snaps.current.unit(self.host.player)
                && !self.options.showcase
            {
                bag::loot(ctx, &projection, player, me.position, &keys.loot);
                let at_shop = player.shop.is_some();
                if self.bag || at_shop {
                    bag_actions.extend(bag::bag(ctx, player));
                }
                bag_actions.extend(bag::shop(ctx, player));
                let interact = craft::interact(&self.snaps.current, player, me.position);
                craft::world(
                    ctx,
                    &projection,
                    &player.nodes,
                    me.position,
                    interact.as_ref(),
                    &keys.interact,
                );
                if let Some(w) = &player.work {
                    craft_actions.extend(craft::work(ctx, w, &keys.interact));
                }
                craft_actions.extend(craft::station(ctx, player));
                if self.skills_panel {
                    craft_actions.extend(craft::skills(ctx, player));
                }
            }
            if crosshair {
                paint_crosshair(ctx, reticle);
            }
            self.perf.draw(
                ctx,
                self.host.stats().step_ms,
                r.gpu_counters_enabled(),
                &self.pacing,
                &self.latency,
            );
            if let Some(m) = &self.menu {
                changes = menu::draw(ctx, m, &self.settings, &self.settings_path);
            }
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
                hud::Action::CloseSpecialties => self.specialties = false,
            }
        }
        for action in bag_actions {
            match action {
                bag::Action::Command(c) => self.host.send(c),
                bag::Action::Close => self.bag = false,
            }
        }
        for action in craft_actions {
            match action {
                craft::Action::Command(c) => self.host.send(c),
                craft::Action::CloseSkills => self.skills_panel = false,
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
        r.target_fps = self
            .options
            .fps
            .or(self.frame_period.map(|p| 1. / p))
            .unwrap_or(60.);
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
                "frame={} gpu={:.2}ms draws={} triangles={} sim={:.3}ms units={} pose={:.3}ms palettes={:.3}ms tiers={}/{}/{}",
                self.frame,
                stats.gpu_ms,
                stats.draw_calls,
                stats.triangles,
                self.host.stats().step_ms,
                units.len(),
                animation.pose_ms,
                animation.palettes_ms,
                animation.tiers[0],
                animation.tiers[1],
                animation.tiers[2]
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
            for kind in latency::Kind::ALL {
                if let Some((median, p95, n)) = self.latency.summary(kind) {
                    eprintln!(
                        "LATENCY {} press_to_photon median={median:.1}ms p95={p95:.1}ms samples={n}",
                        kind.name()
                    );
                }
                if let Some((median, p95, n)) = self.latency.to_frame(kind) {
                    eprintln!(
                        "LATENCY {} press_to_frame median={median:.1}ms p95={p95:.1}ms samples={n}",
                        kind.name()
                    );
                }
            }
            let [none, one, more] = self.pacing.total;
            eprintln!("PACING steps_per_frame 0={none} 1={one} 2+={more}");
            let (paced, by_arrival) = self.pacing.judder_ms();
            eprintln!(
                "PACING judder_rms paced={paced:.3}ms by_arrival={by_arrival:.3}ms (sim time drawn vs refreshes shown)"
            );
            if let Some(line) = self.perf.animation_report() {
                eprintln!("{line}");
            }
            let presentations = r.presentation_times(self.options.warmup as usize);
            if presentations.len() > 2 {
                let mut intervals: Vec<_> = presentations
                    .windows(2)
                    .map(|w| (w[1] - w[0]) * 1000.)
                    .filter(|t| *t > 0.)
                    .collect();
                let mean = intervals.iter().sum::<f64>() / intervals.len() as f64;
                intervals.sort_by(f64::total_cmp);
                let sd = (intervals.iter().map(|i| (i - mean).powi(2)).sum::<f64>()
                    / intervals.len() as f64)
                    .sqrt();
                eprintln!(
                    "PRESENT mean_fps={:.3} p95={:.3}ms sd={sd:.3}ms samples={}",
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
                let missing: Vec<_> = ["jump", "dodge", "damage", "swap"]
                    .into_iter()
                    .zip(self.smoke_checks)
                    .filter(|(_, seen)| !seen)
                    .map(|(name, _)| name)
                    .collect();
                ensure!(
                    moved > 2. && missing.is_empty(),
                    "smoke did not exercise simulation: moved={moved:.2}m (needs 2), events={}, no event for {missing:?}",
                    self.events
                );
                eprintln!("SMOKE PASS moved={moved:.2}m events={}", self.events);
            }
        }
        for change in changes {
            self.change(change);
        }
        Ok(done)
    }
}
/// Where the showcase lineup stands (z), north of the field wall along z = 21.6.
const LINEUP_Z: f32 = 23.5;

fn showcase(mut base: UnitState, valley: &world::Valley) -> Vec<UnitState> {
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
        base.position = Vec3::new(-4. + i as f32 * 2.8, 0., LINEUP_Z);
        base.position.y = valley.ground(base.position.x, base.position.z);
        base.yaw = 0.25;
        let mut look = Look {
            art: art.into(),
            pieces: pieces.iter().map(|s| s.to_string()).collect(),
            gear: vec![("main_hand".into(), weapon.into())],
            drawn,
            ..Default::default()
        };
        if let Some(s) = shield {
            look.gear.push(("off_hand".into(), s.into()));
        }
        if art == "male" {
            look.hair = Some("hair.simple_parted".into());
            look.brows = Some("brows.strong".into());
            look.beard = Some("beard.full".into());
        }
        if art == "elf_female" {
            look.hair = Some("hair.long".into());
        }
        if art == "dwarf_male" {
            look.gear.push(("back".into(), "cape.traveller".into()));
            look.beard = Some("beard.dwarven".into());
        }
        base.look = look.into();
        base.pose = Pose::Idle;
        units.push(base.clone());
    }
    base.id = first + 5;
    base.look = Look {
        art: "wolf.grey".into(),
        ..Default::default()
    }
    .into();
    base.position = Vec3::new(8., valley.ground(8., LINEUP_Z + 2.), LINEUP_Z + 2.);
    units.push(base);
    units
}
/// A small ring with four ticks at the centre of the screen, where aimed skills go.
/// What the crosshair says about your attacks, so you know before pressing a key whether a
/// skill goes to your target or to the crosshair.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reticle {
    /// No target: skills fire at the crosshair.
    Free,
    /// No target, but an enemy under the crosshair is within the basic attack's reach.
    EnemyInReach,
    /// Your target is within the basic attack's reach.
    TargetInRange,
    /// Your target is further away than that.
    TargetOutOfRange,
}

/// The reticle from how far away your target is (if you have one), how far away a living
/// enemy under the crosshair is (if any), and the basic attack's reach.
fn reticle(target: Option<f32>, under: Option<f32>, reach: f32) -> Reticle {
    match (target, under) {
        (Some(d), _) if d <= reach => Reticle::TargetInRange,
        (Some(_), _) => Reticle::TargetOutOfRange,
        (None, Some(d)) if d <= reach => Reticle::EnemyInReach,
        _ => Reticle::Free,
    }
}

fn paint_crosshair(ctx: &egui::Context, reticle: Reticle) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("crosshair"),
    ));
    let c = ctx.content_rect().center();
    // Colour, and how far out the ticks start and end.
    let (colour, ticks) = match reticle {
        Reticle::Free => (egui::Color32::from_white_alpha(220), (7., 13.)),
        Reticle::EnemyInReach => (egui::Color32::from_rgb(250, 190, 70), (6., 12.)),
        Reticle::TargetInRange => (egui::Color32::from_rgb(240, 70, 55), (5., 11.)),
        Reticle::TargetOutOfRange => (
            egui::Color32::from_rgb(240, 70, 55).gamma_multiply(0.55),
            (11., 16.),
        ),
    };
    let shadow = egui::Stroke::new(3f32, egui::Color32::from_black_alpha(110));
    let line = egui::Stroke::new(1.5f32, colour);
    for stroke in [shadow, line] {
        painter.circle_stroke(c, 3., stroke);
        for d in [
            egui::vec2(1., 0.),
            egui::vec2(-1., 0.),
            egui::vec2(0., 1.),
            egui::vec2(0., -1.),
        ] {
            painter.line_segment([c + d * ticks.0, c + d * ticks.1], stroke);
        }
    }
    if reticle == Reticle::TargetInRange {
        painter.circle_filled(c, 1.8, colour);
    }
}
/// The reel's caption, top centre on a dark plate.
fn paint_caption(ctx: &egui::Context, text: &str) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("caption"),
    ));
    let galley = painter.layout_no_wrap(
        text.into(),
        egui::FontId::proportional(24.),
        egui::Color32::from_white_alpha(235),
    );
    let top = egui::pos2(ctx.content_rect().center().x - galley.size().x / 2., 22.);
    let plate = egui::Rect::from_min_size(top, galley.size()).expand2(egui::vec2(16., 8.));
    painter.rect_filled(plate, 6., egui::Color32::from_black_alpha(150));
    painter.galley(top, galley, egui::Color32::WHITE);
}
/// How tall `u` stands, in metres: its character's rig, or its body type's before it has one.
fn body_height(
    characters: &HashMap<u32, Character>,
    catalog: &assets::Catalog,
    u: &UnitState,
) -> f32 {
    characters
        .get(&u.id)
        .map(|c| c.rig_height)
        .or_else(|| {
            catalog
                .humanoid
                .body_types
                .get(&u.look.art)
                .map(|b| b.height)
        })
        .unwrap_or(1.5)
}
/// Mouse travel, in pixels, that turns a click into a drag.
const DRAG_THRESHOLD: f64 = 4.;
/// Your own character is drawn at the newest sim time rather than a step behind (#219).
const OWN_AHEAD: bool = true;
/// How often a held attack button presses the basic attack again, in seconds.
const SWING_REPEAT: f32 = 0.1;
/// Where characters look (#212): how far along the crosshair you look when it points at the sky,
/// how far off a target may be and still be looked at, and how close a player comes before
/// townsfolk look at them, in metres; and the eyes' height, as a share of the body's.
const FAR_GAZE: f32 = 100.;
const GAZE_RANGE: f32 = 40.;
const NPC_GAZE_RANGE: f32 = 5.;
const EYES: f32 = 0.93;
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
    focused: bool,
    proxy: EventLoopProxy<()>,
    /// Starts frames on the window's display refresh; without one, a timer does.
    display_link: Option<pacing::DisplayLink>,
}
impl App {
    /// A key or mouse button 3-5 going down or up: does whatever it is bound to.
    fn input(&mut self, input: controls::Input, pressed: bool, repeat: bool) {
        let bound = self.game.settings.bindings.controls(&input);
        if pressed {
            if self.game.menu.is_some() {
                return;
            }
            self.game.keys.insert(input);
            if !repeat {
                for control in bound {
                    self.game.press(control);
                }
            }
        } else {
            self.game.keys.remove(&input);
            for control in bound {
                self.game.release(control);
            }
        }
        self.sync_pointer(self.focused);
    }
    /// Esc closes the menu (or stops it waiting for a key) or the Specialties screen, and opens
    /// the menu otherwise.
    fn escape(&mut self) {
        match &mut self.game.menu {
            None if self.game.held_ground.is_some() => self.game.held_ground = None,
            None if self.game.specialties => self.game.specialties = false,
            None if self.game.bag => self.game.bag = false,
            None if self.game.skills_panel => self.game.skills_panel = false,
            Some(m) if m.capture.is_some() => {
                m.capture = None;
                m.notice = None;
            }
            Some(_) => self.game.menu = None,
            None => {
                self.end_drag();
                self.game.open_menu();
            }
        }
        self.sync_pointer(self.focused);
    }
    /// Action: left attacks (targeting what's under the crosshair) and right holds the guard.
    /// Classic: left drags the camera, right steers; a press that barely moves is a click
    /// instead, which targets (and with the right button, attacks) the unit under the cursor.
    /// The middle and thumb buttons are bindable inputs like keys.
    fn button(&mut self, button: MouseButton, pressed: bool) {
        // The right button cancels a held ground skill instead of guarding or steering.
        if button == MouseButton::Right && pressed && self.game.held_ground.take().is_some() {
            return;
        }
        match button {
            MouseButton::Left | MouseButton::Right if pressed && self.game.menu.is_some() => {
                return;
            }
            MouseButton::Left => self.game.orbiting = pressed,
            MouseButton::Right => self.game.mouse_look = pressed,
            other => {
                if let Some(input) = mouse_input(other) {
                    self.input(input, pressed, false);
                }
                return;
            }
        }
        if self.pointer_held {
            if button == MouseButton::Right {
                self.game.guard_button(pressed);
                return;
            }
            // A swing takes whatever is under the crosshair as its target.
            if pressed && let Some(w) = &self.window {
                let s = w.inner_size();
                let (x, y) = (s.width as f64 / 2., s.height as f64 / 2.);
                if let Some(id) = self.game.pick(x, y, s.width, s.height) {
                    self.game.host.send(Command::SetTarget(Some(id)));
                }
            }
            self.game.attack_button(pressed);
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
            && !self.game.screen_open()
            && self.game.menu.is_none()
            && !self.game.options.showcase;
        if want == self.pointer_held {
            return;
        }
        self.end_drag();
        // The buttons' releases would land in the other scheme's handling.
        self.game.attack_button(false);
        if self.game.guard_button {
            self.game.guard_button(false);
        }
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
            if let Some(art) = &self.game.effects_art {
                r.load_effects(art)?;
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
            {
                use winit::platform::macos::MonitorHandleExtMacOS;
                let display = w.current_monitor().map_or(0, |m| m.native_id());
                self.display_link = pacing::DisplayLink::start(self.proxy.clone(), display);
                match &self.display_link {
                    Some(link) => {
                        // `--fps` caps the rate; the display sets it otherwise.
                        let cap = self.game.options.fps.map_or(0., |f| 1. / f);
                        self.game.frame_period = Some(link.period.max(cap));
                        eprintln!("PACING display {:.2} Hz", 1. / link.period);
                    }
                    None => eprintln!("PACING timer (no display link)"),
                }
            }
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
        // The menu waiting for a key takes the next one before egui (or the game) sees it.
        if self.game.menu.as_ref().is_some_and(|m| m.capture.is_some()) {
            let captured = match &event {
                WindowEvent::KeyboardInput { event, .. }
                    if event.state == ElementState::Pressed && !event.repeat =>
                {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Escape) => {
                            self.escape();
                            return;
                        }
                        PhysicalKey::Code(key) => Some(key_input(key)),
                        _ => None,
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button,
                    ..
                } => mouse_input(*button),
                _ => None,
            };
            if let Some(input) = captured {
                self.game.bind_captured(input);
                return;
            }
        }
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
                    self.input(key_input(key), false, false);
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
                self.focused = false;
                self.game.release_all();
                self.end_drag();
                self.sync_pointer(false);
            }
            WindowEvent::Focused(true) => {
                self.focused = true;
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
                    let pressed = event.state == ElementState::Pressed;
                    if key == KeyCode::Escape {
                        if pressed && !event.repeat {
                            self.escape();
                        }
                    } else {
                        self.input(key_input(key), pressed, event.repeat);
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
                    if self.game.quit {
                        event_loop.exit();
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
                        w.set_title(&format!("Aldermoor · HP {:.0}/{:.0} · GPU {:.1} ms · mouse aims · Ctrl pointer · WASD · Space jump · Alt roll · Tab target · 1–9 skills · Esc controls",p.health,p.max_health,r.stats.gpu_ms));
                    }
                }
                // The menu may have opened, closed or switched scheme.
                self.sync_pointer(self.focused);
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
    /// A refresh of the window's display: draw, unless `--fps` caps below the refresh rate
    /// and this one comes too soon.
    fn user_event(&mut self, _: &ActiveEventLoop, _: ()) {
        let Some(link) = &self.display_link else {
            return;
        };
        link.taken();
        let now = Instant::now();
        if let Some(fps) = self.game.options.fps
            && 1. / fps > link.period * 1.05
        {
            // Allow for the refresh landing a little early.
            if now + Duration::from_secs_f64(link.period * 0.5) < self.next_redraw {
                return;
            }
            self.next_redraw = (self.next_redraw + Duration::from_secs_f64(1. / fps)).max(now);
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }
    fn about_to_wait(&mut self, e: &ActiveEventLoop) {
        if self.display_link.is_some() {
            e.set_control_flow(ControlFlow::Wait);
            return;
        }
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
        if let Some(art) = &game.effects_art {
            renderer.load_effects(art)?;
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
            } else if game.options.smoke || game.options.gait || game.options.traverse {
                std::thread::sleep(Duration::from_millis(16).saturating_sub(start.elapsed()));
            }
        }
    } else {
        let event_loop = EventLoop::new()?;
        let proxy = event_loop.create_proxy();
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
            focused: true,
            proxy,
            display_link: None,
        };
        event_loop.run_app(&mut app)?;
        if let Some(e) = app.error {
            return Err(e);
        }
    }
    Ok(())
}

/// A key by its winit name, which is how the bindings name it.
fn key_input(key: KeyCode) -> controls::Input {
    controls::Input(format!("{key:?}"))
}

/// Why a key can't be bound, for keys something else takes before the game sees them.
fn reserved(input: &controls::Input) -> Option<String> {
    (*input == key_input(KeyCode::F3)).then(|| {
        format!(
            "{} is the performance overlay. Press another key.",
            input.label()
        )
    })
}

/// Seconds from `from` to `to`, negative if `to` came first.
fn seconds_between(from: Instant, to: Instant) -> f64 {
    match to.checked_duration_since(from) {
        Some(d) => d.as_secs_f64(),
        None => -(from - to).as_secs_f64(),
    }
}

/// How far a blow kicks the camera, in metres, if it does: the player's own heavy, critical
/// and parried blows, and every blow they take. Other people's fights and the player's light
/// blows leave it still.
fn kick_for(weight: protocol::HitWeight, mine: bool, on_me: bool) -> Option<f32> {
    use protocol::HitWeight::*;
    match (weight, mine, on_me) {
        (Light, false, true) => Some(0.02),
        (Blocked, false, true) => Some(0.015),
        (Heavy, _, true) | (Heavy, true, _) => Some(0.04),
        (Critical, _, true) | (Critical, true, _) => Some(0.05),
        (Parried, _, true) | (Parried, true, _) => Some(0.045),
        (Big, _, true) | (Big, true, _) => Some(0.07),
        _ => None,
    }
}

/// The middle and thumb buttons; the left and right ones aren't bindable.
fn mouse_input(button: MouseButton) -> Option<controls::Input> {
    let n = match button {
        MouseButton::Middle => 3,
        MouseButton::Back => 4,
        MouseButton::Forward => 5,
        MouseButton::Other(n) => n + 1,
        MouseButton::Left | MouseButton::Right => return None,
    };
    Some(controls::Input(format!("Mouse{n}")))
}

#[cfg(test)]
mod catalog_defaults_tests {
    use super::*;
    #[test]
    fn the_showcase_lineup_stands_clear_of_the_valleys_solids() {
        let valley = world::Valley::new();
        // Any unit will do: the lineup takes its looks and places from showcase().
        let mut host = server::Host::start(1, "Review");
        let base = host.snapshot().unwrap().unit(host.player).unwrap().clone();
        host.stop();
        for u in showcase(base, &valley) {
            let p = u.position;
            assert!(
                !valley.blocked(Vec2::new(p.x, p.z), 0.45),
                "unit {} stands in a solid at {p}",
                u.id
            );
            assert_eq!(p.y, valley.ground(p.x, p.z));
        }
    }
    #[test]
    fn reticle_follows_target_and_reach() {
        assert_eq!(reticle(None, None, 3.), Reticle::Free);
        assert_eq!(reticle(None, Some(2.), 3.), Reticle::EnemyInReach);
        assert_eq!(reticle(None, Some(9.), 3.), Reticle::Free);
        assert_eq!(reticle(Some(2.5), None, 3.), Reticle::TargetInRange);
        assert_eq!(reticle(Some(12.), Some(1.), 3.), Reticle::TargetOutOfRange);
    }
    #[test]
    fn the_overlay_key_cannot_be_bound() {
        assert_eq!(
            reserved(&key_input(KeyCode::F3)).as_deref(),
            Some("F3 is the performance overlay. Press another key.")
        );
        assert_eq!(reserved(&key_input(KeyCode::F4)), None);
        assert_eq!(reserved(&controls::Input::new("Mouse4")), None);
    }
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

/// MAC CHECK PATCH: the camera's yaw, pitch and distance, easing from one to another.
type MacCam = Option<([f32; 3], [f32; 3], f64, f64)>;
static MAC_CAM: std::sync::Mutex<MacCam> = std::sync::Mutex::new(None);
fn mac_cam(now: f64) -> Option<[f32; 3]> {
    let mut c = MAC_CAM.lock().unwrap();
    if c.is_none()
        && let Ok(s) = std::env::var("ALDER_CAM")
    {
        let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
        *c = Some(([v[0], v[1], v[2]], [v[0], v[1], v[2]], now, 0.));
    }
    c.map(|(a, b, t0, d)| {
        let k = if d <= 0. { 1. } else { ((now - t0) / d).clamp(0., 1.) as f32 };
        [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
    })
}
