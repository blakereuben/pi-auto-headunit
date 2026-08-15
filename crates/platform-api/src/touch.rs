//! Portable multitouch state tracking, independent of any specific input
//! device API (evdev, libinput, ...). Consumes the small set of raw slot
//! events every Linux "protocol B" multitouch driver emits and turns them
//! into complete [`TouchFrame`]s, one per finger-count change or position
//! update — the same shape `protocol_aap::encode_touch_report` needs, kept
//! free of any protocol-aap dependency so this crate stays a pure
//! capability/data-model layer (see `ARCHITECTURE.md`'s dependency rule).

/// One finger's current position, keyed by a stable per-contact id that
/// must not change for the same physical finger across its own down-move-up
/// sequence (Linux calls this `ABS_MT_TRACKING_ID`; Android's `MotionEvent`
/// calls the equivalent concept a pointer id — both map directly onto
/// `pointer_id` here).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TouchPoint {
    pub pointer_id: u32,
    pub x: u32,
    pub y: u32,
}

/// Mirrors Android's `MotionEvent.ACTION_DOWN` / `_UP` / `_MOVE` /
/// `_POINTER_DOWN` / `_POINTER_UP` exactly (see [`MultiTouchTracker`]'s doc
/// comment for why that specific contract, not an invented one, is used).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TouchPhase {
    Down,
    Up,
    Moved,
    PointerDown,
    PointerUp,
}

/// One complete touch report: every finger currently on the screen (for
/// `Up`/`PointerUp` this still includes the just-lifted finger, at its last
/// known position — matching `MotionEvent`'s own contract), plus
/// `action_index` — always present, `0` for `Down`/`Moved`/single-pointer
/// `Up` (there is only ever one relevant index) and the changed pointer's
/// position in `points` for `PointerDown`/`PointerUp`. Real-hardware-tested:
/// an earlier revision left `action_index` unset for `Moved` (`None`),
/// which sent well-formed reports with zero protocol errors but produced no
/// visible reaction on the phone during a drag/pinch, while discrete
/// `Down`/`Up` taps (which already always set it) worked correctly.
/// Confirmed against `opencardev/openauto`'s current `main` branch (the
/// same GitHub org as this project's pinned `aasdk` fork, and — unlike the
/// separately pinned `f1xpl/openauto` — updated to the same
/// `InputReport`/`TouchEvent`/`sendInputReport` schema this crate
/// implements): `InputDevice::handleMultiTouchEvent`
/// (`src/autoapp/Projection/InputDevice.cpp`) always sets
/// `event.actionIndex` (`0` for `TouchBegin`/plain movement, the changed
/// point's index otherwise) before `InputSourceService::onTouchEvent`
/// (`src/autoapp/Service/InputSource/InputSourceService.cpp`)
/// unconditionally calls `touchEvent->set_action_index(event.actionIndex)`
/// — never left unset. Not yet formally adopted with a pinned revision
/// (see `docs/protocol/openauto-adoption.md`); this is read-only
/// provenance for the fact above, pending a real-hardware confirmation
/// that this fix produces a visible effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TouchFrame {
    pub timestamp_micros: u64,
    pub points: Vec<TouchPoint>,
    pub action_index: u32,
    pub phase: TouchPhase,
}

/// The raw per-slot events a Linux "protocol B" multitouch driver reports,
/// stripped of any evdev-specific type (`AbsoluteAxisCode` etc. stay in the
/// Linux adapter crate that reads `/dev/input`, matching `ARCHITECTURE.md`:
/// "platform-api defines capabilities and operations; ... Linux code
/// implements them"). `Slot` selects which finger subsequent `TrackingId`/
/// `PositionX`/`PositionY` events apply to (defaulting to slot 0 if never
/// sent, per the protocol-B spec); `TrackingId(None)` means that slot's
/// finger just lifted; `Sync` is `SYN_REPORT` — the point at which a batch
/// of slot updates becomes one observable frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawTouchEvent {
    Slot(u32),
    TrackingId(Option<u32>),
    PositionX(u32),
    PositionY(u32),
    Sync { timestamp_micros: u64 },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SlotState {
    tracking_id: Option<u32>,
    x: u32,
    y: u32,
}

/// Tracks per-slot multitouch state and emits [`TouchFrame`]s on each
/// `Sync`. Reports at most one pointer transition (add or remove) per
/// frame, preferring an add when a single `SYN_REPORT` batch happens to
/// both add and remove a slot at once — not something real touchscreen
/// drivers do for ordinary finger movement, but not forbidden by the
/// protocol either. In that rare case the removed slot's departure is
/// folded silently into the next committed state without its own `Up`/
/// `PointerUp` frame, rather than risking a `points`/`action_index`
/// mismatch by trying to report both in one frame.
#[derive(Debug, Default)]
pub struct MultiTouchTracker {
    committed: Vec<(u32, SlotState)>,
    pending: std::collections::BTreeMap<u32, SlotState>,
    current_slot: u32,
}

