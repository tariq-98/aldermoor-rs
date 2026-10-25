use glam::{Mat4, Vec3};
use protocol::{Look, Pose, UnitKind, UnitState};
use render::{
    character::{Character, hidden_regions},
    model::{ModelCache, world_matrices},
};
use std::path::PathBuf;
static ASSET_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn catalog() -> assets::Catalog {
    assets::Catalog::load(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")).unwrap()
}
fn unit(look: Look) -> UnitState {
    UnitState {
        id: 1,
        kind: UnitKind::Player,
        body: "player".into(),
        name: "Test".into(),
        level: 1,
        position: Vec3::ZERO,
        velocity: Vec3::ZERO,
        yaw: 0.,
        pose: Pose::Idle,
        pose_time: 0.25,
        health: 100.,
        max_health: 100.,
        hostile: false,
        target: None,
        look: look.into(),
    }
}
#[test]
fn body_bind_pose_round_trips_and_clothing_uses_named_joints() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    for art in ["male", "female", "dwarf_male", "elf_female", "goblin_male"] {
        let look = Look {
            art: art.into(),
            pieces: vec![
                "peasant.top".into(),
                "peasant.trousers".into(),
                "peasant.shoes".into(),
            ],
            gear: vec![
                ("main_hand".into(), "sword.arming".into()),
                ("off_hand".into(), "shield.round".into()),
            ],
            ..Default::default()
        };
        let mut c = Character::load(&cat, &mut cache, &look).unwrap();
        assert!(c.parts[0].hidden != 0);
        let body = &c.parts[0];
        let covered = body
            .model
            .primitives
            .iter()
            .flat_map(|p| &p.vertices)
            .filter(|v| {
                let r = v.position[3] as i32;
                (0..31).contains(&r) && body.hidden & (1 << r) != 0
            })
            .count();
        assert!(
            covered > 1000,
            "{art}: coverage mask must match decoded _REGION values, not just catalogue metadata"
        );
        for part in &c.parts {
            for skin in &part.model.skins {
                for (&j, inv) in skin.joints.iter().zip(&skin.inverse) {
                    let m = c.world[part.remap[j]] * *inv;
                    assert!(
                        (m - Mat4::IDENTITY)
                            .to_cols_array()
                            .iter()
                            .all(|x| x.abs() < 0.001),
                        "{art}: {} bind matrix mismatch",
                        part.model.nodes[j].name
                    );
                }
            }
        }
        let rest = c.world[c.names["hand_r"]];
        c.update(&unit(look.clone()), 0.2);
        assert_ne!(rest, c.world[c.names["hand_r"]]);
        let draws = c.draws(Vec3::ZERO, 0.);
        assert!(draws.iter().any(|d| !d.palette.is_empty()));
        assert!(
            draws
                .iter()
                .all(|d| d.palette.iter().all(|m| m.is_finite()))
        );
        let sword = draws
            .iter()
            .find(|d| d.model.key.ends_with("sword.arming.gltf"))
            .unwrap();
        let socket = c.world[c.names["socket_hip_l"]];
        assert!(
            sword
                .transform
                .w_axis
                .truncate()
                .distance(socket.w_axis.truncate())
                < 0.001
        );
    }
}
#[test]
fn wolf_uses_its_own_rig_and_animations() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let look = Look {
        art: "wolf.grey".into(),
        ..Default::default()
    };
    let mut c = Character::load(&cat, &mut cache, &look).unwrap();
    let mut u = unit(look);
    u.pose = Pose::Run;
    u.velocity = Vec3::Z * 4.;
    c.update(&u, 0.2);
    let first = c.world.clone();
    u.pose_time = 0.5;
    c.update(&u, 0.2);
    assert_ne!(first, c.world);
    assert!(c.world.iter().all(|m| m.is_finite()));
}
#[test]
fn higher_layers_hide_only_the_named_lower_piece() {
    let regions = vec!["chest".into(), "hips".into()];
    let covers = vec![
        (
            "shirt".into(),
            1.1,
            serde_json::json!({"male":{"body":["chest"]}}),
        ),
        (
            "plate".into(),
            2.,
            serde_json::json!({"male":{"body":["hips"],"shirt":["chest"]}}),
        ),
    ];
    assert_eq!(hidden_regions("body", 0., "male", &covers, &regions), 3);
    assert_eq!(hidden_regions("shirt", 1.1, "male", &covers, &regions), 1);
    assert_eq!(hidden_regions("plate", 2., "male", &covers, &regions), 0);
}
#[test]
fn draw_and_sheathe_switch_sockets_at_clip_events() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let look = Look {
        art: "male".into(),
        gear: vec![("main_hand".into(), "sword.arming".into())],
        ..Default::default()
    };
    let mut c = Character::load(&cat, &mut cache, &look).unwrap();
    let mut u = unit(look);
    c.update(&u, 0.2);
    std::sync::Arc::make_mut(&mut u.look).drawn = true;
    c.update(&u, 0.01);
    let at_hip = c
        .draws(Vec3::ZERO, 0.)
        .into_iter()
        .find(|d| d.model.key.ends_with("sword.arming.gltf"))
        .unwrap();
    assert!(
        at_hip
            .transform
            .w_axis
            .truncate()
            .distance(c.world[c.names["socket_hip_l"]].w_axis.truncate())
            < 0.001
    );
    for _ in 0..120 {
        c.update(&u, 1. / 60.);
    }
    let at_hand = c
        .draws(Vec3::ZERO, 0.)
        .into_iter()
        .find(|d| d.model.key.ends_with("sword.arming.gltf"))
        .unwrap();
    assert!(
        at_hand
            .transform
            .w_axis
            .truncate()
            .distance(c.world[c.names["socket_hand_r"]].w_axis.truncate())
            < 0.001
    );
}
#[test]
fn gltf_children_before_parents_are_supported() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let m = cache
        .get(&cat.path(&cat.humanoid.body_types["male"].file))
        .unwrap();
    assert!(
        m.nodes
            .iter()
            .enumerate()
            .any(|(i, n)| n.parent.is_some_and(|p| p > i))
    );
    let world = world_matrices(
        &m.nodes,
        &m.nodes.iter().map(|n| n.rest).collect::<Vec<_>>(),
    );
    assert!(world.iter().all(|m| m.is_finite()));
}

