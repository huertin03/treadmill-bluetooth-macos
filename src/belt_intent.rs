//! Daemon-side intent memory for relative belt commands (задача 063).
//!
//! Rapid `tm speed up` / `tm toggle` presses each spawn a short-lived CLI
//! process; only the daemon sees both live telemetry and what it has just
//! commanded. This module remembers the last *target speed* written and the
//! last *start/stop* issued (each with a timestamp) so relative intents can
//! be resolved at execute time.
//!
//! Pure: no BLE, no wall clock. Time is always injected as [`Instant`].

use std::time::{Duration, Instant};

use crate::control_command::StepDirection;
use crate::speed::CentiKmh;

/// Relative step applied by `speed_step:up` / `speed_step:down` (0.1 km/h).
pub const SPEED_STEP: CentiKmh = CentiKmh::from_wire(10);

/// Device-supported minimum target (Supported Speed Range `0x2AD4`).
pub const SPEED_MIN: CentiKmh = CentiKmh::from_wire(50);

/// Device-supported maximum target (6.10 km/h).
pub const SPEED_MAX: CentiKmh = CentiKmh::from_wire(610);

/// How long a recorded target speed or start/stop remains the resolution base.
pub const INTENT_WINDOW: Duration = Duration::from_secs(5);

/// Last CLI-issued start or stop the daemon actually wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunIntent {
    Start,
    Stop,
}

/// Why a relative speed step must not become a Control Point write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepRefuse {
    /// No fresh target and no live telemetry speed.
    UnknownBase,
    /// Base speed is zero — the belt is stopped. Only `toggle` may start it.
    BeltStopped,
    /// A Stop was issued inside [`INTENT_WINDOW`]; the belt is decelerating.
    RecentStop,
}

impl StepRefuse {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownBase => "belt speed is unknown",
            Self::BeltStopped => "belt is stopped",
            Self::RecentStop => "belt is stopping",
        }
    }
}

/// Outcome of [`BeltIntent::resolve_toggle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToggleOutcome {
    pub run: RunIntent,
    /// Live speed was unknown and no recent start/stop applied — we chose
    /// Stop on the safe side. Caller logs a WARN.
    pub unknown_live: bool,
}

/// Last commanded target speed and start/stop, each timestamped.
#[derive(Debug, Default, Clone)]
pub struct BeltIntent {
    last_target: Option<(CentiKmh, Instant)>,
    last_run: Option<(RunIntent, Instant)>,
}

