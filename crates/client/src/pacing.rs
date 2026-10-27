//! Frame pacing (#223). Frames start on the display's refresh instead of a free-running timer,
//! and the world is drawn at a steady sim time instead of "however long since a snapshot
//! arrived", so the client's frames and the sim's 60 Hz steps stop beating against each other.
use std::collections::VecDeque;

/// How far behind the newest sim time the world is drawn: one step, so there is nearly always
/// a later snapshot to blend towards, plus a little for the time the sim takes to publish one.
pub const DRAW_DELAY: f64 = 1. / 60. + 0.002;

/// The sim's clock as seen by the client. The host says which instant each step was due, and
/// steps are due at exact intervals, so sim time minus that instant holds still while the sim
/// keeps time. It jumps when the sim drops a backlog, and the clock follows.
#[derive(Clone, Copy, Debug, Default)]
pub struct SimClock {
    offset: Option<f64>,
    /// The sim time last observed, so a step seen again doesn't count twice.
    last: f64,
}

impl SimClock {
    /// A step at `sim_time` was due at `due` (seconds on the client's own clock).
    ///
    /// Locally every step agrees, so the offset is exact. Over the network `due` is when the
    /// view arrived, which jitters, so each new step moves the offset only a little (#198); a
    /// jump of a quarter second or more (the sim dropped a backlog, the network stalled) is
    /// taken at once.
    pub fn observe(&mut self, sim_time: f64, due: f64) {
        if self.offset.is_some() && sim_time == self.last {
            return;
        }
        self.last = sim_time;
        let sample = sim_time - due;
        self.offset = Some(match self.offset {
            Some(o) if (sample - o).abs() < 0.25 => o + (sample - o) * 0.02,
            _ => sample,
        });
    }

    /// The sim time to draw at `local` seconds on the client's clock.
    pub fn draw_time(&self, local: f64) -> Option<f64> {
        self.offset.map(|o| local + o - DRAW_DELAY)
    }
}

/// Evenly spaced frame times. While frames come at the display's refresh, each frame's time is
/// the last one's plus exactly one refresh, so wake-up jitter doesn't move what's drawn; a late
/// or early frame (more than a third of a refresh off) starts the count again from the clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameClock {
    last: Option<f64>,
}

impl FrameClock {
    pub fn tick(&mut self, now: f64, period: Option<f64>) -> f64 {
        let t = match (self.last, period) {
            (Some(last), Some(p)) => {
                let steady = last + (((now - last) / p).round().max(1.)) * p;
                if (steady - now).abs() < p / 3. {
                    steady
                } else {
                    now
                }
            }
            _ => now,
        };
        self.last = Some(t);
        t
    }
}

/// What the F3 overlay shows about pacing: how even the last second of frames was, and how many
/// sim steps each frame showed.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    intervals: VecDeque<f64>,
    steps: VecDeque<u64>,
    /// Frames since the start that showed no new step, one, and more than one.
    pub total: [u64; 3],
    /// Squared errors of the sim time drawn per frame against the refreshes shown: this
    /// pacing, then timing by arrival as before; and how many frames.
    judder: (f64, f64, u64),
}

const WINDOW: usize = 120;

impl Stats {
    pub fn frame(&mut self, interval: f64, steps: u64) {
        if self.intervals.len() == WINDOW {
            self.intervals.pop_front();
            self.steps.pop_front();
        }
        self.intervals.push_back(interval);
        self.steps.push_back(steps);
        self.total[(steps as usize).min(2)] += 1;
    }

    pub fn judder(&mut self, paced: f64, by_arrival: f64) {
        self.judder.0 += paced * paced;
        self.judder.1 += by_arrival * by_arrival;
        self.judder.2 += 1;
    }

    /// Root mean square of those errors, in milliseconds: this pacing, and by arrival.
    pub fn judder_ms(&self) -> (f64, f64) {
        let n = self.judder.2.max(1) as f64;
        (
            (self.judder.0 / n).sqrt() * 1e3,
            (self.judder.1 / n).sqrt() * 1e3,
        )
    }

    /// Mean and standard deviation of frame intervals, in milliseconds.
    pub fn interval_ms(&self) -> (f64, f64) {
        let n = self.intervals.len().max(1) as f64;
        let mean = self.intervals.iter().sum::<f64>() / n;
        let var = self
            .intervals
            .iter()
            .map(|i| (i - mean).powi(2))
            .sum::<f64>()
            / n;
        (mean * 1e3, var.sqrt() * 1e3)
    }

    /// How many recent frames showed no new step, exactly one, and more than one.
    pub fn steps(&self) -> [usize; 3] {
        let mut out = [0; 3];
        for &s in &self.steps {
            out[(s as usize).min(2)] += 1;
        }
        out
    }
}

#[cfg(target_os = "macos")]
pub use display::DisplayLink;