#[test]
fn bow_zero_morph_accessors_decode_and_cape_joints_stay_finite() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let bow = cache.get(&cat.path("gear/bow.hunting.gltf")).unwrap();
    assert!(
        bow.primitives
            .iter()
            .any(|p| p.morph_names.iter().any(|n| n == "draw"))
    );
    assert!(
        bow.primitives
            .iter()
            .flat_map(|p| &p.morphs)
            .all(|m| m.iter().flatten().all(|x| x.is_finite()))
    );
    let look = Look {
        art: "dwarf_male".into(),
        gear: vec![("back".into(), "cape.traveller".into())],
        ..Default::default()
    };
    let mut c = Character::load(&cat, &mut cache, &look).unwrap();
    let mut u = unit(look);
    let chain = c.names["cape_2_2"];
    let collar = c.names["spine_03"];
    assert_ne!(chain, collar);
    for frame in 0..120 {
        u.pose = Pose::Run;
        u.velocity = Vec3::Z * 4.5;
        u.position.z = frame as f32 * 4.5 / 60.;
        u.pose_time = frame as f32 / 60.;
        c.update(&u, 1. / 60.);
        assert!(c.world.iter().all(|m| m.is_finite()));
        assert!(
            c.world[chain]
                .w_axis
                .truncate()
                .distance(c.world[collar].w_axis.truncate())
                < 2.
        );
    }
    // Teleports reset particles instead of dragging the cape across the map.
    u.position.x = 150.;
    c.update(&u, 1. / 60.);
    assert!(
        c.world[chain]
            .w_axis
            .truncate()
            .distance(c.world[collar].w_axis.truncate())
            < 2.
    );
}

