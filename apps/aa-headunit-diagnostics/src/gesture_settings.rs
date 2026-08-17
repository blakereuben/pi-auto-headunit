//! Persisted, user-reassignable mapping from a completed follow-up gesture
//! (`platform_api::GestureId`) to a head-unit action, plus the small,
//! closed action set itself.
//!
//! Lives in [`DEFAULT_SETTINGS_PATH`] (`/var/lib/aa-headunit/settings.toml`)
//! — mutable operator state, per `ARCHITECTURE.md` §8's `/etc/aa-headunit/`
//! (schema-validated admin config) vs `/var/lib/aa-headunit/` (mutable
//! state) split. `/etc/aa-headunit/config.toml` (credential paths,
//! wireless provider policy) is deliberately untouched — this is a
//! different file for a different, user-reassignable kind of setting.
//! `packaging/debian/aa-headunit-diagnostics.postinst` creates
//! `/var/lib/aa-headunit` (`root:aa-headunit`, `0770`), the same
//! unprivileged-group pattern already used for `/etc/aa-headunit`
//! (`packaging/README.md`), so saving settings never needs `sudo`.
//!
//! `GestureId`/`Action` deliberately don't derive `serde` traits — that
//! would pull a serialization dependency into `platform-api`, a pure
//! capability/data crate with none today (`ARCHITECTURE.md`'s dependency
//! rule). This module owns the string<->enum mapping itself instead.
//!
//! A missing, unreadable, or malformed settings file always falls back to
//! [`GestureSettings::defaults`] — settings are a convenience, never
//! allowed to fail a live session.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use platform_api::GestureId;
use serde::{Deserialize, Serialize};

pub const DEFAULT_SETTINGS_PATH: &str = "/var/lib/aa-headunit/settings.toml";
/// How long, after the four-finger arming swipe completes, a follow-up
/// gesture has to arrive before the head unit silently disarms. Real-
/// hardware feedback (2026-08-16): the operator asked for this to be
/// visible and adjustable, not a fixed constant only discoverable by
/// reading source — see `gtk_dev_ui.rs`'s armed-state mask overlay.
pub const DEFAULT_ARM_WINDOW_SECONDS: u32 = 3;
pub const MIN_ARM_WINDOW_SECONDS: u32 = 1;
pub const MAX_ARM_WINDOW_SECONDS: u32 = 30;

/// The small, closed set of actions a follow-up gesture can trigger.
/// Deliberately not an open plugin system — see `platform_api::gesture`'s
/// doc comment for the same reasoning applied to gestures themselves.
/// `ToggleFullscreen` is bidirectional (real-hardware feedback,
/// 2026-08-16: an earlier, one-directional "return to desktop" action left
/// the operator stuck with no gesture-driven way back to fullscreen video
/// once triggered — the settings panel's own "Close" button doesn't touch
/// window state at all, so it couldn't help either).
///
/// The four `SwitchTo*` actions send a car-specific `KeyCode` (`Media` /
/// `Navigation` / `Radio` / `Tel`) to the phone via
/// `protocol_aap::encode_key_event` — confirmed, sourced wire values
/// (`docs/protocol/aasdk-adoption.md`'s `KeyCode` section). Real-hardware-
/// confirmed, 2026-08-16 (`MILESTONE_CHECKLIST.md` M3): `Media`/
/// `Navigation`/`Tel` switch to whichever third-party app the phone has
/// set as default for that category (no approved source describes
/// launching a specific named app — this is category-switching only);
/// `Radio` is different — it navigates to Android Auto's own native
/// radio screen rather than any app (empty without a real tuner backend,
/// which this project deliberately doesn't implement — see
/// `protocol_aap::RadioCapability`'s doc comment).
///
/// `ScreenOff` (added 2026-08-17) is a **third** dispatch category,
/// distinct from both the phone-facing `SwitchTo*` actions above and the
/// GTK-thread-local `OpenSettings`/`ToggleFullscreen`/`CycleRotation`
/// actions below: it needs neither `transport` (no phone message) nor GTK
/// window state, but it must still run on the background protocol thread
/// specifically, because that thread's `service_touch_input` is the only
/// place that can swallow the touch used to wake the screen back up — see
/// `auth_discovery_probe.rs`'s `ScreenPowerState`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    OpenSettings,
    ToggleFullscreen,
    CycleRotation,
    SwitchToMedia,
    SwitchToNavigation,
    SwitchToRadio,
    SwitchToPhone,
    ScreenOff,
}

