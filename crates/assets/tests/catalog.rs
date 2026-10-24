//! The real catalogue in `assets/` keeps every promise it makes.

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

#[test]
fn the_catalogue_is_sound() {
    let cat = assets::Catalog::load(root()).expect("catalogue loads");
    let problems = cat.check();
    for p in &problems {
        eprintln!("{p}");
    }
    assert!(problems.is_empty(), "{} catalogue problems (listed above)", problems.len());
}

#[test]
fn every_dressable_body_type_can_wear_the_whole_wardrobe() {
    let cat = assets::Catalog::load(root()).unwrap();
    for (bt, info) in &cat.humanoid.body_types {
        if info.creature {
            continue;
        }
        for p in &cat.humanoid.pieces {
            assert!(p.files.contains_key(bt), "{} has no fit for {bt}", p.id);
        }
    }
}

#[test]
fn no_spiders_anywhere() {
    // The content rule from docs/DESIGN.md, checked on its own so it can never be lost in a list.
    let cat = assets::Catalog::load(root()).unwrap();
    assert!(cat.check().iter().all(|p| !p.0.starts_with("forbidden content")));
}

#[test]
fn the_checker_catches_broken_promises() {
    let mut cat = assets::Catalog::load(root()).unwrap();
    cat.humanoid.body_types.get_mut("male").unwrap().sockets.push("socket_nowhere".into());
    cat.humanoid.pieces[0].hides.get_mut("male").unwrap().insert("body".into(), vec!["elbow_pit".into()]);
    cat.humanoid.pieces[1].files.insert("male".into(), "humanoid/male/outfits/missing.gltf".into());
    cat.humanoid.pieces[2].label = "Webbing of a giant spider".into();
    let problems: Vec<String> = cat.check().into_iter().map(|p| p.0).collect();
    assert!(problems.iter().any(|p| p.contains("socket_nowhere")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("elbow_pit")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("missing.gltf")), "{problems:?}");
    assert!(problems.iter().any(|p| p.starts_with("forbidden content")), "{problems:?}");
}

#[test]
fn clip_curves_follow_the_documented_rule() {
    let keys = [[0.3, 0.0], [0.3, 1.0], [0.72, 1.0], [0.8, 0.0]];
    assert_eq!(assets::curve(&keys, 0.0), 0.0, "held before the first key");
    assert_eq!(assets::curve(&keys, 0.29), 0.0);
    assert_eq!(assets::curve(&keys, 0.3), 1.0, "at a step the later key wins");
    assert_eq!(assets::curve(&keys, 0.5), 1.0);
    assert!((assets::curve(&keys, 0.76) - 0.5).abs() < 1e-5, "linear between keys");
    assert_eq!(assets::curve(&keys, 2.0), 0.0, "held after the last key");
    assert_eq!(assets::curve(&[], 1.0), 0.0);
}

#[test]
fn authored_clips_carry_what_the_renderer_needs() {
    let cat = assets::Catalog::load(root()).unwrap();
    let clip = |name: &str| cat.humanoid.animations.clips.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no clip {name}"));
    let shoot = clip("Bow_Shoot");
    let release = shoot.event("release").expect("the arrow leaves at a release event");
    assert!(shoot.curve("string", release - 0.05) > 0.99 && shoot.curve("string", release) < 0.01);
    assert!(shoot.curve("arrow", release - 0.05) > 0.99 && shoot.curve("arrow", release) < 0.01);
    for name in ["Draw_Hip", "Draw_Back", "Draw_Pole", "Draw_Bow", "Draw_Hip_L", "Draw_Side"] {
        let c = clip(name);
        let grab = c.event("grab").unwrap_or_else(|| panic!("{name} grabs"));
        assert!(c.curve("reach", grab) > 0.99, "{name}: the hand is on the item when it takes it");
        assert!(c.upper_body, "{name} plays over the legs");
    }
    for name in ["Sheathe_Hip", "Sheathe_Back", "Sheathe_Pole", "Sheathe_Bow"] {
        let c = clip(name);
        let stow = c.event("stow").unwrap_or_else(|| panic!("{name} stows"));
        assert!(c.curve("reach", stow) > 0.99, "{name}: the hand is at the item's place when it lets go");
    }
    for name in ["GreatSword_Idle_Loop", "Spear_Idle_Loop", "Staff_Slam"] {
        assert!(clip(name).curves.contains_key("off_hand"), "{name} holds with both hands");
    }
    let gear = cat.gear.as_ref().unwrap();
    for id in ["bow.hunting", "bow.long"] {
        let bow = gear.items.iter().find(|i| i.id == id).unwrap();
        let s = bow.string.as_ref().expect("bows describe their string");
        assert!(s.pull[2] < -0.3, "{id}: the string pulls back towards the archer (-z)");
    }
}

#[test]
fn the_checker_catches_broken_clip_promises() {
    let mut cat = assets::Catalog::load(root()).unwrap();
    let clips = &mut cat.humanoid.animations.clips;
    let i = clips.iter().position(|c| c.name == "Draw_Hip").unwrap();
    clips[i].curves.remove("reach");
    clips[i].events.push(assets::Event { t: 9.0, name: "teleport".into() });
    let gear = cat.gear.as_mut().unwrap();
    let great = gear.items.iter_mut().find(|i| i.id == "sword.great").unwrap();
    great.grips.clear();
    let bow = gear.items.iter_mut().find(|i| i.id == "bow.hunting").unwrap();
    bow.string = None;
    let problems: Vec<String> = cat.check().into_iter().map(|p| p.0).collect();
    assert!(problems.iter().any(|p| p.contains("Draw_Hip: a grab event without a reach curve")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("unknown event teleport")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("outside the clip")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("sword.great") && p.contains("off-hand grip")), "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("bow.hunting") && p.contains("bowstring")), "{problems:?}");
}

#[test]
fn foxfolk_have_fur_and_a_tail() {
    let cat = assets::Catalog::load(root()).unwrap();
    let h = &cat.humanoid;
    for frame in ["foxfolk_male", "foxfolk_female"] {
        let bt = h.body_types.get(frame).unwrap_or_else(|| panic!("no {frame}"));
        assert!(bt.markings.is_some(), "{frame} has fur markings");
        assert!(bt.springs.is_some(), "{frame}'s tail swings");
        assert!(bt.colliders.iter().any(|c| c.bone.starts_with("tail_")), "capes drape over {frame}'s tail");
    }
    let fur: Vec<_> = h.skin_tones.iter().filter(|t| t.races.iter().any(|r| r == "foxfolk")).collect();
    assert!(fur.len() >= 4, "a choice of fur");
    assert!(fur.iter().all(|t| t.light.is_some() && t.dark.is_some()), "every fur tone has light and dark fur");
    // Hats sit where foxfolk ears stand, so every head piece hides them.
    for p in h.pieces.iter().filter(|p| p.slot == "head") {
        let hides = &p.hides["foxfolk_male"]["body"];
        assert!(hides.iter().any(|r| r == "ears"), "{} hides foxfolk ears", p.id);
    }
}

#[test]
fn levels_of_detail_sit_beside_their_files() {
    assert_eq!(assets::lod_path("humanoid/male/body.gltf", 1), "humanoid/male/body.lod1.gltf");
    assert_eq!(assets::lod_path("gear/sword.arming.gltf", 2), "gear/sword.arming.lod2.gltf");
}