#[test]
fn every_wolf_lod_loads_its_own_textures_and_starts_animated() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    for art in ["wolf.grey", "wolf.dire"] {
        for lod in 0..=2 {
            let look = Look {
                art: art.into(),
                ..Default::default()
            };
            let c = Character::load_lod(&cat, &mut cache, &look, lod).unwrap();
            let body = &c.parts[0].model;
            assert!(
                body.primitives.len() > 3,
                "eyes/coat/teeth must remain distinct"
            );
            assert!(
                body.primitives
                    .iter()
                    .any(|p| p.material.maps[0].is_some() && p.material.maps[1].is_some())
            );
            assert_eq!(c.lod, lod);
            assert!(
                c.nodes
                    .iter()
                    .zip(&c.local)
                    .any(|(n, l)| n.rest.rotation.dot(l.rotation).abs() < 0.9999
                        || n.rest.translation.distance(l.translation) > 1e-4),
                "beast must never begin in its jaw-open bind pose"
            );
            if lod > 0 {
                assert!(body.key.contains(&format!(".lod{lod}.glb")));
            }
        }
    }
}

#[test]
fn dyes_use_authored_references_and_share_masks_without_tinting_brass() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let mut look = Look {
        art: "dwarf_male".into(),
        beard: Some("beard.dwarven".into()),
        hair_colour: Some("white".into()),
        pieces: vec!["robe.healer".into()],
        gear: vec![("back".into(), "cape.traveller".into())],
        ..Default::default()
    };
    let dye = cat.humanoid.dyes[0].id.clone();
    look.dyes = vec![
        ("robe.healer".into(), Some(dye.clone()), None),
        ("cape.traveller".into(), Some(dye.clone()), Some(dye)),
    ];
    let a = Character::load(&cat, &mut cache, &look).unwrap();
    let b = Character::load_lod(&cat, &mut cache, &look, 1).unwrap();
    let robe = a
        .parts
        .iter()
        .find(|p| p.model.key.contains("robe.healer"))
        .unwrap();
    let cape = a
        .parts
        .iter()
        .find(|p| p.model.key.contains("cape.traveller"))
        .unwrap();
    let lower_cape = b
        .parts
        .iter()
        .find(|p| p.model.key.contains("cape.traveller"))
        .unwrap();
    let cloth = robe.appearance.dye.as_ref().unwrap();
    assert_eq!(cloth.colours[0][3], 1.);
    assert_eq!(cloth.colours[1][3], 0.);
    assert!(cloth.references[0][..3].iter().all(|v| *v > 0.));
    assert!(std::sync::Arc::ptr_eq(
        &cape.appearance.dye.as_ref().unwrap().mask,
        &lower_cape.appearance.dye.as_ref().unwrap().mask
    ));
    let mut hair_count = 0;
    let mut brass_count = 0;
    for d in a.draws(glam::Vec3::ZERO, 0.) {
        let m = &d.model.primitives[d.primitive].material;
        if d.model.key.contains("beard.dwarven") && m.name.starts_with("hair.") {
            hair_count += 1;
        }
        if d.model.key.contains("beard.dwarven") && !m.name.starts_with("hair.") {
            brass_count += 1;
            assert_eq!(
                d.base, m.base,
                "brass beard rings must keep their own material colour"
            );
        }
    }
    assert!(
        hair_count > 0 && brass_count > 0,
        "beard must exercise both tintable hair and untouched brass"
    );
}

#[test]
fn colourways_replace_only_materials_named_by_the_catalogue() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let (stem, choices) = cat
        .humanoid
        .texture_variants
        .iter()
        .find(|(key, _)| key.contains("Peasant"))
        .unwrap();
    let wanted = &choices[0];
    let look = Look {
        art: "male".into(),
        pieces: vec!["peasant.top".into()],
        variants: vec![("peasant.top".into(), wanted.clone())],
        ..Default::default()
    };
    let c = Character::load(&cat, &mut cache, &look).unwrap();
    let draws = c.draws(glam::Vec3::ZERO, 0.);
    assert!(
        draws
            .iter()
            .any(|d| d.base_map.as_ref().is_some_and(|m| m.key.contains(wanted)))
    );
    for d in draws {
        let p = &d.model.primitives[d.primitive];
        if p.material.name.starts_with("skin.") {
            assert!(
                d.base_map.is_none(),
                "skin must not receive the {stem} outfit colourway"
            );
        }
    }
}

