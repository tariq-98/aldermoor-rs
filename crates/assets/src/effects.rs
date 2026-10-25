//! How combat looks (`assets/catalog/effects.json`): glows for each school and field, the
//! missiles in flight, the glow in a caster's hands, auras on units and marks on the ground.
//!
//! Effects are drawn from simple shapes (spheres, streaks, discs and rings) lit by their own
//! glow, so every look value lives here and the client only supplies the geometry.

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
pub struct EffectCatalog {
    pub version: u32,
    /// Named glows. Schools use their lowercase names (`"fire"`), combo fields `"field.<name>"`,
    /// ground marks `"hostile"` and `"friendly"`, a combo's burst `"combo"`, and a thrown
    /// axe's haft `"wood"`.
    pub glows: BTreeMap<String, Glow>,
    /// By `ProjectileView::visual`.
    pub projectiles: BTreeMap<String, ProjectileLook>,
    /// Abilities whose cast or channel glows in the caster's hands, and the glow it takes.
    /// Wind-ups for heavy blows aren't listed: the swing shows them.
    pub casts: BTreeMap<String, String>,
    /// By aura key. Auras that aren't listed show only on the HUD.
    pub auras: BTreeMap<String, AuraLook>,
    pub style: EffectStyle,
}

/// Light that an effect gives off. Effects are blended, unshadowed and lit mostly by this.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Glow {
    /// Linear RGB.
    pub colour: [f32; 3],
    /// Emission is `colour × strength`; 0 for things that don't glow (an arrow's shaft).
    pub strength: f32,
    /// The surface's own colour is `colour × albedo`, lit by the sun like anything else.
    #[serde(default = "dim")]
    pub albedo: f32,
}