impl MultiTouchTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one raw event. Returns `Some(frame)` only after a `Sync` whose
    /// resulting state actually differs from what was last reported —
    /// a `SYN_REPORT` with no real change (common; drivers sync
    /// periodically regardless of motion) is silently absorbed.
    pub fn push(&mut self, event: RawTouchEvent) -> Option<TouchFrame> {
        match event {
            RawTouchEvent::Slot(slot) => {
                self.current_slot = slot;
                None
            }
            RawTouchEvent::TrackingId(tracking_id) => {
                self.slot_mut(self.current_slot).tracking_id = tracking_id;
                None
            }
            RawTouchEvent::PositionX(x) => {
                self.slot_mut(self.current_slot).x = x;
                None
            }
            RawTouchEvent::PositionY(y) => {
                self.slot_mut(self.current_slot).y = y;
                None
            }
            RawTouchEvent::Sync { timestamp_micros } => self.sync(timestamp_micros),
        }
    }

    fn slot_mut(&mut self, slot: u32) -> &mut SlotState {
        if let std::collections::btree_map::Entry::Vacant(entry) = self.pending.entry(slot) {
            let initial = self
                .committed
                .iter()
                .find(|(id, _)| *id == slot)
                .map_or_else(SlotState::default, |(_, state)| *state);
            entry.insert(initial);
        }
        self.pending.get_mut(&slot).expect("just ensured present")
    }

    fn sync(&mut self, timestamp_micros: u64) -> Option<TouchFrame> {
        if self.pending.is_empty() {
            return None;
        }
        let mut next: Vec<(u32, SlotState)> = self
            .committed
            .iter()
            .filter(|(slot, _)| !self.pending.contains_key(slot))
            .copied()
            .collect();
        next.extend(std::mem::take(&mut self.pending));
        next.retain(|(_, state)| state.tracking_id.is_some());
        next.sort_by_key(|(slot, _)| *slot);

        let was_active = |slot: u32| {
            self.committed
                .iter()
                .any(|(id, state)| *id == slot && state.tracking_id.is_some())
        };
        let newly_active_index = next.iter().position(|(slot, _)| !was_active(*slot));
        let lifted = self.committed.iter().find_map(|(id, state)| {
            let still_active = next.iter().any(|(next_id, next_state)| {
                next_id == id && next_state.tracking_id == state.tracking_id
            });
            (state.tracking_id.is_some() && !still_active).then_some((*id, *state))
        });
        let previously_active_count = self
            .committed
            .iter()
            .filter(|(_, state)| state.tracking_id.is_some())
            .count();

        if let Some(index) = newly_active_index {
            #[allow(clippy::cast_possible_truncation)]
            let index = index as u32;
            let phase = if previously_active_count == 0 {
                TouchPhase::Down
            } else {
                TouchPhase::PointerDown
            };
            self.committed = next.clone();
            return Some(TouchFrame {
                timestamp_micros,
                points: next.into_iter().map(to_point).collect(),
                action_index: index,
                phase,
            });
        }

        if let Some((lifted_id, lifted_state)) = lifted {
            let mut points_including_lifted = next.clone();
            points_including_lifted.push((lifted_id, lifted_state));
            points_including_lifted.sort_by_key(|(slot, _)| *slot);
            let index = points_including_lifted
                .iter()
                .position(|(slot, _)| *slot == lifted_id)?;
            #[allow(clippy::cast_possible_truncation)]
            let index = index as u32;
            let phase = if next.is_empty() {
                TouchPhase::Up
            } else {
                TouchPhase::PointerUp
            };
            self.committed = next;
            return Some(TouchFrame {
                timestamp_micros,
                points: points_including_lifted.into_iter().map(to_point).collect(),
                action_index: index,
                phase,
            });
        }

        if next == self.committed {
            self.committed = next;
            return None;
        }

        self.committed = next.clone();
        Some(TouchFrame {
            timestamp_micros,
            points: next.into_iter().map(to_point).collect(),
            action_index: 0,
            phase: TouchPhase::Moved,
        })
    }
}

