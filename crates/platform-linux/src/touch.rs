//! Real multitouch capture from a Linux "protocol B" evdev touchscreen
//! (the official 7-inch DSI panel's `ft5406`/`raspberrypi-ts` driver, and
//! every other modern kernel touch driver, all use this protocol). Reads
//! run on a dedicated background thread — `evdev::Device::fetch_events`
//! blocks until the kernel has events, so a caller polling once per loop
//! iteration (like `auth-headunit-diagnostics`'s probe loop) needs a
//! non-blocking [`EvdevTouchSource::try_recv`] fed by that thread, not a
//! direct blocking call.
//!
//! **The reader thread is explicitly stopped and joined on
//! [`EvdevTouchSource`]'s `Drop`** — this used to be fire-and-forget
//! ("the OS reclaims the thread when the process exits"), which was true
//! for the original one-shot diagnostic CLI this was built for, but broke
//! the moment `usb session-supervisor` (a long-running process that opens
//! and drops a fresh `EvdevTouchSource` every reconnect cycle) started
//! using it. Real-hardware finding, 2026-08-18: a 100-cycle
//! connect/disconnect soak with nobody touching the screen leaked exactly
//! one open file descriptor per cycle (`/dev/input/event6`, confirmed via
//! `strace -e trace=open,openat,close`) — `run_reader`'s old loop only
//! ever checked whether its channel receiver was gone at the point it
//! tried to `send` a *completed* touch frame, so with zero touch input
//! during a cycle it stayed blocked in `fetch_events()` forever, and the
//! `Device` (and its fd) it owned never dropped. Fixed by polling the
//! device's fd with a bounded timeout
//! ([`READER_POLL_TIMEOUT_MILLIS`]) instead of calling the blocking
//! `fetch_events()` directly, checking a shared stop flag on every
//! timeout — [`EvdevTouchSource::drop`] sets that flag and joins the
//! thread, so the fd is guaranteed closed by the time `drop` returns, not
//! just eventually. Zero latency cost for real touch input: `poll`
//! returns immediately the instant data is ready, the timeout only ever
//! elapses while idle.

use std::io;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use evdev::{AbsoluteAxisCode, Device, EventSummary, SynchronizationCode};
use nix::poll::{PollFd, PollFlags, poll};
use platform_api::{MultiTouchTracker, RawTouchEvent, TouchFrame};

/// How long [`run_reader`]'s poll loop waits for touch data before
/// re-checking the shutdown flag. Bounds worst-case shutdown latency
/// without adding any latency to real touch input (see this module's own
/// doc comment) — short enough that a `Drop`/join never feels slow, long
/// enough not to spin the CPU polling an idle touchscreen.
const READER_POLL_TIMEOUT_MILLIS: u16 = 200;

