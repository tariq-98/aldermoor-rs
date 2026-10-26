//! Press-to-photon latency (#219): from a key going down to the moment the display shows the
//! first frame with its result (the jump, the roll or the skill going off).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Jump,
    Roll,
    Skill,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Jump, Kind::Roll, Kind::Skill];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Jump => "jump",
            Kind::Roll => "roll",
            Kind::Skill => "skill",
        }
    }
}

/// Presses that never show a result (a jump in mid-air, a skill on cooldown) are forgotten
/// after this long.
const FORGET: f64 = 1.;
const KEPT: usize = 200;

#[derive(Default)]
pub struct Latency {
    /// Presses waiting for their result: what (for a skill, which ability), and when on the
    /// media clock.
    pending: Vec<(Kind, Option<String>, f64)>,
    /// Results in the sim, waiting for a frame that draws the player at or past them: what,
    /// when pressed, and the sim time of the step that did it.
    stepped: Vec<(Kind, f64, f64)>,
    /// Results drawn, waiting for that frame to reach the screen: what, when pressed, which frame.
    drawn: Vec<(Kind, f64, usize)>,
    /// Press to photon, by kind, in milliseconds.
    samples: [Vec<f64>; 3],
    /// Press to the start of the frame that draws the result: what a locked or sleeping screen
    /// still lets us measure, since it never presents.
    to_frame: [Vec<f64>; 3],
}

fn keep(s: &mut Vec<f64>, ms: f64) {
    if s.len() == KEPT {
        s.remove(0);
    }
    s.push(ms);
}

fn spread(s: &[f64]) -> Option<(f64, f64, usize)> {
    let mut s = s.to_vec();
    if s.is_empty() {
        return None;
    }
    s.sort_by(f64::total_cmp);
    let n = s.len();
    Some((s[n / 2], s[(n * 95 / 100).min(n - 1)], n))
}

impl Latency {
    /// A key went down for `kind` (a skill names its ability, so a basic attack swinging on
    /// its own isn't taken for it).
    pub fn pressed(&mut self, kind: Kind, ability: Option<String>, at: f64) {
        self.pending.retain(|&(_, _, t)| at - t < FORGET);
        self.pending.push((kind, ability, at));
    }

    /// The step at `sim_time` did `kind` (using `ability`): the oldest matching press waits to
    /// be drawn.
    pub fn stepped(&mut self, kind: Kind, ability: Option<&str>, sim_time: f64) {
        let matches =
            |k: Kind, a: &Option<String>| k == kind && (a.is_none() || a.as_deref() == ability);
        if let Some(i) = self.pending.iter().position(|(k, a, _)| matches(*k, a)) {
            let (_, _, at) = self.pending.remove(i);
            self.stepped.push((kind, at, sim_time));
        }
    }

    /// `frame`, started at `now` on the media clock, draws the player at `drawn` sim time:
    /// results up to then are in it.
    pub fn drew(&mut self, frame: usize, drawn: f64, now: f64) {
        let (drawn_list, to_frame) = (&mut self.drawn, &mut self.to_frame);
        self.stepped.retain(|&(kind, at, t)| {
            let shown = t <= drawn + 1e-6;
            if shown {
                drawn_list.push((kind, at, frame));
                keep(&mut to_frame[kind as usize], (now - at) * 1e3);
            }
            !shown
        });
    }

    /// Takes the frames that have reached the screen, by `presented(frame)`'s media-clock time.
    pub fn presented(&mut self, presented: impl Fn(usize) -> Option<f64>) {
        let samples = &mut self.samples;
        self.drawn
            .retain(|&(kind, at, frame)| match presented(frame) {
                Some(shown) => {
                    keep(&mut samples[kind as usize], (shown - at) * 1e3);
                    false
                }
                None => true,
            });
        // A frame that never presents (offscreen, minimised) mustn't hold presses forever.
        if self.drawn.len() > 64 {
            self.drawn.remove(0);
        }
    }

    /// Median, 95th percentile and count of `kind`'s press-to-photon times, in milliseconds.
    pub fn summary(&self, kind: Kind) -> Option<(f64, f64, usize)> {
        spread(&self.samples[kind as usize])
    }

    /// The same for press to the start of the frame that draws the result.
    pub fn to_frame(&self, kind: Kind) -> Option<(f64, f64, usize)> {
        spread(&self.to_frame[kind as usize])
    }
}

/// Now on the media clock, which presented times are measured on.
pub fn now() -> f64 {
    objc2_quartz_core::CACurrentMediaTime()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_is_timed_to_the_frame_that_shows_it() {
        let mut l = Latency::default();
        l.pressed(Kind::Jump, None, 10.);
        l.pressed(Kind::Roll, None, 10.01);
        l.stepped(Kind::Roll, None, 1.0);
        l.stepped(Kind::Jump, None, 1.1);
        // Frame 7 draws the player at 1.05 s: the roll is in it, the jump isn't yet.
        l.drew(7, 1.05, 10.03);
        l.drew(8, 1.1, 10.05);
        // Frame 7 is on screen; frame 8 isn't yet.
        l.presented(|f| (f == 7).then_some(10.06));
        assert!((l.summary(Kind::Roll).unwrap().0 - 50.).abs() < 1e-6);
        assert!((l.to_frame(Kind::Roll).unwrap().0 - 20.).abs() < 1e-6);
        assert!(l.summary(Kind::Jump).is_none());
        l.presented(|f| (f == 8).then_some(10.075));
        assert!((l.summary(Kind::Jump).unwrap().0 - 75.).abs() < 1e-6);
    }

    #[test]
    fn presses_without_a_result_are_forgotten() {
        let mut l = Latency::default();
        l.pressed(Kind::Skill, Some("rend".into()), 1.);
        l.pressed(Kind::Skill, Some("rend".into()), 3.);
        // A basic attack going off isn't the skill that was pressed.
        l.stepped(Kind::Skill, Some("chain"), 2.);
        l.stepped(Kind::Skill, Some("rend"), 2.);
        l.drew(1, 2., 3.02);
        l.presented(|_| Some(3.05));
        // Only the second press was still waiting, so the result is timed from it.
        assert!((l.summary(Kind::Skill).unwrap().0 - 50.).abs() < 1e-6);
    }
}
