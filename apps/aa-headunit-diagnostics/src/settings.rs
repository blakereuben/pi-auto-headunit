//! Persisted operator settings: the gesture-to-action mapping this module
//! started as (`platform_api::GestureId` → the small, closed [`Action`]
//! set), plus, since M5 (`MILESTONE_CHECKLIST.md`, "Add persistent
//! settings for display, rotation, touch, audio, microphone, Wi-Fi, and
//! Bluetooth"), touch rotation, display brightness, audio output/
//! microphone input device selection, and — as of the same milestone —
//! the preferred Wi-Fi/Bluetooth *radio provider*
//! (`platform_api::ProviderPreference`: `Auto`/`Onboard`/a named USB
//! adapter's stable ID, the same choice `usb wireless --wifi`/
//! `--bluetooth` already accepted as a one-shot flag). This is a
//! narrower thing than a "preferred network": it's which physical
//! radio to use, not which SSID/paired device to connect to.
//! `wireless_bootstrap.rs` (M7's wireless Android Auto `SoftAP`
//! bootstrap) still has no "preferred network" concept to persist
//! against — it generates a fresh random `SoftAP` SSID/password on
//! every bootstrap — so that remains out of scope here.
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
//! `GestureId`/`Action`/`platform_linux::touch::Rotation` deliberately
//! don't derive `serde` traits — that would pull a serialization
//! dependency into `platform-api`/`platform-linux`, pure capability/data
//! crates with none today (`ARCHITECTURE.md`'s dependency rule). This
//! module owns every string<->enum mapping itself instead, matching the
//! existing pattern `auth_discovery_probe.rs`'s `TOUCH_ROTATION_ENV_VAR`
//! parsing already established for rotation specifically.
//!
//! A missing, unreadable, or malformed settings file always falls back to
//! [`HeadUnitSettings::defaults`] — settings are a convenience, never
//! allowed to fail a live session.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use platform_api::GestureId;
use platform_linux::touch::Rotation;
use serde::{Deserialize, Serialize};

pub const DEFAULT_SETTINGS_PATH: &str = "/var/lib/aa-headunit/settings.toml";
/// Backlight level applied whenever the screen is on — `100` (full
/// brightness) matches this project's pre-M5 behaviour exactly (the
/// screen-off/on gesture previously only ever wrote `0` or
/// `max_brightness`), so a fresh install's actual on-screen behaviour is
/// unchanged until an operator deliberately lowers it.
pub const DEFAULT_DISPLAY_BRIGHTNESS_PERCENT: u8 = 100;
/// `0` is deliberately excluded from the adjustable range — indistinguishable
/// from the screen being off via the separate `ScreenOff` gesture action,
/// and a saved `0` would leave a fresh boot with an unrecoverable-looking
/// dark screen until the operator finds the (also on-screen) settings
/// control to fix it.
pub const MIN_DISPLAY_BRIGHTNESS_PERCENT: u8 = 1;
pub const MAX_DISPLAY_BRIGHTNESS_PERCENT: u8 = 100;
/// How long, after the four-finger arming swipe completes, a follow-up
/// gesture has to arrive before the head unit silently disarms. Real-
/// hardware feedback (2026-08-16): the operator asked for this to be
/// visible and adjustable, not a fixed constant only discoverable by
/// reading source — see `gtk_dev_ui.rs`'s armed-state mask overlay.
pub const DEFAULT_ARM_WINDOW_SECONDS: u32 = 3;
pub const MIN_ARM_WINDOW_SECONDS: u32 = 1;
pub const MAX_ARM_WINDOW_SECONDS: u32 = 30;
/// The Raspberry Pi 40-pin header's usable BCM GPIO line range (0/1 are
/// technically wired too — the ID EEPROM pins — but are reserved and
/// excluded here since selecting them for night mode would conflict with
/// board identification hardware).
pub const MIN_NIGHT_MODE_GPIO_LINE: u32 = 2;
pub const MAX_NIGHT_MODE_GPIO_LINE: u32 = 27;

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
/// GTK-thread-local `OpenSettings`/`ToggleFullscreen`/`FlipScreen`
/// actions below: it needs neither `transport` (no phone message) nor GTK
/// window state, but it must still run on the background protocol thread
/// specifically, because that thread's `service_touch_input` is the only
/// place that can swallow the touch used to wake the screen back up — see
/// `auth_discovery_probe.rs`'s `ScreenPowerState`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    OpenSettings,
    ToggleFullscreen,
    FlipScreen,
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
            Action::FlipScreen,
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
            Action::FlipScreen => "Flip screen",
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
            | Action::FlipScreen
            | Action::ScreenOff => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Action::OpenSettings => "open_settings",
            Action::ToggleFullscreen => "toggle_fullscreen",
            Action::FlipScreen => "flip_screen",
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
            "flip_screen" => Some(Action::FlipScreen),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    suppress_phone_mtp_popups: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rotation_degrees: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    display_brightness_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_output_device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    microphone_input_device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    night_mode_gpio_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wifi_preference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bluetooth_preference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    launch_on_boot: Option<bool>,
}

