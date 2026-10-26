//! Hits that read (#218): each blow tips the body away from it, over whatever the body is doing,
//! and holds both bodies' poses for a moment as it lands, the hitstop that action games sell an
//! impact with (Monster Hunter, God of War).
//!
//! [`Flinch::hit`] kicks a critically damped spring on the chest's tilt the way the blow
//! travelled, as far as it was hard, while the clips underneath (a jog, a swing) play on. The head
//! follows the chest on a spring of its own, so it lags as the chest tips and whips past as it
//! comes back; heavy blows tip the pelvis too, about the ground beneath it. Staggers and
//! knockbacks aren't flinches: the character plays them as clips, in full.
//!
//! The hitstop holds the pose clocks for the blow's `hitstop_ms`: [`Flinch::update`] says how far
//! they move on each frame, not at all while held, and [`Flinch::behind`] how far behind the sim's
//! clocks that leaves them, until the clips next switch and the blender hides the catch-up. The
//! sim never pauses, so the body itself moves on, and while held the one who was hit shakes along
//! the blow.
use crate::locomotion::Spring;
use glam::Vec2;
use std::f32::consts::{E, LN_2, TAU};

/// How far blows tip the chest, by how hard they were, in radians: 10° for a light one, 16° for a
/// heavy one, 18° for a critical and 23° for a monster's big slam; 5° when a guard stopped it and
/// 2° when it was parried.
pub const LIGHT: f32 = 0.18;
pub const HEAVY: f32 = 0.28;
pub const CRITICAL: f32 = 0.32;
pub const BIG: f32 = 0.4;
pub const BLOCKED: f32 = 0.08;
pub const PARRIED: f32 = 0.04;
/// How far heavy blows tip the pelvis, against how far they tip the chest.
pub const PELVIS: f32 = 0.3;
/// The furthest blows landing together tip the chest, in radians (32°).
pub const MOST: f32 = 0.55;
/// Half-lives in seconds: of the chest's spring, which kicked tips furthest after 0.72 of one
/// (about 60 ms) and is back within a tenth of that after four; and of the head following.
pub const HALF_LIFE: f32 = 0.085;
pub const HEAD_HALF_LIFE: f32 = 0.06;
/// How the chest's tilt spreads up the spine, from the bottom: it bends most at the top.
pub const SPINE: [f32; 3] = [0.25, 0.35, 0.4];
/// The shake while the hitstop holds: how far, against how far the blow tips the chest, and how
/// many times a second.
pub const SHAKE: f32 = 0.2;
pub const SHAKE_RATE: f32 = 17.;

/// A blow, as the body it lands on takes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Blow {
    /// The way it travelled over the ground, in the body's frame: toward +z (from behind) or +x.
    /// The body tips this way. Only its direction counts.
    pub way: Vec2,
    /// How far it tips the chest, in radians: [`LIGHT`] to [`BIG`], [`BLOCKED`] or [`PARRIED`].
    pub size: f32,
    /// Whether it tips the pelvis too, as heavy blows do.
    pub heavy: bool,
}

/// How the flinch bends the body on one frame, in the model's frame: how far each bone tips on
/// top of its parent, toward +x and toward +z, in radians. The pelvis tips about the ground
/// beneath it, taking the whole body with it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bend {
    pub pelvis: Vec2,
    /// The spine's three bones, from the bottom.
    pub spine: [Vec2; 3],
    pub neck: Vec2,
    pub head: Vec2,
}

/// One body's flinches and hitstop.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flinch {
    /// How far the chest is tipped on the pelvis, the pelvis from upright, and the head from
    /// upright as it follows the two.
    chest: Spring<Vec2>,
    pelvis: Spring<Vec2>,
    head: Spring<Vec2>,
    /// The hitstop: seconds it still holds, how long it holds in all, and how long it has held.
    hold: f32,
    span: f32,
    held: f32,
    /// Which way and how far the one who was hit shakes while it holds.
    shake: Vec2,
    /// How far behind the sim's clocks the pose clocks are, in seconds.
    behind: f32,
}