#[cfg(target_os = "macos")]
#[allow(deprecated)] // CVDisplayLink still works; NSView's display link needs an Objective-C target.
mod display {
    use objc2_core_foundation::CFRetained;
    use objc2_core_video::{CVDisplayLink, CVOptionFlags, CVReturn, CVTimeStamp, kCVReturnSuccess};
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::atomic::{AtomicBool, Ordering};
    use winit::event_loop::EventLoopProxy;

    /// Wakes the event loop once per refresh of one display. A wake-up the loop hasn't taken
    /// yet isn't repeated, so a slow frame doesn't queue up a backlog of redraws.
    pub struct DisplayLink {
        link: CFRetained<CVDisplayLink>,
        state: *mut State,
        /// Seconds per refresh.
        pub period: f64,
    }

    struct State {
        proxy: EventLoopProxy<()>,
        pending: AtomicBool,
    }

    unsafe extern "C-unwind" fn tick(
        _: NonNull<CVDisplayLink>,
        _: NonNull<CVTimeStamp>,
        _: NonNull<CVTimeStamp>,
        _: CVOptionFlags,
        _: NonNull<CVOptionFlags>,
        user: *mut c_void,
    ) -> CVReturn {
        let state = unsafe { &*(user as *const State) };
        if !state.pending.swap(true, Ordering::AcqRel) {
            let _ = state.proxy.send_event(());
        }
        kCVReturnSuccess
    }

    impl DisplayLink {
        /// A link for the display `display` (a CGDirectDisplayID), or `None` if CoreVideo won't
        /// make one.
        pub fn start(proxy: EventLoopProxy<()>, display: u32) -> Option<DisplayLink> {
            let mut raw: *mut CVDisplayLink = std::ptr::null_mut();
            if unsafe { CVDisplayLink::create_with_active_cg_displays(NonNull::from(&mut raw)) }
                != kCVReturnSuccess
            {
                return None;
            }
            let link = unsafe { CFRetained::from_raw(NonNull::new(raw)?) };
            link.set_current_cg_display(display);
            let state = Box::into_raw(Box::new(State {
                proxy,
                pending: AtomicBool::new(false),
            }));
            unsafe { link.set_output_callback(Some(tick), state.cast()) };
            let period = link.actual_output_video_refresh_period();
            let nominal = link.nominal_output_video_refresh_period();
            let period = if period > 0. {
                period
            } else if nominal.timeValue > 0 {
                nominal.timeValue as f64 / nominal.timeScale as f64
            } else {
                1. / 60.
            };
            if link.start() != kCVReturnSuccess {
                unsafe { drop(Box::from_raw(state)) };
                return None;
            }
            Some(DisplayLink {
                link,
                state,
                period,
            })
        }

        /// The loop took the wake-up: the next refresh may send another.
        pub fn taken(&self) {
            unsafe { &*self.state }
                .pending
                .store(false, Ordering::Release);
        }
    }

    impl Drop for DisplayLink {
        fn drop(&mut self) {
            self.link.stop();
            unsafe { drop(Box::from_raw(self.state)) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_times_stay_even_through_wake_up_jitter() {
        let p = 1. / 60.;
        let mut c = FrameClock::default();
        let mut last = c.tick(10., Some(p));
        for (i, jitter) in [0.002, -0.003, 0.004, 0.0, -0.001].iter().enumerate() {
            let t = c.tick(10. + (i + 1) as f64 * p + jitter, Some(p));
            assert!(
                (t - last - p).abs() < 1e-9,
                "frame {i} moved by {}",
                t - last - p
            );
            last = t;
        }
        // A dropped frame counts two refreshes, still on the grid.
        let t = c.tick(last + 2. * p + 0.001, Some(p));
        assert!((t - last - 2. * p).abs() < 1e-9);
        // Far off the grid (the window was hidden): start again from the clock.
        assert_eq!(c.tick(t + 1.0071, Some(p)), t + 1.0071);
    }

    #[test]
    fn the_sim_is_drawn_a_step_behind_its_newest_time() {
        let mut s = SimClock::default();
        assert_eq!(s.draw_time(5.), None);
        // Step 600 (10 s of sim) was due at 4.5 s on the client's clock.
        s.observe(10., 4.5);
        let t = s.draw_time(4.5 + 0.004).unwrap();
        assert!((t - (10. + 0.004 - DRAW_DELAY)).abs() < 1e-9);
    }

    #[test]
    fn stats_count_steps_per_frame_and_spread() {
        let mut st = Stats::default();
        for i in 0..10 {
            st.frame(
                if i % 2 == 0 { 0.016 } else { 0.0174 },
                if i == 3 { 0 } else { 1 },
            );
        }
        assert_eq!(st.steps(), [1, 9, 0]);
        let (mean, sd) = st.interval_ms();
        assert!(
            (mean - 16.7).abs() < 1e-6 && (sd - 0.7).abs() < 1e-6,
            "{mean} {sd}"
        );
    }
}
