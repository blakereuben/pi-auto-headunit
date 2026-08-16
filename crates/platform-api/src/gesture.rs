//! Detects a two-stage "arm, then act" gesture family for opening the head
//! unit's own settings/actions while a live Android Auto session is
//! showing full-screen video (`MILESTONE_CHECKLIST.md` M3's touch item).
//! Stage one — four fingers touching down together, swiping down, then
//! lifting — is deliberately hard to trigger by accident and never
//! overlaps anything Android Auto's own UI recognizes (at most two
//! simultaneous fingers). It doesn't perform an action itself; it just
//! opens a short window (`ArmedGestureDetector`'s `arm_window_micros`)
//! during which one of several recognized follow-up gestures —
//! [`GestureId::DoubleTap`], [`GestureId::TwoFingerTap`],
//! [`GestureId::LongPress`], or one of the four single-finger swipe
//! directions ([`GestureId::SwipeUp`]/`SwipeDown`/`SwipeLeft`/`SwipeRight`,
//! added 2026-08-16 for app-category switching) — can fire. Which action
//! each follow-up gesture performs is a head-unit-side, user-reassignable
//! mapping, kept out of this crate entirely (`platform-api` stays a pure
//! capability/data layer per `ARCHITECTURE.md`'s dependency rule) — see
//! `apps/aa-headunit-diagnostics/src/gesture_settings.rs`.
//!
//! Every single-finger outcome (tap, long-press, or one of the four swipe
//! directions) is classified by one unified [`SingleFingerGestureRecognizer`]
//! rather than several independent recognizers racing each other — see
//! [`SingleFingerOutcome`]'s doc comment for the real bug this replaced.
//! [`TwoFingerTapRecognizer`] runs alongside it as the one genuinely
//! independent recognizer, since a two-finger tap (down together, up
//! together) can never be confused with any single-finger outcome by
//! finger count alone (deliberately chosen 2026-08-16, renamed from an
//! original three-finger design once the operator pointed out two fingers
//! weren't used by anything else here) — a real touch frame's
//! `points.len()` can only ever match one of the two recognizers' finger
//! counts at a time, never both.
//!
//! Pure and timestamp-driven (`TouchFrame.timestamp_micros`, not wall-clock
//! `Instant`), matching `ARCHITECTURE.md`'s testing architecture ("Pure
//! unit tests: state machines, parsers, geometry transforms") — this can be
//! fully exercised without real touch hardware.
//!
//! This project does not yet suppress touch reports sent to the phone
//! while a gesture is being recognized — every raw frame, including ones
//! that turn out to be part of a gesture, is still forwarded as ordinary
//! touch input (see `auth_discovery_probe.rs::service_touch_input`), a
//! known, accepted limitation rather than added complexity for a dev-only
//! diagnostic feature.
//!
//! The arming swipe's distance check measures straight-line displacement
//! magnitude, not specifically "moved downward" — real-hardware trial,
//! 2026-08-16: an earlier revision checked for the *target-space* Y
//! coordinate increasing, and that broke the arming swipe completely the
//! moment the operator changed the live touch rotation (via the settings
//! panel's own "Cycle rotation" control this gesture opens) — a real
//! physical downward swipe on the panel no longer produced an increasing
//! target Y at all once rotation remapped which raw axis feeds target X
//! vs Y. `TouchFrame`s reaching this detector are already in
//! rotation-adjusted target-space coordinates (rotation is applied
//! upstream, in `platform_linux::touch`), so this detector has no access
//! to true unrotated panel coordinates — but displacement *magnitude* is
//! preserved by every one of the four supported rotations (all are
//! distance-preserving isometries: axis swaps and reflections, never
//! scaling), so checking that instead is correct under any current
//! rotation setting without needing raw coordinates at all.

use crate::{TouchFrame, TouchPhase};

const FOUR_FINGER_COUNT: usize = 4;
const TWO_FINGER_COUNT: usize = 2;
const TAP_MAX_DURATION_MICROS: u64 = 400_000;
const DOUBLE_TAP_GAP_MAX_MICROS: u64 = 500_000;
const LONG_PRESS_MIN_DURATION_MICROS: u64 = 800_000;
const LONG_PRESS_MAX_DURATION_MICROS: u64 = 5_000_000;
/// How far a single finger may drift and still count as "held still" for
/// tap/long-press classification, rather than a swipe — squared, compared
/// against squared displacement the same way the arming swipe's own
/// threshold is (see [`ArmedGestureDetector`]'s doc comment).
const STATIONARY_MAX_DISTANCE_SQUARED: u64 = 40 * 40;