fn dim() -> f32 {
    0.15
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectileKind {
    /// A ball of light with a halo and a tapering trail.
    Bolt,
    /// A long shard pointing along its flight, with a trail.
    Lance,
    /// A shaft along its flight with a faint streak behind.
    Arrow,
    /// A flat head that tumbles end over end.
    Axe,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectileLook {
    pub kind: ProjectileKind,
    /// The bright part: a bolt's core, a lance's shard, an arrow's shaft or an axe's head.
    pub glow: String,
    /// The halo and trail, if different.
    #[serde(default)]
    pub trail_glow: Option<String>,
    /// Core radius (metres); an arrow's or lance's thickness.
    pub size: f32,
    /// Shaft or shard length (arrows, lances, axes).
    #[serde(default)]
    pub length: f32,
    /// Halo radius around a bolt (0 for none).
    #[serde(default)]
    pub halo: f32,
    /// Trail length behind it (metres, 0 for none) and its radius where it leaves the core.
    #[serde(default)]
    pub trail: f32,
    #[serde(default)]
    pub trail_width: f32,
    /// Turns a second (axes).
    #[serde(default)]
    pub spin: f32,
    /// The flash where it stops: radius in metres.
    pub burst: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuraKind {
    /// A see-through bubble around the body (wards, shields, invulnerability).
    Shell,
    /// A ring on the ground at the feet (buffs, conditions).
    Ring,
    /// A small spinning ring above the head (stuns, fear, sleep).
    Halo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuraLook {
    pub kind: AuraKind,
    pub glow: String,
    /// A shell's radius per metre of the unit's height, or a ring's radius in metres.
    pub size: f32,
}

/// Opacity, sizes and timings shared by every effect.
#[derive(Debug, Clone, Deserialize)]
pub struct EffectStyle {
    pub core_alpha: f32,
    pub halo_alpha: f32,
    pub trail_alpha: f32,
    pub shell_alpha: f32,
    pub ring_alpha: f32,
    /// A ring's band, as a share of its radius.
    pub ring_width: f32,
    pub halo_ring_alpha: f32,
    /// A warning's whole shape, faint, and the part already filled in.
    pub warning_outline_alpha: f32,
    pub warning_fill_alpha: f32,
    /// A lasting field's fill and its rim.
    pub field_alpha: f32,
    pub field_rim_alpha: f32,
    /// The rim's width in metres.
    pub rim_width: f32,
    /// The orb in a caster's hands: radius when a cast begins and when it completes.
    pub cast_orb: [f32; 2],
    pub cast_orb_alpha: f32,
    /// The circle at a caster's feet: radius and opacity.
    pub cast_circle: f32,
    pub cast_circle_alpha: f32,
    /// How long a missile's flash and a landed blow's burst last (seconds).
    pub burst_seconds: f32,
    pub burst_alpha: f32,
    /// Seconds an aura takes to fade in and out.
    pub aura_fade: f32,
}

impl EffectCatalog {
    pub fn glow(&self, name: &str) -> Option<&Glow> {
        self.glows.get(name)
    }

    /// The glow for a school (`protocol::School`'s name, any case).
    pub fn school(&self, school: &str) -> Option<&Glow> {
        self.glows.get(&school.to_lowercase())
    }

    /// Everything wrong with the effects table.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (name, g) in &self.glows {
            if !(g
                .colour
                .iter()
                .chain([&g.strength, &g.albedo])
                .all(|v| v.is_finite() && *v >= 0.0))
            {
                out.push(format!(
                    "effects: glow {name} has a negative or non-finite value"
                ));
            }
        }
        let mut named = |what: String, glow: &str| {
            if !self.glows.contains_key(glow) {
                out.push(format!(
                    "effects: {what} names glow {glow}, which isn't defined"
                ));
            }
        };
        for (id, p) in &self.projectiles {
            named(format!("projectile {id}"), &p.glow);
            if let Some(t) = &p.trail_glow {
                named(format!("projectile {id} trail"), t);
            }
        }
        for (ability, glow) in &self.casts {
            named(format!("cast {ability}"), glow);
        }
        for (key, a) in &self.auras {
            named(format!("aura {key}"), &a.glow);
        }
        for glow in ["hostile", "friendly", "combo", "wood"] {
            named("the client".into(), glow);
        }
        for school in [
            "physical", "fire", "frost", "arcane", "nature", "holy", "shadow",
        ] {
            named("school".into(), school);
        }
        let mut sizes: Vec<(String, f32)> = Vec::new();
        for (id, p) in &self.projectiles {
            sizes.extend([
                (format!("projectile {id} size"), p.size),
                (format!("projectile {id} burst"), p.burst),
                (format!("projectile {id} length"), p.length),
                (format!("projectile {id} trail"), p.trail),
            ]);
            if p.size <= 0.0 {
                out.push(format!("effects: projectile {id} has no size"));
            }
        }
        for (key, a) in &self.auras {
            sizes.push((format!("aura {key} size"), a.size));
        }
        let s = &self.style;
        for (what, alpha) in [
            ("core_alpha", s.core_alpha),
            ("halo_alpha", s.halo_alpha),
            ("trail_alpha", s.trail_alpha),
            ("shell_alpha", s.shell_alpha),
            ("ring_alpha", s.ring_alpha),
            ("halo_ring_alpha", s.halo_ring_alpha),
            ("warning_outline_alpha", s.warning_outline_alpha),
            ("warning_fill_alpha", s.warning_fill_alpha),
            ("field_alpha", s.field_alpha),
            ("field_rim_alpha", s.field_rim_alpha),
            ("cast_orb_alpha", s.cast_orb_alpha),
            ("cast_circle_alpha", s.cast_circle_alpha),
            ("burst_alpha", s.burst_alpha),
        ] {
            if !(0.0..=1.0).contains(&alpha) {
                out.push(format!("effects: style {what} must be 0…1"));
            }
        }
        if !(0.0..1.0).contains(&s.ring_width) {
            out.push("effects: style ring_width must be below 1".into());
        }
        sizes.extend([
            ("style rim_width".into(), s.rim_width),
            ("style cast_circle".into(), s.cast_circle),
            ("style burst_seconds".into(), s.burst_seconds),
            ("style aura_fade".into(), s.aura_fade),
            ("style cast_orb".into(), s.cast_orb[0]),
            ("style cast_orb".into(), s.cast_orb[1]),
        ]);
        for (what, v) in sizes {
            if !v.is_finite() || v < 0.0 {
                out.push(format!("effects: {what} must be a positive number"));
            }
        }
        out
    }
}