impl Action {
    #[must_use]
    pub const fn all() -> [Action; 8] {
        [
            Action::OpenSettings,
            Action::ToggleFullscreen,
            Action::CycleRotation,
            Action::SwitchToMedia,
            Action::SwitchToNavigation,
            Action::SwitchToRadio,
            Action::SwitchToPhone,
            Action::ScreenOff,
        ]
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Action::OpenSettings => "Open settings",
            Action::ToggleFullscreen => "Toggle fullscreen",
            Action::CycleRotation => "Cycle rotation",
            Action::SwitchToMedia => "Switch to media",
            Action::SwitchToNavigation => "Switch to navigation",
            Action::SwitchToRadio => "Switch to radio",
            Action::SwitchToPhone => "Switch to phone",
            Action::ScreenOff => "Screen off",
        }
    }

    /// The `KeyCode` a `SwitchTo*` action sends, if it is one — `None` for
    /// every other action (dispatched locally, no phone message).
    #[must_use]
    pub const fn key_code(self) -> Option<protocol_aap::KeyCode> {
        match self {
            Action::SwitchToMedia => Some(protocol_aap::KeyCode::Media),
            Action::SwitchToNavigation => Some(protocol_aap::KeyCode::Navigation),
            Action::SwitchToRadio => Some(protocol_aap::KeyCode::Radio),
            Action::SwitchToPhone => Some(protocol_aap::KeyCode::Tel),
            Action::OpenSettings
            | Action::ToggleFullscreen
            | Action::CycleRotation
            | Action::ScreenOff => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Action::OpenSettings => "open_settings",
            Action::ToggleFullscreen => "toggle_fullscreen",
            Action::CycleRotation => "cycle_rotation",
            Action::SwitchToMedia => "switch_to_media",
            Action::SwitchToNavigation => "switch_to_navigation",
            Action::SwitchToRadio => "switch_to_radio",
            Action::SwitchToPhone => "switch_to_phone",
            Action::ScreenOff => "screen_off",
        }
    }

    fn from_key(key: &str) -> Option<Action> {
        match key {
            "open_settings" => Some(Action::OpenSettings),
            "toggle_fullscreen" => Some(Action::ToggleFullscreen),
            "cycle_rotation" => Some(Action::CycleRotation),
            "switch_to_media" => Some(Action::SwitchToMedia),
            "switch_to_navigation" => Some(Action::SwitchToNavigation),
            "switch_to_radio" => Some(Action::SwitchToRadio),
            "switch_to_phone" => Some(Action::SwitchToPhone),
            "screen_off" => Some(Action::ScreenOff),
            _ => None,
        }
    }
}

#[must_use]
pub fn gesture_label(gesture: GestureId) -> &'static str {
    match gesture {
        GestureId::DoubleTap => "Double-tap",
        GestureId::TwoFingerTap => "Two-finger tap",
        GestureId::LongPress => "Long press",
        GestureId::SwipeUp => "Swipe up",
        GestureId::SwipeDown => "Swipe down",
        GestureId::SwipeLeft => "Swipe left",
        GestureId::SwipeRight => "Swipe right",
        GestureId::Circle => "Circle / spiral",
    }
}

#[derive(Default, Serialize, Deserialize)]
struct RawSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    double_tap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    two_finger_tap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    long_press: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    swipe_up: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    swipe_down: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    swipe_left: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    swipe_right: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    circle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arm_window_seconds: Option<u32>,
}

impl RawSettings {
    fn get(&self, gesture: GestureId) -> Option<&str> {
        match gesture {
            GestureId::DoubleTap => self.double_tap.as_deref(),
            GestureId::TwoFingerTap => self.two_finger_tap.as_deref(),
            GestureId::LongPress => self.long_press.as_deref(),
            GestureId::SwipeUp => self.swipe_up.as_deref(),
            GestureId::SwipeDown => self.swipe_down.as_deref(),
            GestureId::SwipeLeft => self.swipe_left.as_deref(),
            GestureId::SwipeRight => self.swipe_right.as_deref(),
            GestureId::Circle => self.circle.as_deref(),
        }
    }