/// Which follow-up gesture completed while armed. Kept small and closed
/// (not an open plugin system) — see this module's doc comment for why.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GestureId {
    DoubleTap,
    TwoFingerTap,
    LongPress,
    SwipeUp,
    SwipeDown,
    SwipeLeft,
    SwipeRight,
}

impl GestureId {
    /// Every recognized follow-up gesture, in a stable order — used to
    /// build the fixed set of recognizers `ArmedGestureDetector` runs in
    /// parallel while armed, and by callers (the settings UI) that need to
    /// list every assignable gesture.
    #[must_use]
    pub const fn all() -> [GestureId; 7] {
        [
            GestureId::DoubleTap,
            GestureId::TwoFingerTap,
            GestureId::LongPress,
            GestureId::SwipeUp,
            GestureId::SwipeDown,
            GestureId::SwipeLeft,
            GestureId::SwipeRight,
        ]
    }
}

/// A single simultaneous two-finger tap: both fingers down together, then
/// both lifted together, within [`TAP_MAX_DURATION_MICROS`] — not two
/// sequential taps (see this module's doc comment for why).
///
/// Fires the instant the finger count drops *below* two, rather than
/// waiting for an exact "still 2, now releasing" frame — real-hardware
/// trial, 2026-08-16: real fingers essentially never release in perfect
/// lockstep, and `MultiTouchTracker` reports at most one slot transition
/// per frame (its own doc comment: "Reports at most one pointer
/// transition ... per frame ... folded silently into the next committed
/// state without its own Up/PointerUp frame" when two departures land in
/// the same input batch). An earlier revision (when this gesture used
/// three fingers) required seeing a frame tagged `Up`/`PointerUp` with
/// the exact starting count still listed, which that folding behavior
/// can skip entirely — so this gesture would silently fail to fire on
/// most real releases.
#[derive(Default)]
struct TwoFingerTapRecognizer {
    down_at_micros: Option<u64>,
}

impl TwoFingerTapRecognizer {
    fn push(&mut self, frame: &TouchFrame) -> bool {
        match self.down_at_micros {
            None => {
                if is_tap_down(frame, TWO_FINGER_COUNT) {
                    self.down_at_micros = Some(frame.timestamp_micros);
                }
                false
            }
            Some(down_at_micros) => {
                if expired(frame, down_at_micros + TAP_MAX_DURATION_MICROS) {
                    self.down_at_micros = None;
                    return false;
                }
                if frame.points.len() > TWO_FINGER_COUNT {
                    // A third finger joined — not a clean two-finger tap.
                    self.down_at_micros = None;
                    return false;
                }
                if frame.points.len() < TWO_FINGER_COUNT {
                    self.down_at_micros = None;
                    return true;
                }
                false
            }
        }
    }

    fn reset(&mut self) {
        self.down_at_micros = None;
    }
}

/// One single-finger touch-down-then-up episode, mid-classification.
#[derive(Clone, Copy)]
struct SingleFingerTouch {
    start_x: u32,
    start_y: u32,
    down_at_micros: u64,
}

/// What a completed single-finger touch (down then up) turned out to be,
/// decided once, at release, from its own duration and total displacement
/// — never from independent, separately-racing recognizers. Real-hardware
/// trial, 2026-08-16: this project's first attempt at a two-finger tap
/// used independent `DoubleTap`/`LongPress` recognizers watching every
/// frame in parallel, and a two-finger tap's own naturally-staggered
/// landing (one finger touches first) primed the single-finger
/// `DoubleTap` recognizer's state, letting a later, completely unrelated
/// tap complete a spurious `DoubleTap`. Unifying every single-finger
/// outcome into one classification, decided exactly once per touch
/// episode, makes that whole class of bug structurally impossible instead
/// of something to keep patching defensively.
enum SingleFingerOutcome {
    /// Short duration, stayed within [`STATIONARY_MAX_DISTANCE_SQUARED`]
    /// — a plain tap, contributing to the double-tap count.
    Tap,
    /// Duration within the long-press window, stayed stationary.
    LongPress,
    /// Displacement met the swipe threshold, in the given direction —
    /// duration doesn't matter (a slow swipe is still a swipe, not a
    /// long press, since it moved).
    Swipe(GestureId),
    /// Matched none of the above (e.g. moved a little but not far enough,
    /// or held for a duration in the dead zone between a tap and a long
    /// press) — not a valid outcome on its own.
    Ambiguous,
}