#[test]
fn scenery_uses_only_supplied_placements_and_models() {
    let _asset_guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let mut scene = render::scenery::Scenery::new(&cat, &mut cache).unwrap();
    let v = world::Valley::new();
    let env = cat.environment.as_ref().unwrap();
    assert_eq!(
        scene.len(),
        v.houses.len()
            + v.trees.len()
            + 1
            + env.placements.landmarks.len()
            + env.placements.dressing.len()
            + v.masonry.len()
    );
    let camera = render::Camera {
        eye: glam::Vec3::new(6., 15., 28.),
        target: glam::Vec3::new(6., 8., 0.),
        fov: 1.,
    };
    let a = scene.draws(&cat, &mut cache, camera, 1440, 0.).unwrap();
    let b = scene.draws(&cat, &mut cache, camera, 1440, 1.).unwrap();
    assert!(a.iter().all(|d| d.model.key.contains("/environment/")));
    assert!(
        a.iter()
            .zip(&b)
            .any(|(a, b)| a.model.key.contains("sails") && a.transform != b.transform)
    );
    assert!(a.iter().any(|d| {
        d.model.primitives[d.primitive]
            .vertices
            .iter()
            .any(|v| v.normal[3] > 0.)
    }));
}

#[test]
fn lod_switch_keeps_the_live_rig_and_skin_palettes_are_shared() {
    let _guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let look = armoured_look();
    let mut c = Character::load(&cat, &mut cache, &look).unwrap();
    let mut u = unit(look);
    u.pose = Pose::Run;
    u.velocity = Vec3::X * 6.;
    for _ in 0..12 {
        c.update(&u, 1. / 60.);
    }
    let local = c.local.iter().map(|t| t.matrix()).collect::<Vec<_>>();
    let world = c.world.clone();
    for lod in [1, 2, 0, 2, 1, 0] {
        c.set_lod(&cat, &mut cache, lod).unwrap();
        assert_eq!(
            world, c.world,
            "LOD must not reset animation or spring transforms"
        );
        assert_eq!(
            local,
            c.local.iter().map(|t| t.matrix()).collect::<Vec<_>>()
        );
    }
    let draws = c.draws(Vec3::ZERO, 0.);
    let skinned = draws
        .iter()
        .filter(|d| !d.palette.is_empty())
        .collect::<Vec<_>>();
    let unique: std::collections::HashSet<_> = skinned.iter().map(|d| d.palette.as_ptr()).collect();
    assert!(
        unique.len() < skinned.len() / 2,
        "share one palette per skin"
    );
}
fn armoured_look() -> Look {
    Look {
        art: "male".into(),
        pieces: [
            "peasant.top",
            "peasant.trousers",
            "peasant.shoes",
            "plate.cuirass",
            "plate.legs",
            "plate.gauntlets",
            "plate.pauldrons",
        ]
        .map(str::to_owned)
        .into(),
        gear: vec![
            ("main_hand".into(), "sword.arming".into()),
            ("off_hand".into(), "shield.round".into()),
            ("back".into(), "cape.traveller".into()),
        ],
        drawn: true,
        ..Default::default()
    }
}
#[cfg(target_os = "macos")]
#[test]
fn metal_draws_sixty_armoured_characters() {
    let _guard = ASSET_TEST_LOCK.lock().unwrap();
    let cat = catalog();
    let mut cache = ModelCache::default();
    let mut characters: Vec<_> = (0..60)
        .map(|_| Character::load(&cat, &mut cache, &armoured_look()).unwrap())
        .collect();
    let mut renderer = render::Renderer::new(640, 360, 0.65).unwrap();
    renderer.set_frames_in_flight(2).unwrap();
    for frame in 0..3 {
        let mut draws = Vec::new();
        for (i, c) in characters.iter_mut().enumerate() {
            let mut u = unit(armoured_look());
            u.pose_time = frame as f32 / 60.;
            c.update(&u, 1. / 60.);
            let pos = Vec3::new((i % 10) as f32 * 2. - 9., 0., (i / 10) as f32 * 2.);
            draws.extend(c.draws(pos, 0.).into_iter().map(|mut d| {
                d.instance |= ((i + 1) as u64) << 32;
                d
            }));
        }
        let stats = renderer
            .draw(
                render::Camera {
                    eye: Vec3::new(0., 14., 32.),
                    target: Vec3::new(0., 1., 5.),
                    fov: 1.1,
                },
                &draws,
                None,
            )
            .unwrap();
        assert!(stats.triangles > 1_000_000);
        assert!(stats.bone_bytes > 64 * 1024 && stats.bone_capacity >= stats.bone_bytes);
    }
    renderer.wait().unwrap();
}