/// Finds the first connected evdev device that reports both multitouch
/// position axes — sufficient to identify a touchscreen without depending
/// on any particular kernel-assigned device name (which varies across
/// kernel versions and isn't guaranteed stable).
pub fn discover_touchscreen() -> io::Result<Option<PathBuf>> {
    for (path, device) in evdev::enumerate() {
        let Some(axes) = device.supported_absolute_axes() else {
            continue;
        };
        if axes.contains(AbsoluteAxisCode::ABS_MT_POSITION_X)
            && axes.contains(AbsoluteAxisCode::ABS_MT_POSITION_Y)
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// How the touch panel is physically mounted relative to how video is
/// rendered, matching Wayland's `wl_output.transform` convention (rotation
/// is anti-clockwise) so a compositor-side `wlr-randr --transform` and this
/// rotation can be set to the same value and stay visually consistent.
/// `Rotate90`/`Rotate270` swap which raw axis feeds the target X vs Y
/// coordinate; all four also reverse one or both axes as needed so the
/// rotated mapping still lands on the correct rendered pixel. Real-hardware
/// verification of anything but `Rotate0` needs the DSI panel actually
/// mounted rotated (not available on this project's reference rig — see
/// `MILESTONE_CHECKLIST.md` M3's touch item) or a software-only check:
/// rotate the Wayland output digitally (`wlr-randr --transform`) to match,
/// then confirm taps land correctly.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Rotation {
    Rotate0,
    Rotate90,
    Rotate180,
    Rotate270,
}

impl Rotation {
    fn encode(self) -> u8 {
        match self {
            Self::Rotate0 => 0,
            Self::Rotate90 => 1,
            Self::Rotate180 => 2,
            Self::Rotate270 => 3,
        }
    }

    fn decode(value: u8) -> Self {
        match value {
            1 => Self::Rotate90,
            2 => Self::Rotate180,
            3 => Self::Rotate270,
            _ => Self::Rotate0,
        }
    }
}

/// A [`Rotation`] that can be changed live while the reader thread it's
/// shared with keeps running — built for the head unit's own settings
/// panel (`gtk_dev_ui.rs`), where an operator picks a rotation while a
/// session is already active rather than only at startup. Cheap to clone
/// (an `Arc` around a single atomic byte); `set` takes effect on the very
/// next raw touch sample the reader thread processes, with no restart.
#[derive(Clone)]
pub struct SharedRotation(Arc<AtomicU8>);

impl SharedRotation {
    #[must_use]
    pub fn new(initial: Rotation) -> Self {
        Self(Arc::new(AtomicU8::new(initial.encode())))
    }

    pub fn set(&self, rotation: Rotation) {
        self.0.store(rotation.encode(), Ordering::SeqCst);
    }

    fn get(&self) -> Rotation {
        Rotation::decode(self.0.load(Ordering::SeqCst))
    }
}

/// A live touch source feeding [`TouchFrame`]s from a background reader
/// thread, scaled from the device's own reported coordinate range into
/// `target_width`/`target_height` — the same dimensions advertised to the
/// phone in `ServiceDiscoveryResponse`'s `TouchCapability`, since the phone
/// interprets `TouchEvent` coordinates in that space, not the touch
/// controller's native resolution. `rotation` compensates for how the panel
/// is physically mounted; `target_width`/`target_height` never change with
/// rotation, since they describe the phone's own video frame, not the
/// panel.
pub struct EvdevTouchSource {
    receiver: mpsc::Receiver<TouchFrame>,
    rotation: SharedRotation,
    stop: Arc<AtomicBool>,
    reader: Option<thread::JoinHandle<()>>,
}

impl EvdevTouchSource {
    pub fn open(
        path: &Path,
        target_width: u32,
        target_height: u32,
        rotation: Rotation,
    ) -> io::Result<Self> {
        let device = Device::open(path)?;
        let scale = AxisScale::from_device(&device, target_width, target_height, rotation)?;
        let rotation_handle = scale.rotation.clone();
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_reader = Arc::clone(&stop);
        let reader = thread::spawn(move || run_reader(device, &scale, &sender, &stop_for_reader));
        Ok(Self {
            receiver,
            rotation: rotation_handle,
            stop,
            reader: Some(reader),
        })
    }

    /// Drains one queued frame, if any, without blocking — safe to call
    /// once per probe-loop iteration.
    #[must_use]
    pub fn try_recv(&self) -> Option<TouchFrame> {
        self.receiver.try_recv().ok()
    }

    /// A cloneable handle that can change this already-running source's
    /// rotation live — see [`SharedRotation`].
    #[must_use]
    pub fn rotation_handle(&self) -> SharedRotation {
        self.rotation.clone()
    }
}

impl Drop for EvdevTouchSource {
    /// Stops and joins the reader thread so its `Device` (and the fd it
    /// owns) is guaranteed closed before this returns — see this module's
    /// own doc comment for the real-hardware leak this fixes.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[derive(Clone)]
struct AxisScale {
    x_min: i32,
    x_span: i32,
    y_min: i32,
    y_span: i32,
    target_width: u32,
    target_height: u32,
    rotation: SharedRotation,
}

impl AxisScale {
    fn from_device(
        device: &Device,
        target_width: u32,
        target_height: u32,
        rotation: Rotation,
    ) -> io::Result<Self> {
        let x_info = device
            .get_absinfo()?
            .find(|(code, _)| *code == AbsoluteAxisCode::ABS_MT_POSITION_X)
            .map(|(_, info)| info)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "device has no ABS_MT_POSITION_X",
                )
            })?;
        let y_info = device
            .get_absinfo()?
            .find(|(code, _)| *code == AbsoluteAxisCode::ABS_MT_POSITION_Y)
            .map(|(_, info)| info)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "device has no ABS_MT_POSITION_Y",
                )
            })?;
        Ok(Self {
            x_min: x_info.minimum(),
            x_span: (x_info.maximum() - x_info.minimum()).max(1),
            y_min: y_info.minimum(),
            y_span: (y_info.maximum() - y_info.minimum()).max(1),
            target_width,
            target_height,
            rotation: SharedRotation::new(rotation),
        })
    }

    /// Builds the correctly-tagged event for a raw `ABS_MT_POSITION_X`
    /// sample. Under `Rotate0`/`Rotate180` this still feeds the target X
    /// coordinate; under `Rotate90`/`Rotate270` the raw X axis actually
    /// determines the target *Y* coordinate instead, since the panel's own
    /// X/Y axes are swapped relative to the rendered video once rotated —
    /// see [`Rotation`]'s doc comment. Reads the current rotation fresh on
    /// every call, so a live change via [`SharedRotation::set`] takes
    /// effect on the very next sample.
    fn event_for_position_x(&self, raw: i32) -> RawTouchEvent {
        let scaled_to_width = scale(raw, self.x_min, self.x_span, self.target_width);
        let scaled_to_height = scale(raw, self.x_min, self.x_span, self.target_height);
        match self.rotation.get() {
            Rotation::Rotate0 => RawTouchEvent::PositionX(scaled_to_width),
            Rotation::Rotate180 => {
                RawTouchEvent::PositionX(reverse(scaled_to_width, self.target_width))
            }
            Rotation::Rotate90 => {
                RawTouchEvent::PositionY(reverse(scaled_to_height, self.target_height))
            }
            Rotation::Rotate270 => RawTouchEvent::PositionY(scaled_to_height),
        }
    }

    /// Mirrors [`Self::event_for_position_x`] for a raw `ABS_MT_POSITION_Y`
    /// sample.
    fn event_for_position_y(&self, raw: i32) -> RawTouchEvent {
        let scaled_to_height = scale(raw, self.y_min, self.y_span, self.target_height);
        let scaled_to_width = scale(raw, self.y_min, self.y_span, self.target_width);
        match self.rotation.get() {
            Rotation::Rotate0 => RawTouchEvent::PositionY(scaled_to_height),
            Rotation::Rotate180 => {
                RawTouchEvent::PositionY(reverse(scaled_to_height, self.target_height))
            }
            Rotation::Rotate90 => RawTouchEvent::PositionX(scaled_to_width),
            Rotation::Rotate270 => {
                RawTouchEvent::PositionX(reverse(scaled_to_width, self.target_width))
            }
        }
    }
}