fn classify_single_finger_touch(
    touch: SingleFingerTouch,
    end_x: u32,
    end_y: u32,
    end_at_micros: u64,
    swipe_threshold_squared: u64,
) -> SingleFingerOutcome {
    let duration_micros = end_at_micros.saturating_sub(touch.down_at_micros);
    let distance_squared = squared_distance(touch.start_x, touch.start_y, end_x, end_y);
    if distance_squared >= swipe_threshold_squared {
        let dx = i64::from(end_x) - i64::from(touch.start_x);
        let dy = i64::from(end_y) - i64::from(touch.start_y);
        return SingleFingerOutcome::Swipe(dominant_swipe_direction(dx, dy));
    }
    if distance_squared < STATIONARY_MAX_DISTANCE_SQUARED {
        if duration_micros <= TAP_MAX_DURATION_MICROS {
            return SingleFingerOutcome::Tap;
        }
        if (LONG_PRESS_MIN_DURATION_MICROS..=LONG_PRESS_MAX_DURATION_MICROS)
            .contains(&duration_micros)
        {
            return SingleFingerOutcome::LongPress;
        }
    }
    SingleFingerOutcome::Ambiguous
}

/// Larger axis of displacement wins; ties go horizontal. `dy` follows this
/// crate's existing screen convention (increasing downward — see
/// [`ArmedGestureDetector`]'s own arming-swipe doc comment).
fn dominant_swipe_direction(dx: i64, dy: i64) -> GestureId {
    if dx.abs() >= dy.abs() {
        if dx >= 0 {
            GestureId::SwipeRight
        } else {
            GestureId::SwipeLeft
        }
    } else if dy >= 0 {
        GestureId::SwipeDown
    } else {
        GestureId::SwipeUp
    }
}

enum SingleFingerStage {
    Idle,
    Down(SingleFingerTouch),
    /// The first touch classified as a plain [`SingleFingerOutcome::Tap`];
    /// waiting to see whether a second one arrives to complete
    /// [`GestureId::DoubleTap`].
    AwaitingSecondDown {
        deadline_micros: u64,
    },
    SecondDown(SingleFingerTouch),
}

/// Classifies every single-finger touch episode while armed into exactly
/// one outcome (tap / long press / one of four swipe directions), and
/// separately counts consecutive taps toward [`GestureId::DoubleTap`] —
/// see [`SingleFingerOutcome`]'s doc comment for why this replaced two
/// independent recognizers.
struct SingleFingerGestureRecognizer {
    stage: SingleFingerStage,
    swipe_threshold_squared: u64,
}

impl SingleFingerGestureRecognizer {
    fn new(swipe_threshold_squared: u64) -> Self {
        Self {
            stage: SingleFingerStage::Idle,
            swipe_threshold_squared,
        }
    }

    fn reset(&mut self) {
        self.stage = SingleFingerStage::Idle;
    }

    fn push(&mut self, frame: &TouchFrame) -> Option<GestureId> {
        match self.stage {
            SingleFingerStage::Idle => {
                if is_tap_down(frame, 1) {
                    self.stage = SingleFingerStage::Down(SingleFingerTouch {
                        start_x: frame.points[0].x,
                        start_y: frame.points[0].y,
                        down_at_micros: frame.timestamp_micros,
                    });
                }
                None
            }
            SingleFingerStage::Down(touch) => self.advance_touch(frame, touch, false),
            SingleFingerStage::AwaitingSecondDown { deadline_micros } => {
                if expired(frame, deadline_micros) {
                    self.stage = SingleFingerStage::Idle;
                    return None;
                }
                if frame.points.len() > 1 {
                    // A second finger joined instead of a clean second
                    // tap — not a double-tap attempt after all.
                    self.stage = SingleFingerStage::Idle;
                    return None;
                }
                if is_tap_down(frame, 1) {
                    self.stage = SingleFingerStage::SecondDown(SingleFingerTouch {
                        start_x: frame.points[0].x,
                        start_y: frame.points[0].y,
                        down_at_micros: frame.timestamp_micros,
                    });
                }
                None
            }
            SingleFingerStage::SecondDown(touch) => self.advance_touch(frame, touch, true),
        }
    }