    fn set(&mut self, gesture: GestureId, action: Action) {
        let value = Some(action.key().to_string());
        match gesture {
            GestureId::DoubleTap => self.double_tap = value,
            GestureId::TwoFingerTap => self.two_finger_tap = value,
            GestureId::LongPress => self.long_press = value,
            GestureId::SwipeUp => self.swipe_up = value,
            GestureId::SwipeDown => self.swipe_down = value,
            GestureId::SwipeLeft => self.swipe_left = value,
            GestureId::SwipeRight => self.swipe_right = value,
            GestureId::Circle => self.circle = value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GestureSettings {
    mappings: HashMap<GestureId, Action>,
    arm_window_seconds: u32,
}

impl GestureSettings {
    /// Double-tap opens settings (the discoverable, always-safe default);
    /// two-finger tap toggles fullscreen; long press cycles rotation; three
    /// of the four swipe directions default to category-switch actions in a
    /// spatially obvious layout (up→navigation, down→media, matching how a
    /// map sits above and music sits below on a typical AA home screen;
    /// left→phone) — chosen so every gesture does something distinct out of
    /// the box, not because any one mapping is more "correct" than another.
    /// Swipe right defaults to `ToggleFullscreen` (redundant with
    /// two-finger tap, but genuinely functional) rather than
    /// `SwitchToRadio`: `SwitchToRadio` is real-hardware-confirmed to
    /// correctly navigate to Android Auto's own native radio screen (see
    /// `protocol_aap::RadioCapability`'s doc comment), but that screen is
    /// empty without a real tuner backend this project deliberately
    /// doesn't implement — not a good default for a fresh install.
    /// `SwitchToRadio` is still a fully selectable, working action.
    /// `Circle` defaults to `ScreenOff` — the gesture this action was
    /// added for (2026-08-17), originally a triple-finger double-tap
    /// replaced 2026-08-18 after real-hardware testing found three-finger
    /// coordination unreliable (see `platform_api::gesture`'s doc
    /// comment).
    #[must_use]
    pub fn defaults() -> Self {
        let mut mappings = HashMap::new();
        mappings.insert(GestureId::DoubleTap, Action::OpenSettings);
        mappings.insert(GestureId::TwoFingerTap, Action::ToggleFullscreen);
        mappings.insert(GestureId::LongPress, Action::CycleRotation);
        mappings.insert(GestureId::SwipeUp, Action::SwitchToNavigation);
        mappings.insert(GestureId::SwipeDown, Action::SwitchToMedia);
        mappings.insert(GestureId::SwipeLeft, Action::SwitchToPhone);
        mappings.insert(GestureId::SwipeRight, Action::ToggleFullscreen);
        mappings.insert(GestureId::Circle, Action::ScreenOff);
        Self {
            mappings,
            arm_window_seconds: DEFAULT_ARM_WINDOW_SECONDS,
        }
    }

    #[must_use]
    pub fn action_for(&self, gesture: GestureId) -> Action {
        self.mappings
            .get(&gesture)
            .copied()
            .unwrap_or(Action::OpenSettings)
    }

    pub fn set_action(&mut self, gesture: GestureId, action: Action) {
        self.mappings.insert(gesture, action);
    }

    #[must_use]
    pub fn arm_window_seconds(&self) -> u32 {
        self.arm_window_seconds
    }

    /// Clamped to [`MIN_ARM_WINDOW_SECONDS`]..=[`MAX_ARM_WINDOW_SECONDS`] —
    /// zero would arm and instantly disarm on the same frame, and an
    /// unbounded value defeats the point of a timeout.
    pub fn set_arm_window_seconds(&mut self, seconds: u32) {
        self.arm_window_seconds = seconds.clamp(MIN_ARM_WINDOW_SECONDS, MAX_ARM_WINDOW_SECONDS);
    }

    /// Loads from `path`; falls back to [`Self::defaults`] on any error
    /// (missing file, unreadable, malformed, or an unrecognized
    /// gesture/action key — forward/backward compatible with a future
    /// gesture or action this build doesn't know about, which is simply
    /// ignored rather than treated as corruption).
    #[must_use]
    pub fn load(path: &Path) -> Self {
        Self::try_load(path).unwrap_or_else(Self::defaults)
    }

    fn try_load(path: &Path) -> Option<Self> {
        let text = fs::read_to_string(path).ok()?;
        let raw: RawSettings = toml::from_str(&text).ok()?;
        let mut settings = Self::defaults();
        for gesture in GestureId::all() {
            if let Some(action) = raw.get(gesture).and_then(Action::from_key) {
                settings.set_action(gesture, action);
            }
        }
        if let Some(seconds) = raw.arm_window_seconds {
            settings.set_arm_window_seconds(seconds);
        }
        Some(settings)
    }

    /// Creates `path`'s parent directory if needed (matches
    /// `/var/lib/aa-headunit` already existing with group-writable
    /// permissions from packaging — this only ever needs `create_dir_all`
    /// for a fresh `/var/lib/aa-headunit` that already has the right
    /// parent permissions, never for `/var/lib` itself).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut raw = RawSettings::default();
        for gesture in GestureId::all() {
            raw.set(gesture, self.action_for(gesture));
        }
        raw.arm_window_seconds = Some(self.arm_window_seconds);
        let text = toml::to_string_pretty(&raw)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_assign_every_gesture_a_functional_action() {
        let settings = GestureSettings::defaults();
        assert_eq!(
            settings.action_for(GestureId::DoubleTap),
            Action::OpenSettings
        );
        assert_eq!(
            settings.action_for(GestureId::TwoFingerTap),
            Action::ToggleFullscreen
        );
        assert_eq!(
            settings.action_for(GestureId::LongPress),
            Action::CycleRotation
        );
        assert_eq!(
            settings.action_for(GestureId::SwipeUp),
            Action::SwitchToNavigation
        );
        assert_eq!(
            settings.action_for(GestureId::SwipeDown),
            Action::SwitchToMedia
        );
        assert_eq!(
            settings.action_for(GestureId::SwipeLeft),
            Action::SwitchToPhone
        );
        // Not `SwitchToRadio` — it works, but navigates to an empty
        // native radio screen without real tuner hardware, see
        // `defaults`'s own doc comment.
        assert_eq!(
            settings.action_for(GestureId::SwipeRight),
            Action::ToggleFullscreen
        );
        assert_eq!(settings.action_for(GestureId::Circle), Action::ScreenOff);
    }

    #[test]
    fn every_switch_to_action_carries_the_expected_key_code() {
        assert_eq!(
            Action::SwitchToMedia.key_code(),
            Some(protocol_aap::KeyCode::Media)
        );
        assert_eq!(
            Action::SwitchToNavigation.key_code(),
            Some(protocol_aap::KeyCode::Navigation)
        );
        assert_eq!(
            Action::SwitchToRadio.key_code(),
            Some(protocol_aap::KeyCode::Radio)
        );
        assert_eq!(
            Action::SwitchToPhone.key_code(),
            Some(protocol_aap::KeyCode::Tel)
        );
        assert_eq!(Action::OpenSettings.key_code(), None);
        assert_eq!(Action::ToggleFullscreen.key_code(), None);
        assert_eq!(Action::CycleRotation.key_code(), None);
        assert_eq!(Action::ScreenOff.key_code(), None);
    }

    #[test]
    fn a_circle_gesture_reassignment_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-gesture-settings-circle-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = GestureSettings::defaults();
        settings.set_action(GestureId::Circle, Action::OpenSettings);
        settings.save(&path).expect("save succeeds");

        let loaded = GestureSettings::load(&path);
        assert_eq!(loaded.action_for(GestureId::Circle), Action::OpenSettings);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_swipe_gesture_reassignment_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-gesture-settings-swipe-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = GestureSettings::defaults();
        settings.set_action(GestureId::SwipeUp, Action::SwitchToRadio);
        settings.save(&path).expect("save succeeds");

        let loaded = GestureSettings::load(&path);
        assert_eq!(loaded.action_for(GestureId::SwipeUp), Action::SwitchToRadio);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_load_round_trips_a_reassignment() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-gesture-settings-test-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = GestureSettings::defaults();
        settings.set_action(GestureId::DoubleTap, Action::CycleRotation);
        settings.save(&path).expect("save succeeds");

        let loaded = GestureSettings::load(&path);
        assert_eq!(
            loaded.action_for(GestureId::DoubleTap),
            Action::CycleRotation
        );
        // Untouched mappings still round-trip correctly.
        assert_eq!(
            loaded.action_for(GestureId::TwoFingerTap),
            Action::ToggleFullscreen
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_falls_back_to_defaults_when_the_file_is_missing() {
        let path = Path::new("/nonexistent/aa-headunit-settings-test.toml");
        let settings = GestureSettings::load(path);
        assert_eq!(settings, GestureSettings::defaults());
    }

    #[test]
    fn load_falls_back_to_defaults_on_malformed_content() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-gesture-settings-malformed-{}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir creates");
        let path = dir.join("settings.toml");
        fs::write(&path, "not valid toml {{{").expect("write succeeds");

        let settings = GestureSettings::load(&path);
        assert_eq!(settings, GestureSettings::defaults());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn arm_window_seconds_round_trips_and_persists() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-gesture-settings-arm-window-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = GestureSettings::defaults();
        assert_eq!(settings.arm_window_seconds(), DEFAULT_ARM_WINDOW_SECONDS);
        settings.set_arm_window_seconds(10);
        settings.save(&path).expect("save succeeds");

        let loaded = GestureSettings::load(&path);
        assert_eq!(loaded.arm_window_seconds(), 10);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn arm_window_seconds_clamps_to_the_allowed_range() {
        let mut settings = GestureSettings::defaults();
        settings.set_arm_window_seconds(0);
        assert_eq!(settings.arm_window_seconds(), MIN_ARM_WINDOW_SECONDS);
        settings.set_arm_window_seconds(1000);
        assert_eq!(settings.arm_window_seconds(), MAX_ARM_WINDOW_SECONDS);
    }

    #[test]
    fn gesture_label_is_distinct_and_readable_per_gesture() {
        assert_eq!(gesture_label(GestureId::DoubleTap), "Double-tap");
        assert_eq!(gesture_label(GestureId::TwoFingerTap), "Two-finger tap");
        assert_eq!(gesture_label(GestureId::LongPress), "Long press");
    }
}