impl Flinch {
    /// A blow lands on this body: it tips away from it, and its pose holds for `hitstop` seconds.
    pub fn hit(&mut self, blow: Blow, hitstop: f32) {
        let way = blow.way.normalize_or_zero();
        // A critically damped spring at rest, kicked to a speed v, goes out to v / (rate e) and
        // comes back.
        let kick = way * blow.size * 2. * LN_2 / HALF_LIFE * E;
        self.chest.velocity += kick;
        if blow.heavy {
            self.pelvis.velocity += kick * PELVIS;
        }
        self.hold(hitstop);
        if self.hold > 0. {
            self.shake = way * blow.size * SHAKE;
        }
    }

    /// This body's own blow landed: its pose holds for `hitstop` seconds.
    pub fn hold(&mut self, hitstop: f32) {
        if hitstop > self.hold {
            self.hold = hitstop;
            self.span = hitstop;
            self.held = 0.;
        }
    }

    /// One frame `dt` seconds long. Returns how far the pose clocks move on (not at all while the
    /// hitstop holds), and how the flinch bends the body.
    pub fn update(&mut self, dt: f32) -> (f32, Bend) {
        let held = dt.min(self.hold);
        self.hold -= held;
        self.held += held;
        self.behind += held;
        let on = dt - held;
        if self.hold <= 0. {
            self.shake = Vec2::ZERO;
        }
        if self.settled() {
            return (on, Bend::default());
        }
        // The springs run on the pose clock, so they hold with it.
        settle(&mut self.chest, Vec2::ZERO, HALF_LIFE, on, MOST);
        settle(&mut self.pelvis, Vec2::ZERO, HALF_LIFE, on, MOST * PELVIS);
        let upper = self.chest.value + self.pelvis.value;
        settle(&mut self.head, upper, HEAD_HALF_LIFE, on, f32::INFINITY);
        // Strongest as the blow lands, fading out as the hold lets go.
        let shake = if self.hold > 0. {
            self.shake * (TAU * SHAKE_RATE * self.held).sin() * (self.hold / self.span)
        } else {
            Vec2::ZERO
        };
        let chest = self.chest.value;
        let lag = (self.head.value - upper) / 2.;
        let bend = Bend {
            pelvis: self.pelvis.value,
            spine: SPINE.map(|share| chest * share),
            neck: lag,
            head: lag,
        };
        (
            on,
            Bend {
                spine: [bend.spine[0] + shake, bend.spine[1], bend.spine[2]],
                ..bend
            },
        )
    }

    /// How far behind the sim's clocks the hitstop has left the pose clocks, in seconds: to take
    /// off the times that clips driven by the sim's clocks are sampled at.
    pub fn behind(&self) -> f32 {
        self.behind
    }

    /// The clips switched, and the blender smooths whatever jumped: unless the hitstop still
    /// holds, the pose clocks catch up with the sim's.
    pub fn switched(&mut self) {
        if self.hold <= 0. {
            self.behind = 0.;
        }
    }

    /// Whether the hitstop holds the pose clocks.
    pub fn holding(&self) -> bool {
        self.hold > 0.
    }

    /// Whether it bends the body no way at all, and won't until the next blow.
    pub fn settled(&self) -> bool {
        self.hold <= 0.
            && self.chest == Spring::default()
            && self.pelvis == Spring::default()
            && self.head == Spring::default()
    }
}

