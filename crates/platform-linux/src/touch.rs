//! Real multitouch capture from a Linux "protocol B" evdev touchscreen
//! (the official 7-inch DSI panel's `ft5406`/`raspberrypi-ts` driver, and
//! every other modern kernel touch driver, all use this protocol). Reads
//! run on a dedicated background thread — `evdev::Device::fetch_events`
//! blocks until the kernel has events, so a caller polling once per loop
//! iteration (like `auth-headunit-diagnostics`'s probe loop) needs a
//! non-blocking [`EvdevTouchSource::try_recv`] fed by that thread, not a
//! direct blocking call. The thread is never explicitly joined: this is a
//! short-lived diagnostic CLI, and the OS reclaims the thread when the
//! process exits, matching how the rest of this crate's process-lifetime
//! hardware handles (transport claims, TLS sessions) are already handled.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use evdev::{AbsoluteAxisCode, Device, EventSummary, SynchronizationCode};
use platform_api::{MultiTouchTracker, RawTouchEvent, TouchFrame};

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

/// A live touch source feeding [`TouchFrame`]s from a background reader
/// thread, scaled from the device's own reported coordinate range into
/// `target_width`/`target_height` — the same dimensions advertised to the
/// phone in `ServiceDiscoveryResponse`'s `TouchCapability`, since the phone
/// interprets `TouchEvent` coordinates in that space, not the touch
/// controller's native resolution.
pub struct EvdevTouchSource {
    receiver: mpsc::Receiver<TouchFrame>,
}

impl EvdevTouchSource {
    pub fn open(path: &Path, target_width: u32, target_height: u32) -> io::Result<Self> {
        let device = Device::open(path)?;
        let scale = AxisScale::from_device(&device, target_width, target_height)?;
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || run_reader(device, scale, &sender));
        Ok(Self { receiver })
    }

    /// Drains one queued frame, if any, without blocking — safe to call
    /// once per probe-loop iteration.
    #[must_use]
    pub fn try_recv(&self) -> Option<TouchFrame> {
        self.receiver.try_recv().ok()
    }
}

#[derive(Clone, Copy, Debug)]
struct AxisScale {
    x_min: i32,
    x_span: i32,
    y_min: i32,
    y_span: i32,
    target_width: u32,
    target_height: u32,
}

impl AxisScale {
    fn from_device(device: &Device, target_width: u32, target_height: u32) -> io::Result<Self> {
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
        })
    }

    fn scale_x(self, raw: i32) -> u32 {
        scale(raw, self.x_min, self.x_span, self.target_width)
    }

    fn scale_y(self, raw: i32) -> u32 {
        scale(raw, self.y_min, self.y_span, self.target_height)
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

fn run_reader(mut device: Device, scale: AxisScale, sender: &mpsc::Sender<TouchFrame>) {
    let mut tracker = MultiTouchTracker::new();
    loop {
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
                    Some(RawTouchEvent::PositionX(scale.scale_x(value)))
                }
                EventSummary::AbsoluteAxis(_, AbsoluteAxisCode::ABS_MT_POSITION_Y, value) => {
                    Some(RawTouchEvent::PositionY(scale.scale_y(value)))
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
}