fn to_point((slot, state): (u32, SlotState)) -> TouchPoint {
    TouchPoint {
        pointer_id: state.tracking_id.unwrap_or(slot),
        x: state.x,
        y: state.y,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap_down(tracker: &mut MultiTouchTracker, tracking_id: u32, x: u32, y: u32) {
        assert_eq!(
            tracker.push(RawTouchEvent::TrackingId(Some(tracking_id))),
            None
        );
        assert_eq!(tracker.push(RawTouchEvent::PositionX(x)), None);
        assert_eq!(tracker.push(RawTouchEvent::PositionY(y)), None);
    }

    #[test]
    fn single_finger_down_move_up() {
        let mut tracker = MultiTouchTracker::new();
        tap_down(&mut tracker, 5, 100, 200);
        let down = tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 10,
            })
            .expect("down frame");
        assert_eq!(down.phase, TouchPhase::Down);
        assert_eq!(down.action_index, 0);
        assert_eq!(
            down.points,
            vec![TouchPoint {
                pointer_id: 5,
                x: 100,
                y: 200
            }]
        );

        assert_eq!(tracker.push(RawTouchEvent::PositionX(110)), None);
        let moved = tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 20,
            })
            .expect("move frame");
        assert_eq!(moved.phase, TouchPhase::Moved);
        assert_eq!(
            moved.points,
            vec![TouchPoint {
                pointer_id: 5,
                x: 110,
                y: 200
            }]
        );

        assert_eq!(tracker.push(RawTouchEvent::TrackingId(None)), None);
        let up = tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 30,
            })
            .expect("up frame");
        assert_eq!(up.phase, TouchPhase::Up);
        assert_eq!(up.action_index, 0);
        assert_eq!(
            up.points,
            vec![TouchPoint {
                pointer_id: 5,
                x: 110,
                y: 200
            }]
        );
    }

    #[test]
    fn redundant_sync_with_no_change_is_absorbed() {
        let mut tracker = MultiTouchTracker::new();
        tap_down(&mut tracker, 1, 1, 1);
        tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 10,
            })
            .expect("down frame");
        assert_eq!(
            tracker.push(RawTouchEvent::Sync {
                timestamp_micros: 20
            }),
            None,
            "a sync with nothing pending must not emit a frame"
        );
    }

    #[test]
    fn second_finger_reports_pointer_down_listing_both() {
        let mut tracker = MultiTouchTracker::new();
        tap_down(&mut tracker, 5, 100, 200);
        tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 10,
            })
            .expect("first down");

        assert_eq!(tracker.push(RawTouchEvent::Slot(1)), None);
        tap_down(&mut tracker, 9, 300, 400);
        let second_down = tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 20,
            })
            .expect("second down frame");
        assert_eq!(second_down.phase, TouchPhase::PointerDown);
        assert_eq!(second_down.action_index, 1);
        assert_eq!(
            second_down.points,
            vec![
                TouchPoint {
                    pointer_id: 5,
                    x: 100,
                    y: 200
                },
                TouchPoint {
                    pointer_id: 9,
                    x: 300,
                    y: 400
                },
            ]
        );
    }

    #[test]
    fn first_finger_lifts_while_second_stays_down() {
        let mut tracker = MultiTouchTracker::new();
        tap_down(&mut tracker, 5, 100, 200);
        tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 10,
            })
            .expect("first down");
        tracker.push(RawTouchEvent::Slot(1));
        tap_down(&mut tracker, 9, 300, 400);
        tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 20,
            })
            .expect("second down");

        tracker.push(RawTouchEvent::Slot(0));
        assert_eq!(tracker.push(RawTouchEvent::TrackingId(None)), None);
        let lifted = tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 30,
            })
            .expect("pointer-up frame");
        assert_eq!(lifted.phase, TouchPhase::PointerUp);
        assert_eq!(lifted.action_index, 0);
        assert_eq!(
            lifted.points,
            vec![
                TouchPoint {
                    pointer_id: 5,
                    x: 100,
                    y: 200
                },
                TouchPoint {
                    pointer_id: 9,
                    x: 300,
                    y: 400
                },
            ],
            "PointerUp must still list the lifting finger, matching MotionEvent"
        );

        tracker.push(RawTouchEvent::Slot(1));
        assert_eq!(tracker.push(RawTouchEvent::TrackingId(None)), None);
        let last_up = tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 40,
            })
            .expect("final up frame");
        assert_eq!(last_up.phase, TouchPhase::Up);
        assert_eq!(
            last_up.points,
            vec![TouchPoint {
                pointer_id: 9,
                x: 300,
                y: 400
            }]
        );
    }

    #[test]
    fn a_slot_can_be_reused_by_a_later_unrelated_finger() {
        let mut tracker = MultiTouchTracker::new();
        tap_down(&mut tracker, 5, 100, 200);
        tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 10,
            })
            .expect("down");
        assert_eq!(tracker.push(RawTouchEvent::TrackingId(None)), None);
        tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 20,
            })
            .expect("up");

        tap_down(&mut tracker, 6, 50, 60);
        let reused = tracker
            .push(RawTouchEvent::Sync {
                timestamp_micros: 30,
            })
            .expect("reused-slot down frame");
        assert_eq!(reused.phase, TouchPhase::Down);
        assert_eq!(
            reused.points,
            vec![TouchPoint {
                pointer_id: 6,
                x: 50,
                y: 60
            }],
            "a new tracking id in a reused slot must not carry over stale coordinates"
        );
    }
}