    /// Shared by [`SingleFingerStage::Down`] and `::SecondDown` — only the
    /// outcome-on-tap differs (arm the second-tap wait vs. complete
    /// `DoubleTap`).
    fn advance_touch(
        &mut self,
        frame: &TouchFrame,
        touch: SingleFingerTouch,
        is_second_touch: bool,
    ) -> Option<GestureId> {
        if frame.points.len() > 1 {
            self.stage = SingleFingerStage::Idle;
            return None;
        }
        if frame.timestamp_micros.saturating_sub(touch.down_at_micros)
            > LONG_PRESS_MAX_DURATION_MICROS
        {
            // Held far longer than any recognized gesture needs; whatever
            // this is, it isn't one of ours.
            self.stage = SingleFingerStage::Idle;
            return None;
        }
        if !is_tap_up(frame, 1) {
            return None;
        }
        let outcome = classify_single_finger_touch(
            touch,
            frame.points[0].x,
            frame.points[0].y,
            frame.timestamp_micros,
            self.swipe_threshold_squared,
        );
        match outcome {
            SingleFingerOutcome::Swipe(gesture) => {
                self.stage = SingleFingerStage::Idle;
                Some(gesture)
            }
            SingleFingerOutcome::LongPress if !is_second_touch => {
                self.stage = SingleFingerStage::Idle;
                Some(GestureId::LongPress)
            }
            SingleFingerOutcome::Tap if is_second_touch => {
                self.stage = SingleFingerStage::Idle;
                Some(GestureId::DoubleTap)
            }
            SingleFingerOutcome::Tap => {
                self.stage = SingleFingerStage::AwaitingSecondDown {
                    deadline_micros: frame.timestamp_micros + DOUBLE_TAP_GAP_MAX_MICROS,
                };
                None
            }
            SingleFingerOutcome::LongPress | SingleFingerOutcome::Ambiguous => {
                // A long press as the *second* tap of a double-tap
                // attempt, or anything ambiguous either time, completes
                // nothing.
                self.stage = SingleFingerStage::Idle;
                None
            }
        }
    }
}

fn is_tap_down(frame: &TouchFrame, finger_count: usize) -> bool {
    matches!(frame.phase, TouchPhase::Down | TouchPhase::PointerDown)
        && frame.points.len() == finger_count
}

fn is_tap_up(frame: &TouchFrame, finger_count: usize) -> bool {
    matches!(frame.phase, TouchPhase::Up | TouchPhase::PointerUp)
        && frame.points.len() == finger_count
}

fn expired(frame: &TouchFrame, deadline_micros: u64) -> bool {
    frame.timestamp_micros > deadline_micros
}

enum ArmState {
    Idle,
    Watching { start_x: u32, start_y: u32 },
    Armed { deadline_micros: u64 },
}

/// What just happened, reported to the caller so it can give real
/// feedback (an on-screen mask while armed — real-hardware feedback,
/// 2026-08-16: without this, an operator who only performs the swipe half
/// has no way to know whether anything registered at all) rather than
/// only reporting a fully completed gesture.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GestureEvent {
    /// The arming swipe just completed; a follow-up gesture now has
    /// [`SharedArmWindow`]'s current duration to arrive.
    Armed,
    /// The armed window closed with no follow-up gesture completing.
    Disarmed,
    /// A follow-up gesture completed while armed.
    Completed(GestureId),
}