fn scale(raw: i32, min: i32, span: i32, target: u32) -> u32 {
    let clamped = raw.clamp(min, min + span);
    let offset = u64::from((clamped - min).unsigned_abs());
    let span = u64::from(span.unsigned_abs());
    let scaled = offset * u64::from(target.saturating_sub(1)) / span;
    #[allow(clippy::cast_possible_truncation)]
    let scaled = scaled as u32;
    scaled.min(target.saturating_sub(1))
}

fn reverse(value: u32, target: u32) -> u32 {
    target.saturating_sub(1).saturating_sub(value)
}

fn run_reader(
    mut device: Device,
    scale: &AxisScale,
    sender: &mpsc::Sender<TouchFrame>,
    stop: &AtomicBool,
) {
    let mut tracker = MultiTouchTracker::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let mut poll_fds = [PollFd::new(device.as_fd(), PollFlags::POLLIN)];
        match poll(&mut poll_fds, READER_POLL_TIMEOUT_MILLIS) {
            Ok(0) => continue,
            Ok(_) => {}
            Err(_) => return,
        }
        let Ok(events) = device.fetch_events() else {
            return;
        };
        for event in events {
            let timestamp_micros = micros_since_epoch(event.timestamp());
            let raw = match event.destructure() {
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_SLOT, value) => {
                    Some(RawTouchEvent::Slot(value.unsigned_abs()))
                }
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_TRACKING_ID, value) => Some(
                    RawTouchEvent::TrackingId((value >= 0).then(|| value.unsigned_abs())),
                ),
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_POSITION_X, value) => {
                    Some(scale.event_for_position_x(value))
                }
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_POSITION_Y, value) => {
                    Some(scale.event_for_position_y(value))
                }
                EventSummary::Synchronization(_, SynchronizationCode::SYN_REPORT, _) => {
                    Some(RawTouchEvent::Sync { timestamp_micros })
                }
                _ => None,
            };
            let Some(raw) = raw else {
                continue;
            };
            if let Some(frame) = tracker.push(raw) {
                if sender.send(frame).is_err() {
                    return;
                }
            }
        }
    }
}

