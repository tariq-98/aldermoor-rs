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
    looks.extend(sim::content::catalog().mobs.iter().map(|m| (m.look)()));
    // Every specialty, with each of its weapon sets in hand.
    let mut sim = Sim::new(1);
    for s in sim::Specialty::ALL {
        let id = sim.spawn_hero("Hero", s, 1, Vec2::new(5.0, 10.0), 0.0);
        looks.push(sim.get::<sim::Appearance>(id).unwrap().0.clone());
        sim.get_mut::<sim::Fighter>(id).unwrap().swap_ready_at = 0.0;
        sim.command(id, protocol::Command::SwapWeapons);
        sim.step();
        looks.push(sim.get::<sim::Appearance>(id).unwrap().0.clone());
    }
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
        for (piece, main, trim) in &look.dyes {
            let worn_item = look.gear.iter().any(|(_, id)| id == piece);
            assert!(look.pieces.contains(piece) || worn_item, "a dye for {piece}, which isn't worn");
            let dyeable = cat.humanoid.pieces.iter().find(|x| &x.id == piece).map(|d| d.dye.is_some())
                .or_else(|| cat.gear.as_ref()?.items.iter().find(|i| &i.id == piece).map(|i| i.dye.is_some()));
            assert_eq!(dyeable, Some(true), "{piece} can't be dyed");
            for d in [main, trim].into_iter().flatten() {
                assert!(cat.humanoid.dyes.iter().any(|x| &x.id == d), "no dye {d}");
            }
        }
        for (piece, variant) in &look.variants {
            assert!(look.pieces.contains(piece), "a colourway for {piece}, which isn't worn");
            assert!(cat.humanoid.texture_variants.values().any(|v| v.contains(variant)), "no colourway {variant}");
        }
    }
}

#[test]
fn every_missile_cast_and_aura_effect_names_real_content() {
    let cat = catalog();
    let fx = cat.effects.as_ref().expect("assets/catalog/effects.json");
    let content = sim::content::catalog();
    for a in &content.abilities {
        if let Some(m) = &a.missile {
            assert!(fx.projectiles.contains_key(m.visual), "{} flies a {}, which has no look", a.key, m.visual);
        }
        // A spell's cast glows in the hands; a heavy blow's wind-up shows in the swing.
        if (a.cast > 0.0 || a.channel > 0.0) && a.school != protocol::School::Physical {
            assert!(fx.casts.contains_key(a.key), "{} is cast but its hands don't glow", a.key);
        }
    }
    for key in fx.casts.keys() {
        assert!(content.abilities.iter().any(|a| a.key == key), "effects name a cast {key} that isn't an ability");
    }
    for key in fx.auras.keys() {
        assert!(content.auras.iter().any(|a| a.key == key), "effects name an aura {key} that doesn't exist");
    }
}