/// A live-adjustable arm-window duration, mirroring
/// `platform_linux::touch::SharedRotation`'s pattern — cheap to clone (an
/// `Arc` around one atomic), read fresh by [`ArmedGestureDetector`] each
/// time a swipe actually arms, so a change takes effect on the very next
/// swipe with no restart needed.
#[derive(Clone)]
pub struct SharedArmWindow(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl SharedArmWindow {
    #[must_use]
    pub fn new(initial_micros: u64) -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(
            initial_micros,
        )))
    }

    pub fn set_micros(&self, micros: u64) {
        self.0.store(micros, std::sync::atomic::Ordering::SeqCst);
    }

    fn get_micros(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Orchestrates the whole two-stage gesture: the four-finger swipe arms a
/// window ([`SharedArmWindow`] long), during which every [`GestureId`]'s
/// recognizer runs in parallel; the first to complete wins and disarms
/// immediately. An armed window that times out with nothing completing
/// disarms silently back to watching for the next swipe.
pub struct ArmedGestureDetector {
    state: ArmState,
    /// Squared, to compare against squared displacement without a `sqrt`
    /// call — see [`Self::push`]'s `Watching` arm.
    swipe_threshold_squared: u64,
    arm_window: SharedArmWindow,
    single_finger: SingleFingerGestureRecognizer,
    two_finger_tap: TwoFingerTapRecognizer,
    /// The most fingers seen simultaneously since the touch surface was
    /// last completely empty (reset on every `TouchPhase::Up`) — see
    /// [`Self::push`]'s `Armed` arm for why this exists: a two-finger
    /// tap's first-landing finger is, for one frame, indistinguishable
    /// from the start of a genuine single-finger tap.
    episode_max_fingers: usize,
}

impl ArmedGestureDetector {
    #[must_use]
    pub fn new(swipe_threshold: u32, initial_arm_window_micros: u64) -> Self {
        let swipe_threshold_squared = u64::from(swipe_threshold) * u64::from(swipe_threshold);
        Self {
            state: ArmState::Idle,
            swipe_threshold_squared,
            arm_window: SharedArmWindow::new(initial_arm_window_micros),
            single_finger: SingleFingerGestureRecognizer::new(swipe_threshold_squared),
            two_finger_tap: TwoFingerTapRecognizer::default(),
            episode_max_fingers: 0,
        }
    }

    /// A cloneable handle that can change this detector's arm-window
    /// duration live — see [`SharedArmWindow`].
    #[must_use]
    pub fn arm_window_handle(&self) -> SharedArmWindow {
        self.arm_window.clone()
    }

    /// Feeds one frame. Returns `Some(event)` on the frame that arms,
    /// disarms, or completes a follow-up gesture; `None` otherwise
    /// (including every other frame of the arming swipe itself).
    pub fn push(&mut self, frame: &TouchFrame) -> Option<GestureEvent> {
        match self.state {
            ArmState::Idle => {
                if frame.points.len() == FOUR_FINGER_COUNT {
                    let (start_x, start_y) = average_position(frame);
                    self.state = ArmState::Watching { start_x, start_y };
                }
                None
            }
            ArmState::Watching { start_x, start_y } => {
                if frame.points.len() > FOUR_FINGER_COUNT {
                    self.state = ArmState::Idle;
                    return None;
                }
                if frame.phase == TouchPhase::Up && frame.points.len() == 1 {
                    let end_x = frame.points[0].x;
                    let end_y = frame.points[0].y;
                    // Displacement *magnitude*, not "moved down" — a
                    // physical swipe stays the same length however the
                    // live touch rotation currently remaps which raw axis
                    // feeds target X vs Y (real-hardware trial,
                    // 2026-08-16: checking specifically for downward
                    // motion in the *rotated* target space broke this
                    // arming swipe entirely the moment the operator
                    // changed rotation mid-session, since a real physical
                    // downward swipe no longer looked "downward" in that
                    // now-remapped coordinate space at all). All four
                    // rotations are distance-preserving, so this check is
                    // correct regardless of the live rotation setting.
                    if squared_distance(start_x, start_y, end_x, end_y)
                        >= self.swipe_threshold_squared
                    {
                        self.reset_follow_up_recognizers();
                        self.episode_max_fingers = 0;
                        self.state = ArmState::Armed {
                            deadline_micros: frame.timestamp_micros + self.arm_window.get_micros(),
                        };
                        return Some(GestureEvent::Armed);
                    }
                    self.state = ArmState::Idle;
                }
                None
            }
            ArmState::Armed { deadline_micros } => {
                if frame.timestamp_micros > deadline_micros {
                    self.state = ArmState::Idle;
                    return Some(GestureEvent::Disarmed);
                }
                // A two-finger tap's fingers essentially never land or
                // lift in perfect lockstep on real hardware — for one
                // frame, the first finger touching down is
                // indistinguishable from the start of a genuine
                // single-finger tap. Real-hardware trial, 2026-08-16:
                // `DoubleTapRecognizer` picked up that leading single-
                // finger frame as a real first tap, and a later,
                // unrelated single-finger touch (e.g. interacting with
                // whatever the gesture opened) could then complete it,
                // firing `DoubleTap` instead of the intended
                // `TwoFingerTap`. Fix: the single-finger recognizer only
                // runs at all once this touch episode has ever shown more
                // than one finger, and gets explicitly wiped the instant a
                // second finger reveals that — before any later unrelated
                // touch could exploit its stale leading-edge state.
                let was_single_finger_episode = self.episode_max_fingers <= 1;
                self.episode_max_fingers = self.episode_max_fingers.max(frame.points.len());
                if was_single_finger_episode && self.episode_max_fingers > 1 {
                    self.single_finger.reset();
                }
                let single_finger_episode = self.episode_max_fingers <= 1;
                let result = if single_finger_episode {
                    self.single_finger.push(frame)
                } else {
                    None
                }
                .or_else(|| {
                    if self.two_finger_tap.push(frame) {
                        Some(GestureId::TwoFingerTap)
                    } else {
                        None
                    }
                });
                if frame.phase == TouchPhase::Up {
                    self.episode_max_fingers = 0;
                }
                if let Some(gesture) = result {
                    self.state = ArmState::Idle;
                    return Some(GestureEvent::Completed(gesture));
                }
                None
            }
        }
    }

    /// Checks the armed deadline against the current time with no touch
    /// frame required — `push` alone can only notice a timeout on the
    /// *next* frame, so an operator who arms the gesture and then touches
    /// nothing at all would otherwise stay "armed" (and any on-screen
    /// indicator stuck showing) forever. Callers should call this once per
    /// poll iteration regardless of whether any touch frame arrived (see
    /// `auth_discovery_probe.rs::service_touch_input`). A no-op outside
    /// `Armed`.
    pub fn tick(&mut self, now_micros: u64) -> Option<GestureEvent> {
        if let ArmState::Armed { deadline_micros } = self.state {
            if now_micros > deadline_micros {
                self.state = ArmState::Idle;
                return Some(GestureEvent::Disarmed);
            }
        }
        None
    }

    fn reset_follow_up_recognizers(&mut self) {
        self.single_finger.reset();
        self.two_finger_tap.reset();
    }
}

fn average_position(frame: &TouchFrame) -> (u32, u32) {
    if frame.points.is_empty() {
        return (0, 0);
    }
    let count = frame.points.len() as u64;
    let sum_x: u64 = frame.points.iter().map(|point| u64::from(point.x)).sum();
    let sum_y: u64 = frame.points.iter().map(|point| u64::from(point.y)).sum();
    #[allow(clippy::cast_possible_truncation)]
    let average = ((sum_x / count) as u32, (sum_y / count) as u32);
    average
}

fn squared_distance(x1: u32, y1: u32, x2: u32, y2: u32) -> u64 {
    let dx = i64::from(x2) - i64::from(x1);
    let dy = i64::from(y2) - i64::from(y1);
    #[allow(clippy::cast_sign_loss)]
    let squared = (dx * dx + dy * dy) as u64;
    squared
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TouchPoint;

    const ARM_WINDOW_MICROS: u64 = 3_000_000;

    fn frame(timestamp_micros: u64, phase: TouchPhase, points: Vec<(u32, u32, u32)>) -> TouchFrame {
        TouchFrame {
            timestamp_micros,
            action_index: 0,
            phase,
            points: points
                .into_iter()
                .map(|(pointer_id, x, y)| TouchPoint { pointer_id, x, y })
                .collect(),
        }
    }

    fn four_fingers_at(timestamp_micros: u64, y: u32) -> TouchFrame {
        frame(
            timestamp_micros,
            TouchPhase::PointerDown,
            vec![(0, 100, y), (1, 200, y), (2, 300, y), (3, 400, y)],
        )
    }

    fn swipe(detector: &mut ArmedGestureDetector) {
        assert_eq!(detector.push(&four_fingers_at(1_000_000, 100)), None);
        let swipe_end = frame(2_000_000, TouchPhase::Up, vec![(0, 50, 500)]);
        assert_eq!(detector.push(&swipe_end), Some(GestureEvent::Armed));
    }

    fn tap_down(timestamp_micros: u64) -> TouchFrame {
        frame(timestamp_micros, TouchPhase::Down, vec![(0, 50, 50)])
    }

    fn tap_up(timestamp_micros: u64) -> TouchFrame {
        frame(timestamp_micros, TouchPhase::Up, vec![(0, 50, 50)])
    }

    fn two_finger_tap_down(timestamp_micros: u64) -> TouchFrame {
        frame(
            timestamp_micros,
            TouchPhase::PointerDown,
            vec![(0, 50, 50), (1, 150, 50)],
        )
    }

    /// A realistic release: one finger lifts first (per
    /// `MultiTouchTracker`'s own one-transition-per-frame contract, the
    /// tracker essentially never reports both lifting in the same frame),
    /// dropping from 2 points to 1 — not the exact "still 2, tagged Up"
    /// frame the fragile pre-2026-08-16 release check required.
    fn two_finger_tap_up(timestamp_micros: u64) -> TouchFrame {
        frame(timestamp_micros, TouchPhase::PointerUp, vec![(0, 50, 50)])
    }

    #[test]
    fn swipe_then_double_tap_fires_double_tap() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(detector.push(&tap_down(2_100_000)), None);
        assert_eq!(detector.push(&tap_up(2_200_000)), None);
        assert_eq!(detector.push(&tap_down(2_400_000)), None);
        assert_eq!(
            detector.push(&tap_up(2_500_000)),
            Some(GestureEvent::Completed(GestureId::DoubleTap))
        );
    }

    /// Regression test for the real-hardware bug (2026-08-16): a swipe
    /// that moves purely horizontally in target-space coordinates — what
    /// a real physical *downward* swipe looks like once a 90°/270° live
    /// touch rotation remaps which raw axis feeds target X vs Y — must
    /// still arm, since the check is displacement magnitude, not
    /// specifically "moved down."
    #[test]
    fn a_purely_horizontal_swipe_still_arms() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        assert_eq!(detector.push(&four_fingers_at(1_000_000, 100)), None);
        let horizontal_swipe_end = frame(2_000_000, TouchPhase::Up, vec![(0, 500, 100)]);
        assert_eq!(
            detector.push(&horizontal_swipe_end),
            Some(GestureEvent::Armed)
        );
    }

    #[test]
    fn swipe_then_two_finger_tap_fires_two_finger_tap() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(detector.push(&two_finger_tap_down(2_100_000)), None);
        assert_eq!(
            detector.push(&two_finger_tap_up(2_200_000)),
            Some(GestureEvent::Completed(GestureId::TwoFingerTap))
        );
    }

    /// Regression test for the real-hardware bug (2026-08-16): a staggered
    /// two-finger tap (fingers landing one at a time, as real hardware
    /// always reports it — see `TwoFingerTapRecognizer`'s doc comment)
    /// must still fire `TwoFingerTap`, not `DoubleTap`, even though its
    /// first frame alone (one finger down) is indistinguishable from the
    /// start of a real single-finger tap.
    #[test]
    fn a_staggered_two_finger_tap_fires_two_finger_tap_not_double_tap() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        // Fingers land one at a time, exactly as MultiTouchTracker's own
        // one-transition-per-frame contract always reports it.
        assert_eq!(
            detector.push(&frame(2_100_000, TouchPhase::Down, vec![(0, 50, 50)])),
            None
        );
        assert_eq!(
            detector.push(&frame(
                2_101_000,
                TouchPhase::PointerDown,
                vec![(0, 50, 50), (1, 150, 50)]
            )),
            None
        );
        assert_eq!(
            detector.push(&two_finger_tap_up(2_200_000)),
            Some(GestureEvent::Completed(GestureId::TwoFingerTap))
        );
    }

    /// Regression test for the real-hardware bug (2026-08-16): after a
    /// staggered two-finger tap's leading single-finger frame primed
    /// `DoubleTapRecognizer`'s state, a later, completely unrelated
    /// single-finger tap must not be able to complete a spurious
    /// `DoubleTap` using that stale state.
    #[test]
    fn a_stray_tap_after_a_staggered_two_finger_landing_does_not_complete_double_tap() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        // Same staggered landing as above, but this time the group is
        // aborted (a third finger joins, disqualifying it) rather than
        // completing two-finger-tap — worst case for leaked state, since
        // two_finger_tap never fires and resets nothing on its own.
        assert_eq!(
            detector.push(&frame(2_100_000, TouchPhase::Down, vec![(0, 50, 50)])),
            None
        );
        assert_eq!(
            detector.push(&frame(
                2_101_000,
                TouchPhase::PointerDown,
                vec![(0, 50, 50), (1, 150, 50)]
            )),
            None
        );
        let third_finger_disqualifies = frame(
            2_102_000,
            TouchPhase::PointerDown,
            vec![(0, 50, 50), (1, 150, 50), (2, 250, 50)],
        );
        assert_eq!(detector.push(&third_finger_disqualifies), None);
        let all_lifted = frame(2_150_000, TouchPhase::Up, vec![(0, 50, 50)]);
        assert_eq!(detector.push(&all_lifted), None);
        // A later, unrelated single tap, well within what would have been
        // DoubleTapRecognizer's own gap window from the leading frame
        // above — must not complete anything by itself.
        assert_eq!(detector.push(&tap_down(2_300_000)), None);
        assert_eq!(detector.push(&tap_up(2_400_000)), None);
    }

    #[test]
    fn a_slow_two_finger_release_past_the_tap_window_does_not_fire() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(detector.push(&two_finger_tap_down(2_100_000)), None);
        // Released 2 seconds later — far past a tap's timing.
        assert_eq!(detector.push(&two_finger_tap_up(4_100_000)), None);
    }

    #[test]
    fn swipe_then_long_press_fires_long_press() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(detector.push(&tap_down(2_100_000)), None);
        let long_press_up = frame(3_000_000, TouchPhase::Up, vec![(0, 50, 50)]);
        assert_eq!(
            detector.push(&long_press_up),
            Some(GestureEvent::Completed(GestureId::LongPress))
        );
    }

    #[test]
    fn a_release_before_the_long_press_minimum_duration_is_just_a_tap() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(detector.push(&tap_down(2_100_000)), None);
        // Released after only 100ms — a normal tap, not a long press. This
        // becomes the first half of a double-tap attempt instead.
        assert_eq!(detector.push(&tap_up(2_200_000)), None);
    }

    #[test]
    fn a_short_swipe_never_arms() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        // Average start position of `four_fingers_at(_, 100)` is (250, 100)
        // — end near that same x so this isolates a short *overall*
        // displacement, not a coincidentally-large horizontal one.
        assert_eq!(detector.push(&four_fingers_at(1_000_000, 100)), None);
        let short_swipe_end = frame(1_100_000, TouchPhase::Up, vec![(0, 250, 150)]);
        assert_eq!(detector.push(&short_swipe_end), None);
        assert_eq!(detector.push(&tap_down(1_200_000)), None);
        assert_eq!(detector.push(&tap_up(1_300_000)), None);
        assert_eq!(detector.push(&tap_down(1_400_000)), None);
        assert_eq!(detector.push(&tap_up(1_500_000)), None);
    }

    #[test]
    fn the_armed_window_expiring_disarms_without_firing() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        // A single tap-down long after the arm window closed: the first
        // frame observed past the deadline reports Disarmed, not a
        // completed gesture.
        let late_tap_down = tap_down(2_000_000 + ARM_WINDOW_MICROS + 1);
        assert_eq!(detector.push(&late_tap_down), Some(GestureEvent::Disarmed));
        // Disarmed already fired once; the very next frame is back to
        // ordinary Idle behavior.
        assert_eq!(
            detector.push(&tap_up(2_000_000 + ARM_WINDOW_MICROS + 2)),
            None
        );
    }

    #[test]
    fn tick_disarms_with_no_further_touch_frames() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(detector.tick(2_000_000 + ARM_WINDOW_MICROS), None);
        assert_eq!(
            detector.tick(2_000_000 + ARM_WINDOW_MICROS + 1),
            Some(GestureEvent::Disarmed)
        );
        // Only reported once.
        assert_eq!(detector.tick(2_000_000 + ARM_WINDOW_MICROS + 2), None);
    }

    #[test]
    fn tick_is_a_no_op_outside_the_armed_state() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        assert_eq!(detector.tick(u64::MAX), None);
    }

    #[test]
    fn arm_window_handle_changes_take_effect_on_the_next_swipe() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        let handle = detector.arm_window_handle();
        handle.set_micros(1_000_000);
        swipe(&mut detector);
        // A follow-up gesture just past the *old* 3s window but within the
        // new, shorter 1s window is already too late.
        assert_eq!(
            detector.tick(2_000_000 + 1_000_000 + 1),
            Some(GestureEvent::Disarmed)
        );
    }

    #[test]
    fn an_ordinary_single_finger_tap_never_fires_anything() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        assert_eq!(detector.push(&tap_down(1_000_000)), None);
        assert_eq!(detector.push(&tap_up(1_100_000)), None);
    }

    #[test]
    fn detector_is_reusable_after_firing_once() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(detector.push(&tap_down(2_100_000)), None);
        assert_eq!(detector.push(&tap_up(2_200_000)), None);
        assert_eq!(detector.push(&tap_down(2_400_000)), None);
        assert_eq!(
            detector.push(&tap_up(2_500_000)),
            Some(GestureEvent::Completed(GestureId::DoubleTap))
        );

        swipe(&mut detector);
        assert_eq!(detector.push(&tap_down(2_100_000)), None);
        assert_eq!(detector.push(&tap_up(2_200_000)), None);
        assert_eq!(detector.push(&tap_down(2_400_000)), None);
        assert_eq!(
            detector.push(&tap_up(2_500_000)),
            Some(GestureEvent::Completed(GestureId::DoubleTap))
        );
    }

    #[test]
    fn all_lists_every_variant_exactly_once() {
        let all = GestureId::all();
        assert_eq!(all.len(), 7);
        assert!(all.contains(&GestureId::DoubleTap));
        assert!(all.contains(&GestureId::TwoFingerTap));
        assert!(all.contains(&GestureId::LongPress));
        assert!(all.contains(&GestureId::SwipeUp));
        assert!(all.contains(&GestureId::SwipeDown));
        assert!(all.contains(&GestureId::SwipeLeft));
        assert!(all.contains(&GestureId::SwipeRight));
    }

    #[test]
    fn a_single_finger_swipe_fires_the_matching_direction() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(
            detector.push(&frame(2_000_000, TouchPhase::Down, vec![(0, 400, 400)])),
            None
        );
        assert_eq!(
            detector.push(&frame(2_100_000, TouchPhase::Up, vec![(0, 400, 100)])),
            Some(GestureEvent::Completed(GestureId::SwipeUp))
        );
    }

    #[test]
    fn a_single_finger_swipe_right_fires_swipe_right_not_a_tap() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(
            detector.push(&frame(2_000_000, TouchPhase::Down, vec![(0, 100, 400)])),
            None
        );
        assert_eq!(
            detector.push(&frame(2_100_000, TouchPhase::Up, vec![(0, 500, 420)])),
            Some(GestureEvent::Completed(GestureId::SwipeRight))
        );
    }

    #[test]
    fn a_slow_swipe_still_fires_the_swipe_not_a_long_press() {
        let mut detector = ArmedGestureDetector::new(200, ARM_WINDOW_MICROS);
        swipe(&mut detector);
        assert_eq!(
            detector.push(&frame(2_000_000, TouchPhase::Down, vec![(0, 400, 100)])),
            None
        );
        assert_eq!(
            detector.push(&frame(2_900_000, TouchPhase::Up, vec![(0, 400, 400)])),
            Some(GestureEvent::Completed(GestureId::SwipeDown))
        );
    }
}