/// `ProviderPreference` (`platform_api`) has no `Display`/serialization
/// of its own — this module owns its own wire mapping instead, matching
/// the same choice already made for `Rotation`/`GestureId`/`Action` (see
/// this module's own doc comment). `ProviderPreference::parse` already
/// defines the string→enum direction (`"auto"`/`"onboard"`/anything
/// else as a stable adapter ID) exactly as `usb wireless --wifi`/
/// `--bluetooth` already accept on the command line; this is just its
/// inverse, so a saved preference round-trips through the same format
/// an operator would type.
fn provider_preference_to_key(preference: &platform_api::ProviderPreference) -> String {
    match preference {
        platform_api::ProviderPreference::Auto => "auto".to_string(),
        platform_api::ProviderPreference::Onboard => "onboard".to_string(),
        platform_api::ProviderPreference::StableId(id) => id.clone(),
    }
}

/// `Rotation`'s own `encode`/`decode` (`platform_linux::touch`) are
/// crate-private there — this module owns its own wire mapping instead,
/// matching the same choice already made for `GestureId`/`Action` (see
/// the module doc comment) and reusing the exact `0`/`180` values
/// `auth_discovery_probe.rs`'s `TOUCH_ROTATION_ENV_VAR` parsing already
/// established. Degrees, not a bare bool, for a self-describing on-disk
/// TOML value — `Rotation` itself is a 2-variant enum now (2026-08-18;
/// see its own doc comment for why 90°/270° were dropped), but
/// `rotation_degrees = 180` reads clearly without opening this file,
/// where `rotation_flipped = true` would not.
pub(crate) fn rotation_to_degrees(rotation: Rotation) -> u16 {
    match rotation {
        Rotation::Normal => 0,
        Rotation::Flipped180 => 180,
    }
}

