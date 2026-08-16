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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    OpenSettings,
    ToggleFullscreen,
    CycleRotation,
}

impl Action {
    #[must_use]
    pub const fn all() -> [Action; 3] {
        [
            Action::OpenSettings,
            Action::ToggleFullscreen,
            Action::CycleRotation,
        ]
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Action::OpenSettings => "Open settings",
            Action::ToggleFullscreen => "Toggle fullscreen",
            Action::CycleRotation => "Cycle rotation",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Action::OpenSettings => "open_settings",
            Action::ToggleFullscreen => "toggle_fullscreen",
            Action::CycleRotation => "cycle_rotation",
        }
    }

    fn from_key(key: &str) -> Option<Action> {
        match key {
            "open_settings" => Some(Action::OpenSettings),
            "toggle_fullscreen" => Some(Action::ToggleFullscreen),
            "cycle_rotation" => Some(Action::CycleRotation),
            _ => None,
        }
    }
}

#[must_use]
pub fn gesture_label(gesture: GestureId) -> &'static str {
    match gesture {
        GestureId::DoubleTap => "Double-tap",
        GestureId::ThreeFingerTap => "Three-finger tap",
        GestureId::LongPress => "Long press",
    }
}

#[derive(Default, Serialize, Deserialize)]
struct RawSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    double_tap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    three_finger_tap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    long_press: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arm_window_seconds: Option<u32>,
}

impl RawSettings {
    fn get(&self, gesture: GestureId) -> Option<&str> {
        match gesture {
            GestureId::DoubleTap => self.double_tap.as_deref(),
            GestureId::ThreeFingerTap => self.three_finger_tap.as_deref(),
            GestureId::LongPress => self.long_press.as_deref(),
        }
    }

    fn set(&mut self, gesture: GestureId, action: Action) {
        let value = Some(action.key().to_string());
        match gesture {
            GestureId::DoubleTap => self.double_tap = value,
            GestureId::ThreeFingerTap => self.three_finger_tap = value,
            GestureId::LongPress => self.long_press = value,
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
    /// three-finger tap toggles fullscreen; long press cycles rotation —
    /// chosen so every gesture does something distinct out of the box,
    /// not because any one mapping is more "correct" than another. Fully
    /// reassignable afterward.
    #[must_use]
    pub fn defaults() -> Self {
        let mut mappings = HashMap::new();
        mappings.insert(GestureId::DoubleTap, Action::OpenSettings);
        mappings.insert(GestureId::ThreeFingerTap, Action::ToggleFullscreen);
        mappings.insert(GestureId::LongPress, Action::CycleRotation);
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
    fn defaults_assign_every_gesture_a_distinct_action() {
        let settings = GestureSettings::defaults();
        assert_eq!(
            settings.action_for(GestureId::DoubleTap),
            Action::OpenSettings
        );
        assert_eq!(
            settings.action_for(GestureId::ThreeFingerTap),
            Action::ToggleFullscreen
        );
        assert_eq!(
            settings.action_for(GestureId::LongPress),
            Action::CycleRotation
        );
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
            loaded.action_for(GestureId::ThreeFingerTap),
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
        assert_eq!(gesture_label(GestureId::ThreeFingerTap), "Three-finger tap");
        assert_eq!(gesture_label(GestureId::LongPress), "Long press");
    }
}
