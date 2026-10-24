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