/// Moves `spring` toward `target` over `dt` seconds, no further than `most` from upright (losing
/// the speed that would take it further), and once it is there, stops it on it.
fn settle(spring: &mut Spring<Vec2>, target: Vec2, half_life: f32, dt: f32, most: f32) {
    if *spring == Spring::default() && target == Vec2::ZERO {
        return;
    }
    spring.update(target, half_life, dt);
    let out = spring.value.length();
    if out > most {
        let n = spring.value / out;
        spring.value = n * most;
        spring.velocity -= n * spring.velocity.dot(n).max(0.);
    }
    if spring.value.distance_squared(target) < 1e-10 && spring.velocity.length() < 1e-4 {
        spring.snap(target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1. / 60.;

    /// How far the chest tips from the pelvis: what the spine's bones add up to.
    fn chest(b: &Bend) -> Vec2 {
        b.spine.iter().sum()
    }

    /// Takes `blow` (with no hitstop) on a body at rest, and returns each frame's bend after.
    fn flinch(blow: Blow, frames: usize) -> Vec<Bend> {
        let mut f = Flinch::default();
        f.hit(blow, 0.);
        (0..frames).map(|_| f.update(DT).1).collect()
    }

    /// The most a blow of `size` moves the chest in a frame: the speed it is kicked to. It moves
    /// that fast only as the blow lands; everything after is slower.
    fn fastest(size: f32) -> f32 {
        size * 2. * LN_2 / HALF_LIFE * E * DT
    }

    /// The furthest any bend tips `pick` of it, and the frame it does.
    fn furthest(bends: &[Bend], pick: impl Fn(&Bend) -> Vec2) -> (Vec2, usize) {
        let mut most = (Vec2::ZERO, 0);
        for (i, b) in bends.iter().enumerate() {
            if pick(b).length() > most.0.length() {
                most = (pick(b), i);
            }
        }
        most
    }

    #[test]
    fn a_blow_tips_the_body_its_way_as_far_as_it_was_hard() {
        let ways = [Vec2::Y, -Vec2::Y, Vec2::X, Vec2::new(-1., 2.).normalize()];
        for way in ways {
            for (size, heavy) in [(LIGHT, false), (HEAVY, true), (BIG, true), (BLOCKED, false)] {
                let bends = flinch(Blow { way, size, heavy }, 60);
                let (tipped, frame) = furthest(&bends, chest);
                // The way it went, as far as it was hard, about 60 ms after it landed.
                assert!(
                    tipped.normalize().dot(way) > 0.9999,
                    "{way} tipped {tipped}"
                );
                assert!(
                    (tipped.length() - size).abs() < 0.02 * size,
                    "{size} tipped {}",
                    tipped.length()
                );
                let peak = 0.72 * HALF_LIFE;
                assert!(
                    ((frame + 1) as f32 * DT - peak).abs() <= DT,
                    "furthest on frame {frame}"
                );
                // Heavy blows tip the pelvis that way too; light ones leave it.
                let (pelvis, _) = furthest(&bends, |b| b.pelvis);
                if heavy {
                    assert!(pelvis.normalize().dot(way) > 0.9999);
                    assert!((pelvis.length() - PELVIS * size).abs() < 0.02 * size);
                } else {
                    assert_eq!(pelvis, Vec2::ZERO);
                }
                // The spine bends most at the top.
                let b = bends[frame];
                assert!(b.spine[0].length() < b.spine[1].length());
                assert!(b.spine[1].length() < b.spine[2].length());
            }
        }
    }

    #[test]
    fn a_flinch_comes_back_within_four_half_lives_without_a_jump() {
        let blow = Blow {
            way: Vec2::new(1., 1.),
            size: BIG,
            heavy: true,
        };
        let bends = flinch(blow, 120);
        let (tipped, _) = furthest(&bends, chest);
        // Back within a tenth after four half-lives, and on to nothing at all.
        let after = (4. * HALF_LIFE / DT).ceil() as usize;
        for b in &bends[after..] {
            assert!(chest(b).length() < 0.1 * tipped.length());
            assert!(b.pelvis.length() < 0.1 * PELVIS * tipped.length());
        }
        assert_eq!(*bends.last().unwrap(), Bend::default());
        // No frame jumps: nothing moves faster than the blow kicked the chest.
        let mut last = Bend::default();
        for b in &bends {
            let moved = [
                (b.pelvis, last.pelvis),
                (chest(b), chest(&last)),
                (b.neck, last.neck),
            ];
            for (now, was) in moved {
                assert!(now.distance(was) <= fastest(BIG), "{was} to {now}");
            }
            last = *b;
        }
        // And settled, it costs nothing.
        let mut f = Flinch::default();
        f.hit(blow, 0.);
        for _ in 0..120 {
            f.update(DT);
        }
        assert!(f.settled());
    }

    #[test]
    fn the_head_lags_the_chest_then_whips_past_it() {
        let way = Vec2::Y;
        let bends = flinch(
            Blow {
                way,
                size: HEAVY,
                heavy: false,
            },
            40,
        );
        let (_, peak) = furthest(&bends, chest);
        // As the chest tips, the head holds back, turning against it on the neck...
        assert!(
            bends[..peak]
                .iter()
                .all(|b| (b.neck + b.head).dot(way) < 0.)
        );
        // ...and as it comes back, the head carries on past it.
        let (whip, frame) = furthest(&bends, |b| b.neck + b.head);
        let (past, _) = furthest(&bends[peak + 2..], |b| b.neck + b.head);
        assert!(frame < peak && whip.dot(way) < 0.);
        assert!(past.dot(way) > 0.1 * whip.length());
    }

    #[test]
    fn the_hitstop_holds_the_pose_clock_and_lets_go_without_a_jump() {
        let blow = Blow {
            way: -Vec2::Y,
            size: HEAVY,
            heavy: true,
        };
        let hitstop = 0.08;
        let mut f = Flinch::default();
        // Two frames before the blow; nothing is held, nothing behind.
        for _ in 0..2 {
            assert_eq!(f.update(DT), (DT, Bend::default()));
        }
        f.hit(blow, hitstop);
        assert!(f.holding());
        let mut clock = 0.;
        let mut frames = vec![];
        for _ in 0..30 {
            let (on, bend) = f.update(DT);
            clock += on;
            frames.push((on, bend, f.holding()));
        }
        // The clock stands still for the hitstop, then runs at once at its pace: it is behind by
        // just the hitstop, and moves on the rest of the frame the hold ends on.
        let held = (hitstop / DT) as usize;
        assert!(frames[..held].iter().all(|(on, ..)| *on == 0.));
        assert!((frames[held].0 - (DT - (hitstop - held as f32 * DT))).abs() < 1e-6);
        assert!(frames[held + 1..].iter().all(|(on, ..)| *on == DT));
        assert!((clock - (30. * DT - hitstop)).abs() < 1e-5);
        assert!((f.behind() - hitstop).abs() < 1e-6);
        // Held, the body doesn't tip: it only shakes along the blow, a little, and less as the
        // hold lets go.
        for (on, bend, holding) in &frames[..held] {
            assert!(*on == 0. && *holding);
            assert!(chest(bend).distance(bend.spine[0]) < 1e-6);
            assert!(bend.spine[0].length() <= SHAKE * HEAVY + 1e-6);
            assert!(bend.spine[0].perp_dot(blow.way).abs() < 1e-6);
            assert_eq!(bend.pelvis, Vec2::ZERO);
        }
        // Then it tips from where it held, with no jump: the shake gone, and no faster than the
        // blow's kick.
        assert!(frames[held - 1].1.spine[0].length() < 0.2 * SHAKE * HEAVY);
        let mut last = frames[held - 1].1;
        for (_, bend, _) in &frames[held..] {
            let step = chest(bend).distance(chest(&last));
            assert!(step <= fastest(HEAVY) + 1e-6, "jumped {step}");
            last = *bend;
        }
        let (tipped, _) = furthest(&frames.iter().map(|f| f.1).collect::<Vec<_>>(), chest);
        assert!(tipped.normalize().dot(blow.way) > 0.999);
        assert!((tipped.length() - HEAVY).abs() < 0.03 * HEAVY);
        // The clips catch up when they next switch; one switching while it held stays behind.
        let mut g = Flinch::default();
        g.hold(hitstop);
        g.update(DT);
        g.switched();
        assert!((g.behind() - DT).abs() < 1e-6);
        f.switched();
        assert_eq!(f.behind(), 0.);
        // A shorter hold doesn't cut a longer one short; a longer one holds on.
        let mut h = Flinch::default();
        h.hold(0.1);
        h.update(DT);
        h.hold(0.04);
        assert!((h.hold - (0.1 - DT)).abs() < 1e-6);
        h.hold(0.12);
        assert_eq!(h.hold, 0.12);
    }

    #[test]
    fn blows_landing_together_tip_no_further_than_the_most() {
        let mut f = Flinch::default();
        for _ in 0..4 {
            f.hit(
                Blow {
                    way: Vec2::X,
                    size: BIG,
                    heavy: true,
                },
                0.,
            );
        }
        let bends: Vec<_> = (0..60).map(|_| f.update(DT).1).collect();
        let (tipped, _) = furthest(&bends, chest);
        let (pelvis, _) = furthest(&bends, |b| b.pelvis);
        assert!((tipped.length() - MOST).abs() < 1e-4);
        assert!(pelvis.length() <= MOST * PELVIS + 1e-4);
        assert!(f.settled() || chest(bends.last().unwrap()).length() < 0.01);
    }
}