fn rotation_from_degrees(degrees: u16) -> Option<Rotation> {
    match degrees {
        0 => Some(Rotation::Normal),
        180 => Some(Rotation::Flipped180),
        _ => None,
    }
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
pub struct HeadUnitSettings {
    mappings: HashMap<GestureId, Action>,
    arm_window_seconds: u32,
    mtp_popup_suppression_enabled: bool,
    rotation: Rotation,
    display_brightness_percent: u8,
    audio_output_device: Option<String>,
    microphone_input_device: Option<String>,
    night_mode_gpio_line: Option<u32>,
    wifi_preference: platform_api::ProviderPreference,
    bluetooth_preference: platform_api::ProviderPreference,
    theme: Option<String>,
    launch_on_boot: bool,
}

impl HeadUnitSettings {
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
        mappings.insert(GestureId::LongPress, Action::FlipScreen);
        mappings.insert(GestureId::SwipeUp, Action::SwitchToNavigation);
        mappings.insert(GestureId::SwipeDown, Action::SwitchToMedia);
        mappings.insert(GestureId::SwipeLeft, Action::SwitchToPhone);
        mappings.insert(GestureId::SwipeRight, Action::ToggleFullscreen);
        mappings.insert(GestureId::Circle, Action::ScreenOff);
        Self {
            mappings,
            arm_window_seconds: DEFAULT_ARM_WINDOW_SECONDS,
            mtp_popup_suppression_enabled: false,
            rotation: Rotation::Normal,
            display_brightness_percent: DEFAULT_DISPLAY_BRIGHTNESS_PERCENT,
            audio_output_device: None,
            microphone_input_device: None,
            night_mode_gpio_line: None,
            wifi_preference: platform_api::ProviderPreference::Auto,
            bluetooth_preference: platform_api::ProviderPreference::Auto,
            theme: None,
            launch_on_boot: false,
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

    /// Off by default — a real-hardware finding (2026-08-17/18): every
    /// AOA reconnect (a forced software disconnect, or an ordinary
    /// physical unplug/replug) makes the phone briefly re-enumerate in
    /// its normal MTP mode before this project's own code transitions it
    /// to Android Auto accessory mode, and the desktop's
    /// `gvfs-mtp-volume-monitor` sometimes tries to claim it during that
    /// window, producing "couldn't find matching udev device"/"no MTP
    /// devices found" popups. An initial udev-property-based attempt at
    /// suppressing this was confirmed real-hardware-*ineffective*
    /// (correct property override, popups still happened — see
    /// `mtp_suppression.rs`'s doc comment for the full investigation);
    /// the actual fix, confirmed by the operator watching the screen
    /// across 8 real reconnect cycles with zero popups, is masking the
    /// `gvfs-mtp-volume-monitor.service` `systemctl --user` service
    /// entirely while this is enabled. Off by default because that also
    /// disables ordinary MTP file-browsing of a phone plugged into this
    /// machine outside of Android Auto use, which is a real trade-off the
    /// operator should opt into, not a silent default.
    #[must_use]
    pub fn mtp_popup_suppression_enabled(&self) -> bool {
        self.mtp_popup_suppression_enabled
    }

    pub fn set_mtp_popup_suppression_enabled(&mut self, enabled: bool) {
        self.mtp_popup_suppression_enabled = enabled;
    }

    /// Defaults to `Normal` — real-hardware-adjustable since M3 (the
    /// `FlipScreen` gesture action, `AA_HEADUNIT_TOUCH_ROTATION` env
    /// override) but never persisted across a restart until M5. Live
    /// changes still apply immediately via `SharedRotation` exactly as
    /// before; this only adds "and it's remembered next time."
    #[must_use]
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    pub fn set_rotation(&mut self, rotation: Rotation) {
        self.rotation = rotation;
    }

    /// Applied whenever the screen is turned back on (`set_screen_power`,
    /// `auth_discovery_probe.rs`) — `100` (full brightness) by default,
    /// matching this project's behaviour before M5 introduced a real
    /// adjustable level at all.
    #[must_use]
    pub fn display_brightness_percent(&self) -> u8 {
        self.display_brightness_percent
    }

    /// Clamped to [`MIN_DISPLAY_BRIGHTNESS_PERCENT`]..=[`MAX_DISPLAY_BRIGHTNESS_PERCENT`]
    /// — see those constants' doc comments for why `0` is excluded.
    pub fn set_display_brightness_percent(&mut self, percent: u8) {
        self.display_brightness_percent = percent.clamp(
            MIN_DISPLAY_BRIGHTNESS_PERCENT,
            MAX_DISPLAY_BRIGHTNESS_PERCENT,
        );
    }

    /// `None` (the default) means the system default `PulseAudio` sink —
    /// this project has never had any other behaviour before M5, so a
    /// fresh install's actual audio routing is unchanged. `Some(name)`
    /// is passed straight to `pulsesink`'s `device` property
    /// (`media_gstreamer::AudioPlaybackPipeline::new`) with no validation
    /// that `name` actually exists; an invalid name simply fails at
    /// pipeline start like any other unreachable sink.
    #[must_use]
    pub fn audio_output_device(&self) -> Option<&str> {
        self.audio_output_device.as_deref()
    }

    pub fn set_audio_output_device(&mut self, device: Option<String>) {
        self.audio_output_device = device;
    }

    /// Mirrors [`Self::audio_output_device`] for microphone capture
    /// (`pulsesrc`'s `device` property,
    /// `media_gstreamer::MicrophoneCapturePipeline::new`).
    #[must_use]
    pub fn microphone_input_device(&self) -> Option<&str> {
        self.microphone_input_device.as_deref()
    }

    pub fn set_microphone_input_device(&mut self, device: Option<String>) {
        self.microphone_input_device = device;
    }

    /// `None` (the default) means night-mode signaling is disabled — no
    /// GPIO line is read, and the `NightMode` sensor always reports day
    /// mode, exactly matching this project's behaviour before this
    /// setting existed. `Some(line)` is a BCM GPIO line number on
    /// `platform_linux::gpio::DEFAULT_GPIO_CHIP`, read once per probe
    /// loop iteration (`sync_night_mode`, `auth_discovery_probe.rs`) to
    /// detect the car's illumination-wire signal — see
    /// `platform_linux::gpio`'s doc comment for the required external
    /// level-shifting hardware (Pi GPIOs are 3.3V logic, not 5V
    /// tolerant).
    #[must_use]
    pub fn night_mode_gpio_line(&self) -> Option<u32> {
        self.night_mode_gpio_line
    }

    pub fn set_night_mode_gpio_line(&mut self, line: Option<u32>) {
        self.night_mode_gpio_line = line;
    }

    /// Defaults to `Auto`. `usb wireless --wifi <auto|onboard|STABLE_ID>`
    /// uses this as its default whenever `--wifi` is omitted, and saves
    /// back to it whenever `--wifi` *is* given — the same "an explicit
    /// choice both applies and persists" pattern every other M5 setting
    /// already follows, so an operator only has to specify a preferred
    /// adapter once.
    #[must_use]
    pub fn wifi_preference(&self) -> &platform_api::ProviderPreference {
        &self.wifi_preference
    }

    pub fn set_wifi_preference(&mut self, preference: platform_api::ProviderPreference) {
        self.wifi_preference = preference;
    }

    /// Mirrors [`Self::wifi_preference`] for `--bluetooth`.
    #[must_use]
    pub fn bluetooth_preference(&self) -> &platform_api::ProviderPreference {
        &self.bluetooth_preference
    }

    pub fn set_bluetooth_preference(&mut self, preference: platform_api::ProviderPreference) {
        self.bluetooth_preference = preference;
    }

    /// `None` (the default) means the ordinary GTK4 theme with no custom
    /// stylesheet applied — this project's behaviour before this setting
    /// existed. `Some(name)` names a `.css` file the operator dropped
    /// into the themes directory (`gtk_dev_ui::THEMES_DIR`), stored here
    /// as just the file's stem (no directory, no `.css` extension) so a
    /// theme keeps working if that directory ever moves. No validation
    /// that the file still exists — a theme deleted since it was chosen
    /// simply fails to load at apply time, matching this project's
    /// existing device-dropdown precedent (`build_device_dropdown`'s doc
    /// comment) of falling back to the default rather than erroring.
    #[must_use]
    pub fn theme(&self) -> Option<&str> {
        self.theme.as_deref()
    }

    pub fn set_theme(&mut self, theme: Option<String>) {
        self.theme = theme;
    }

    /// Gates `packaging/labwc/aa-headunit-autostart`'s fullscreen
    /// auto-launch (checked via the `launch-on-boot-enabled` CLI
    /// subcommand, since a plain shell script can't parse this file's
    /// TOML directly) — **off by default**. Deliberate operator decision
    /// (2026-08-19), not an oversight: a real-hardware trial the same day
    /// found the fullscreen↔windowed transition ("return to desktop")
    /// can hang the whole compositor, with no way to recover short of a
    /// physical reboot — see `docs/development/appliance-recovery.md`.
    /// An unattended appliance that can silently hang on boot with no
    /// recourse is a worse default than one that boots to a plain,
    /// always-usable desktop; only turn this on once that hang is
    /// confirmed fixed on real hardware. An operator can turn it on from
    /// the Display settings page; the desktop shortcut
    /// (`packaging/labwc/aa-headunit.desktop`) still launches the app
    /// manually either way, regardless of this setting.
    #[must_use]
    pub fn launch_on_boot(&self) -> bool {
        self.launch_on_boot
    }

    pub fn set_launch_on_boot(&mut self, enabled: bool) {
        self.launch_on_boot = enabled;
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
        if let Some(enabled) = raw.suppress_phone_mtp_popups {
            settings.set_mtp_popup_suppression_enabled(enabled);
        }
        if let Some(rotation) = raw.rotation_degrees.and_then(rotation_from_degrees) {
            settings.set_rotation(rotation);
        }
        if let Some(percent) = raw.display_brightness_percent {
            settings.set_display_brightness_percent(percent);
        }
        if let Some(device) = raw.audio_output_device {
            settings.set_audio_output_device(Some(device));
        }
        if let Some(device) = raw.microphone_input_device {
            settings.set_microphone_input_device(Some(device));
        }
        if let Some(line) = raw.night_mode_gpio_line {
            settings.set_night_mode_gpio_line(Some(line));
        }
        if let Some(preference) = raw.wifi_preference {
            settings.set_wifi_preference(platform_api::ProviderPreference::parse(&preference));
        }
        if let Some(preference) = raw.bluetooth_preference {
            settings.set_bluetooth_preference(platform_api::ProviderPreference::parse(&preference));
        }
        if let Some(theme) = raw.theme {
            settings.set_theme(Some(theme));
        }
        if let Some(enabled) = raw.launch_on_boot {
            settings.set_launch_on_boot(enabled);
        }
        Some(settings)
    }

    /// Creates `path`'s parent directory if needed (matches
    /// `/var/lib/aa-headunit` already existing with group-writable
    /// permissions from packaging — this only ever needs `create_dir_all`
    /// for a fresh `/var/lib/aa-headunit` that already has the right
    /// parent permissions, never for `/var/lib` itself).
    ///
    /// Real-hardware finding (2026-08-19): the operator's `theme` choice
    /// repeatedly vanished from disk across a session with heavy testing
    /// activity (the app manually relaunched many times over, sometimes
    /// with more than one instance briefly alive at once). Root cause: a
    /// `HeadUnitSettings` in memory only ever reflects whatever was on
    /// disk *at the moment it was loaded* — every call site here saves
    /// the *whole* object on every single change (mtp toggle, rotation,
    /// brightness, ...), so a process whose own in-memory copy predates
    /// a field being set elsewhere silently writes that field back to
    /// its own default the next time anything at all changes, clobbering
    /// a concurrently-running process's more recent save. For every
    /// `Option`-typed field, `None` unambiguously means "this process
    /// never touched or loaded a value for this" (never "deliberately
    /// cleared" — none of these fields have a UI path that sets `None`
    /// on purpose), so it's safe to re-read whatever is currently on
    /// disk for exactly those fields and defer to it instead of
    /// overwriting with a stale `None`. Every other field lacks that
    /// distinction (e.g. `false`/`0` are both real, legitimate,
    /// deliberately-chosen values as well as defaults), so those are
    /// unaffected and still save exactly what this in-memory copy holds.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let on_disk = Self::try_load(path);
        let mut raw = RawSettings::default();
        for gesture in GestureId::all() {
            raw.set(gesture, self.action_for(gesture));
        }
        raw.arm_window_seconds = Some(self.arm_window_seconds);
        raw.suppress_phone_mtp_popups = Some(self.mtp_popup_suppression_enabled);
        raw.rotation_degrees = Some(rotation_to_degrees(self.rotation));
        raw.display_brightness_percent = Some(self.display_brightness_percent);
        raw.audio_output_device = self.audio_output_device.clone().or_else(|| {
            on_disk
                .as_ref()
                .and_then(|settings| settings.audio_output_device.clone())
        });
        raw.microphone_input_device = self.microphone_input_device.clone().or_else(|| {
            on_disk
                .as_ref()
                .and_then(|settings| settings.microphone_input_device.clone())
        });
        raw.night_mode_gpio_line = self.night_mode_gpio_line.or_else(|| {
            on_disk
                .as_ref()
                .and_then(|settings| settings.night_mode_gpio_line)
        });
        raw.wifi_preference = Some(provider_preference_to_key(&self.wifi_preference));
        raw.bluetooth_preference = Some(provider_preference_to_key(&self.bluetooth_preference));
        raw.theme = self
            .theme
            .clone()
            .or_else(|| on_disk.as_ref().and_then(|settings| settings.theme.clone()));
        raw.launch_on_boot = Some(self.launch_on_boot);
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
        let settings = HeadUnitSettings::defaults();
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
            Action::FlipScreen
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
        assert_eq!(Action::FlipScreen.key_code(), None);
        assert_eq!(Action::ScreenOff.key_code(), None);
    }

    #[test]
    fn a_circle_gesture_reassignment_round_trips() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-gesture-settings-circle-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_action(GestureId::Circle, Action::OpenSettings);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
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

        let mut settings = HeadUnitSettings::defaults();
        settings.set_action(GestureId::SwipeUp, Action::SwitchToRadio);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
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

        let mut settings = HeadUnitSettings::defaults();
        settings.set_action(GestureId::DoubleTap, Action::FlipScreen);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.action_for(GestureId::DoubleTap), Action::FlipScreen);
        // Untouched mappings still round-trip correctly.
        assert_eq!(
            loaded.action_for(GestureId::TwoFingerTap),
            Action::ToggleFullscreen
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mtp_popup_suppression_defaults_off_and_round_trips_on() {
        let defaults = HeadUnitSettings::defaults();
        assert!(!defaults.mtp_popup_suppression_enabled());

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-gesture-settings-mtp-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_mtp_popup_suppression_enabled(true);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert!(loaded.mtp_popup_suppression_enabled());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_falls_back_to_defaults_when_the_file_is_missing() {
        let path = Path::new("/nonexistent/aa-headunit-settings-test.toml");
        let settings = HeadUnitSettings::load(path);
        assert_eq!(settings, HeadUnitSettings::defaults());
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

        let settings = HeadUnitSettings::load(&path);
        assert_eq!(settings, HeadUnitSettings::defaults());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn arm_window_seconds_round_trips_and_persists() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-gesture-settings-arm-window-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        assert_eq!(settings.arm_window_seconds(), DEFAULT_ARM_WINDOW_SECONDS);
        settings.set_arm_window_seconds(10);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.arm_window_seconds(), 10);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn arm_window_seconds_clamps_to_the_allowed_range() {
        let mut settings = HeadUnitSettings::defaults();
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

    #[test]
    fn rotation_defaults_to_zero_and_round_trips() {
        let defaults = HeadUnitSettings::defaults();
        assert_eq!(defaults.rotation(), Rotation::Normal);

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-rotation-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_rotation(Rotation::Flipped180);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.rotation(), Rotation::Flipped180);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn display_brightness_defaults_full_and_clamps_and_round_trips() {
        let defaults = HeadUnitSettings::defaults();
        assert_eq!(
            defaults.display_brightness_percent(),
            DEFAULT_DISPLAY_BRIGHTNESS_PERCENT
        );

        let mut settings = HeadUnitSettings::defaults();
        settings.set_display_brightness_percent(0);
        assert_eq!(
            settings.display_brightness_percent(),
            MIN_DISPLAY_BRIGHTNESS_PERCENT
        );
        settings.set_display_brightness_percent(255);
        assert_eq!(
            settings.display_brightness_percent(),
            MAX_DISPLAY_BRIGHTNESS_PERCENT
        );

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-brightness-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");
        settings.set_display_brightness_percent(42);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.display_brightness_percent(), 42);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn audio_and_microphone_devices_default_to_system_default_and_round_trip() {
        let defaults = HeadUnitSettings::defaults();
        assert_eq!(defaults.audio_output_device(), None);
        assert_eq!(defaults.microphone_input_device(), None);

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-devices-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_audio_output_device(Some("alsa_output.example".to_string()));
        settings.set_microphone_input_device(Some("alsa_input.example".to_string()));
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.audio_output_device(), Some("alsa_output.example"));
        assert_eq!(loaded.microphone_input_device(), Some("alsa_input.example"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn night_mode_gpio_line_defaults_to_disabled_and_round_trips() {
        let defaults = HeadUnitSettings::defaults();
        assert_eq!(defaults.night_mode_gpio_line(), None);

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-night-mode-gpio-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_night_mode_gpio_line(Some(17));
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.night_mode_gpio_line(), Some(17));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn radio_preferences_default_to_auto_and_round_trip_all_variants() {
        let defaults = HeadUnitSettings::defaults();
        assert_eq!(
            defaults.wifi_preference(),
            &platform_api::ProviderPreference::Auto
        );
        assert_eq!(
            defaults.bluetooth_preference(),
            &platform_api::ProviderPreference::Auto
        );

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-radio-preference-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_wifi_preference(platform_api::ProviderPreference::Onboard);
        settings.set_bluetooth_preference(platform_api::ProviderPreference::StableId(
            "usb:1234:5678:1-2".to_string(),
        ));
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(
            loaded.wifi_preference(),
            &platform_api::ProviderPreference::Onboard
        );
        assert_eq!(
            loaded.bluetooth_preference(),
            &platform_api::ProviderPreference::StableId("usb:1234:5678:1-2".to_string())
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_defaults_to_none_and_round_trips() {
        let defaults = HeadUnitSettings::defaults();
        assert_eq!(defaults.theme(), None);

        let dir =
            std::env::temp_dir().join(format!("aa-headunit-settings-theme-{}", std::process::id()));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_theme(Some("sunset".to_string()));
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.theme(), Some("sunset"));

        let _ = fs::remove_dir_all(&dir);
    }

    /// Real-hardware finding (2026-08-19): the operator's theme choice
    /// repeatedly vanished from disk during a session with heavy testing
    /// activity — traced to a second, independently-loaded (and thus
    /// theme-unaware) `HeadUnitSettings` in a different process saving
    /// over it. Reproduces that exact race in miniature: process A loads
    /// fresh, sets a theme, saves; process B — already loaded *before*
    /// that save, so its own `theme` is still `None` — then saves an
    /// unrelated change. Without `save`'s on-disk merge for
    /// `Option`-typed fields, process B's save would blindly overwrite
    /// process A's theme with `None`.
    #[test]
    fn a_second_already_loaded_settings_instance_does_not_clobber_a_concurrently_saved_theme() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-concurrent-theme-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        // Both "processes" start from the same pre-theme on-disk state.
        let mut process_a = HeadUnitSettings::defaults();
        process_a.save(&path).expect("initial save succeeds");
        let mut process_b = HeadUnitSettings::load(&path);

        process_a.set_theme(Some("sunset".to_string()));
        process_a.save(&path).expect("process a save succeeds");
        assert_eq!(HeadUnitSettings::load(&path).theme(), Some("sunset"));

        // process_b never learned about the theme — its own save (for
        // something unrelated) must not erase it.
        process_b.set_launch_on_boot(true);
        process_b.save(&path).expect("process b save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.theme(), Some("sunset"));
        assert!(loaded.launch_on_boot());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn launch_on_boot_defaults_to_false_and_round_trips_enabled() {
        let defaults = HeadUnitSettings::defaults();
        assert!(!defaults.launch_on_boot());

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-launch-on-boot-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_launch_on_boot(true);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert!(loaded.launch_on_boot());

        let _ = fs::remove_dir_all(&dir);
    }
}