fn micros_since_epoch(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_micros()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_maps_the_full_native_range_onto_the_target_range() {
        assert_eq!(scale(0, 0, 4095, 800), 0);
        assert_eq!(scale(4095, 0, 4095, 800), 799);
        assert_eq!(scale(4095 / 2, 0, 4095, 800), 399);
    }

    #[test]
    fn scale_clamps_out_of_range_input() {
        assert_eq!(scale(-10, 0, 4095, 800), 0);
        assert_eq!(scale(5000, 0, 4095, 800), 799);
    }

    fn axis_scale(rotation: Rotation) -> AxisScale {
        AxisScale {
            x_min: 0,
            x_span: 4095,
            y_min: 0,
            y_span: 4095,
            target_width: 800,
            target_height: 480,
            rotation: SharedRotation::new(rotation),
        }
    }

    #[test]
    fn rotate0_leaves_axes_unchanged() {
        let scale = axis_scale(Rotation::Rotate0);
        assert_eq!(scale.event_for_position_x(0), RawTouchEvent::PositionX(0));
        assert_eq!(
            scale.event_for_position_x(4095),
            RawTouchEvent::PositionX(799)
        );
        assert_eq!(scale.event_for_position_y(0), RawTouchEvent::PositionY(0));
        assert_eq!(
            scale.event_for_position_y(4095),
            RawTouchEvent::PositionY(479)
        );
    }

    #[test]
    fn rotate180_reverses_both_axes_without_swapping_them() {
        let scale = axis_scale(Rotation::Rotate180);
        assert_eq!(scale.event_for_position_x(0), RawTouchEvent::PositionX(799));
        assert_eq!(
            scale.event_for_position_x(4095),
            RawTouchEvent::PositionX(0)
        );
        assert_eq!(scale.event_for_position_y(0), RawTouchEvent::PositionY(479));
        assert_eq!(
            scale.event_for_position_y(4095),
            RawTouchEvent::PositionY(0)
        );
    }

    #[test]
    fn rotate90_swaps_axes() {
        let scale = axis_scale(Rotation::Rotate90);
        // Raw X now determines target Y, raw Y now determines target X.
        assert_eq!(scale.event_for_position_x(0), RawTouchEvent::PositionY(479));
        assert_eq!(
            scale.event_for_position_x(4095),
            RawTouchEvent::PositionY(0)
        );
        assert_eq!(scale.event_for_position_y(0), RawTouchEvent::PositionX(0));
        assert_eq!(
            scale.event_for_position_y(4095),
            RawTouchEvent::PositionX(799)
        );
    }

    #[test]
    fn rotate270_swaps_axes_the_other_way_from_rotate90() {
        let scale = axis_scale(Rotation::Rotate270);
        assert_eq!(scale.event_for_position_x(0), RawTouchEvent::PositionY(0));
        assert_eq!(
            scale.event_for_position_x(4095),
            RawTouchEvent::PositionY(479)
        );
        assert_eq!(scale.event_for_position_y(0), RawTouchEvent::PositionX(799));
        assert_eq!(
            scale.event_for_position_y(4095),
            RawTouchEvent::PositionX(0)
        );
    }

    #[test]
    fn all_four_rotations_are_pairwise_distinct_for_the_same_input() {
        // A real regression this guards against: accidentally implementing
        // two rotation cases identically (e.g. copy-paste leaving Rotate270
        // behaving like Rotate90) would silently pass every other test
        // above, since each is checked in isolation.
        let corner = (4095, 0);
        let outputs: Vec<(RawTouchEvent, RawTouchEvent)> = [
            Rotation::Rotate0,
            Rotation::Rotate90,
            Rotation::Rotate180,
            Rotation::Rotate270,
        ]
        .into_iter()
        .map(|rotation| {
            let scale = axis_scale(rotation);
            (
                scale.event_for_position_x(corner.0),
                scale.event_for_position_y(corner.1),
            )
        })
        .collect();
        for (index, pair) in outputs.iter().enumerate() {
            for (other_index, other_pair) in outputs.iter().enumerate() {
                if index != other_index {
                    assert_ne!(
                        pair, other_pair,
                        "rotations at indices {index} and {other_index} produced identical output"
                    );
                }
            }
        }
    }
}