impl BeltIntent {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a target speed the daemon actually wrote (CLI `speed:` /
    /// `speed_step:`, resume restore, default speed, Zone Hold).
    pub fn note_speed(&mut self, target: CentiKmh, now: Instant) {
        self.last_target = Some((target, now));
    }

    /// Record a CLI-issued start or stop the daemon actually wrote. Auto-pause
    /// and Zone Hold stops must not call this — the operator is not at the keys.
    pub fn note_run(&mut self, run: RunIntent, now: Instant) {
        self.last_run = Some((run, now));
    }

    /// Resolve `speed_step:up|down` against intent memory and live telemetry.
    ///
    /// Base is the last target if written inside [`INTENT_WINDOW`], else live
    /// speed. Refused when the base is unknown, zero, or a Stop is still in
    /// the window (a speed key must never set a moving target on a stopping
    /// or stopped belt). The stepped value is clamped to
    /// [`SPEED_MIN`]..=[`SPEED_MAX`].
    pub fn resolve_step(
        &self,
        dir: StepDirection,
        live: Option<CentiKmh>,
        now: Instant,
    ) -> Result<CentiKmh, StepRefuse> {
        if self.recent_run(RunIntent::Stop, now) {
            return Err(StepRefuse::RecentStop);
        }
        let Some(base) = self.speed_base(live, now) else {
            return Err(StepRefuse::UnknownBase);
        };
        if base == CentiKmh::ZERO {
            return Err(StepRefuse::BeltStopped);
        }
        let stepped = match dir {
            StepDirection::Up => base.saturating_add_centi(SPEED_STEP.to_wire()),
            StepDirection::Down => base.saturating_sub_centi(SPEED_STEP.to_wire()),
        };
        Ok(stepped.clamp(SPEED_MIN, SPEED_MAX))
    }

    /// Resolve `toggle`: opposite of a start/stop still in [`INTENT_WINDOW`],
    /// else Stop if live > 0, Start if live == 0, Stop (safe) if live unknown.
    #[must_use]
    pub fn resolve_toggle(&self, live: Option<CentiKmh>, now: Instant) -> ToggleOutcome {
        if let Some((run, at)) = self.last_run
            && now.saturating_duration_since(at) < INTENT_WINDOW
        {
            let opposite = match run {
                RunIntent::Start => RunIntent::Stop,
                RunIntent::Stop => RunIntent::Start,
            };
            return ToggleOutcome {
                run: opposite,
                unknown_live: false,
            };
        }
        match live {
            Some(speed) if speed > CentiKmh::ZERO => ToggleOutcome {
                run: RunIntent::Stop,
                unknown_live: false,
            },
            Some(_) => ToggleOutcome {
                run: RunIntent::Start,
                unknown_live: false,
            },
            None => ToggleOutcome {
                run: RunIntent::Stop,
                unknown_live: true,
            },
        }
    }

    fn speed_base(&self, live: Option<CentiKmh>, now: Instant) -> Option<CentiKmh> {
        if let Some((target, at)) = self.last_target
            && now.saturating_duration_since(at) < INTENT_WINDOW
        {
            return Some(target);
        }
        live
    }

    fn recent_run(&self, want: RunIntent, now: Instant) -> bool {
        matches!(
            self.last_run,
            Some((run, at)) if run == want && now.saturating_duration_since(at) < INTENT_WINDOW
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(centi: u16) -> CentiKmh {
        CentiKmh::from_wire(centi)
    }

    #[test]
    fn step_uses_fresh_target_not_live() {
        let t0 = Instant::now();
        let mut intent = BeltIntent::new();
        intent.note_speed(c(320), t0);
        let live = Some(c(300));
        assert_eq!(
            intent.resolve_step(StepDirection::Up, live, t0 + Duration::from_secs(1)),
            Ok(c(330))
        );
        assert_eq!(
            intent.resolve_step(StepDirection::Down, live, t0 + Duration::from_millis(4999)),
            Ok(c(310))
        );
    }

    #[test]
    fn step_uses_live_when_target_expired() {
        let t0 = Instant::now();
        let mut intent = BeltIntent::new();
        intent.note_speed(c(320), t0);
        assert_eq!(
            intent.resolve_step(
                StepDirection::Up,
                Some(c(300)),
                t0 + INTENT_WINDOW
            ),
            Ok(c(310))
        );
    }

    #[test]
    fn step_clamps_at_both_bounds() {
        let t0 = Instant::now();
        let mut intent = BeltIntent::new();
        intent.note_speed(SPEED_MAX, t0);
        assert_eq!(
            intent.resolve_step(StepDirection::Up, None, t0),
            Ok(SPEED_MAX)
        );
        intent.note_speed(SPEED_MIN, t0);
        assert_eq!(
            intent.resolve_step(StepDirection::Down, None, t0),
            Ok(SPEED_MIN)
        );
    }

    #[test]
    fn step_refused_when_stopped() {
        let t0 = Instant::now();
        let intent = BeltIntent::new();
        assert_eq!(
            intent.resolve_step(StepDirection::Up, Some(CentiKmh::ZERO), t0),
            Err(StepRefuse::BeltStopped)
        );
    }

    #[test]
    fn step_refused_when_unknown() {
        let t0 = Instant::now();
        let intent = BeltIntent::new();
        assert_eq!(
            intent.resolve_step(StepDirection::Up, None, t0),
            Err(StepRefuse::UnknownBase)
        );
    }

    #[test]
    fn step_refused_after_recent_stop_even_with_fresh_target() {
        let t0 = Instant::now();
        let mut intent = BeltIntent::new();
        intent.note_speed(c(320), t0);
        intent.note_run(RunIntent::Stop, t0);
        assert_eq!(
            intent.resolve_step(StepDirection::Up, Some(c(320)), t0 + Duration::from_secs(1)),
            Err(StepRefuse::RecentStop)
        );
        // Window expired: fall through to live.
        assert_eq!(
            intent.resolve_step(StepDirection::Up, Some(c(320)), t0 + INTENT_WINDOW),
            Ok(c(330))
        );
    }

    #[test]
    fn toggle_opposite_of_recent_start() {
        let t0 = Instant::now();
        let mut intent = BeltIntent::new();
        intent.note_run(RunIntent::Start, t0);
        // Telemetry still 0 during console countdown — still Stop.
        let out = intent.resolve_toggle(Some(CentiKmh::ZERO), t0 + Duration::from_secs(1));
        assert_eq!(
            out,
            ToggleOutcome {
                run: RunIntent::Stop,
                unknown_live: false,
            }
        );
    }

    #[test]
    fn toggle_opposite_of_recent_stop() {
        let t0 = Instant::now();
        let mut intent = BeltIntent::new();
        intent.note_run(RunIntent::Stop, t0);
        let out = intent.resolve_toggle(Some(c(320)), t0 + Duration::from_secs(1));
        assert_eq!(
            out,
            ToggleOutcome {
                run: RunIntent::Start,
                unknown_live: false,
            }
        );
    }

    #[test]
    fn toggle_stop_when_moving() {
        let t0 = Instant::now();
        let intent = BeltIntent::new();
        let out = intent.resolve_toggle(Some(c(250)), t0);
        assert_eq!(
            out,
            ToggleOutcome {
                run: RunIntent::Stop,
                unknown_live: false,
            }
        );
    }

    #[test]
    fn toggle_start_when_stopped() {
        let t0 = Instant::now();
        let intent = BeltIntent::new();
        let out = intent.resolve_toggle(Some(CentiKmh::ZERO), t0);
        assert_eq!(
            out,
            ToggleOutcome {
                run: RunIntent::Start,
                unknown_live: false,
            }
        );
    }

    #[test]
    fn toggle_stop_when_unknown_live() {
        let t0 = Instant::now();
        let intent = BeltIntent::new();
        let out = intent.resolve_toggle(None, t0);
        assert_eq!(
            out,
            ToggleOutcome {
                run: RunIntent::Stop,
                unknown_live: true,
            }
        );
    }

    #[test]
    fn toggle_falls_through_to_live_after_run_window() {
        let t0 = Instant::now();
        let mut intent = BeltIntent::new();
        intent.note_run(RunIntent::Start, t0);
        let out = intent.resolve_toggle(Some(c(250)), t0 + INTENT_WINDOW);
        assert_eq!(
            out,
            ToggleOutcome {
                run: RunIntent::Stop,
                unknown_live: false,
            }
        );
    }

    #[test]
    fn three_quick_ups_accumulate_on_recorded_targets() {
        let t0 = Instant::now();
        let mut intent = BeltIntent::new();
        let live = Some(c(320));
        let first = intent
            .resolve_step(StepDirection::Up, live, t0)
            .expect("first up");
        assert_eq!(first, c(330));
        intent.note_speed(first, t0 + Duration::from_millis(10));
        let second = intent
            .resolve_step(StepDirection::Up, live, t0 + Duration::from_millis(20))
            .expect("second up");
        assert_eq!(second, c(340));
        intent.note_speed(second, t0 + Duration::from_millis(30));
        let third = intent
            .resolve_step(StepDirection::Up, live, t0 + Duration::from_millis(40))
            .expect("third up");
        assert_eq!(third, c(350));
    }

    #[test]
    fn refuse_reasons_are_human() {
        assert_eq!(StepRefuse::BeltStopped.as_str(), "belt is stopped");
        assert_eq!(StepRefuse::UnknownBase.as_str(), "belt speed is unknown");
        assert_eq!(StepRefuse::RecentStop.as_str(), "belt is stopping");
    }
}
