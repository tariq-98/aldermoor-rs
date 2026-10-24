//! Units carry looks the renderer can assemble, and every name in them exists in the art.

use glam::Vec2;
use sim::Sim;
use std::path::PathBuf;

fn catalog() -> assets::Catalog {
    assets::Catalog::load(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")).expect("art catalogue")
}

#[test]
fn snapshots_say_what_to_draw() {
    let mut sim = Sim::brambleford(3);
    let hero = sim.spawn_player("Hero", Vec2::new(5.0, 10.0), 0.0);
    sim.step();
    let snap = sim.snapshot(Some(hero));
    let me = snap.unit(hero).unwrap();
    assert_eq!(me.look.art, "male");
    assert!(me.look.gear.iter().any(|(slot, id)| slot == "main_hand" && id == "sword.arming"));
    assert!(!me.look.drawn, "weapons stay put away out of combat");
    let arts: Vec<&str> = snap.units.iter().map(|u| u.look.art.as_str()).collect();
    assert!(arts.contains(&"wolf.grey") && arts.contains(&"goblin_male"), "{arts:?}");
}

#[test]
fn every_look_names_real_art() {
    let cat = catalog();
    let gear: Vec<String> = cat.gear.as_ref().map(|g| g.items.iter().map(|i| i.id.clone()).collect()).unwrap_or_default();
    let creatures: Vec<String> = cat.creatures.as_ref().map(|c| c.creatures.iter().map(|c| c.id.clone()).collect()).unwrap_or_default();
    let mut looks = vec![sim::player_look()];
    looks.extend([sim::MonsterKind::Wolf, sim::MonsterKind::Goblin].map(|k| k.look()));
    for look in looks {
        let body = cat.humanoid.body_types.get(&look.art);
        assert!(body.is_some() || creatures.contains(&look.art), "no art called {}", look.art);
        for p in &look.pieces {
            let piece = cat.humanoid.pieces.iter().find(|x| &x.id == p).unwrap_or_else(|| panic!("no piece {p}"));
            assert!(piece.files.contains_key(&look.art), "{p} has no fit for {}", look.art);
        }
        for (_, item) in &look.gear {
            assert!(gear.contains(item), "no gear {item}");
        }
        for (kind, id) in [("hair", &look.hair), ("brows", &look.brows), ("beard", &look.beard)] {
            if let Some(id) = id {
                let h = cat.humanoid.hair.iter().find(|h| &h.id == id).unwrap_or_else(|| panic!("no {kind} {id}"));
                assert!(h.files.contains_key(&look.art), "{id} has no fit for {}", look.art);
            }
        }
        if let Some(t) = &look.tone {
            assert!(cat.humanoid.skin_tones.iter().any(|x| &x.id == t), "no tone {t}");
        }
    }
}
