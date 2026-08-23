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
/// `100` (unity gain on the `GStreamer` `volume` element — no change
/// from whatever level the phone itself sends) is the default, so a
/// fresh install sounds exactly like it did before this setting existed.
/// Capped at `100` rather than allowing a boost above the phone's own
/// output level, to avoid clipping/distortion — a deliberately
/// conservative starting range, not a hard technical limit of the
/// underlying element.
pub const DEFAULT_VOLUME_PERCENT: u8 = 100;
pub const MIN_VOLUME_PERCENT: u8 = 0;
pub const MAX_VOLUME_PERCENT: u8 = 100;
/// `equalizer-10bands`' own documented per-band gain range in dB — not
/// this project's choice, matched here so a saved value never gets
/// clamped to something different than what the `GStreamer` element itself
/// would accept.
pub const MIN_EQ_BAND_GAIN_DB: f64 = -24.0;
pub const MAX_EQ_BAND_GAIN_DB: f64 = 12.0;
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
    QuickControls,
}

impl Action {
    #[must_use]
    pub const fn all() -> [Action; 9] {
        [
            Action::OpenSettings,
            Action::ToggleFullscreen,
            Action::FlipScreen,
            Action::SwitchToMedia,
            Action::SwitchToNavigation,
            Action::SwitchToRadio,
            Action::SwitchToPhone,
            Action::ScreenOff,
            Action::QuickControls,
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
            Action::QuickControls => "Brightness & volume",
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
            | Action::ScreenOff
            | Action::QuickControls => None,
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
            Action::QuickControls => "quick_controls",
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
            "quick_controls" => Some(Action::QuickControls),
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
    #[serde(skip_serializing_if = "Option::is_none")]
    wireless_bluetooth_auto_connect: Option<bool>,
    /// `Vec<f64>` rather than `[f64; 10]` purely to sidestep any doubt
    /// about this project's pinned `serde`/`toml` versions' fixed-size
    /// array support — `HeadUnitSettings::eq_bands`/`set_eq_bands` are
    /// the actual `[f64; 10]`-typed API; this field is a plain,
    /// always-serializable transport shape between the two, validated to
    /// exactly 10 entries on load (see `try_load`/`load`) and never
    /// trusted otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    eq_bands: Option<Vec<f64>>,
    /// `EQ_PRESET_COUNT` slots, each a 10-entry `Vec<f64>` — same
    /// plain-`Vec` transport shape as `eq_bands` and for the same reason
    /// (see that field's doc comment); validated to exactly
    /// `EQ_PRESET_COUNT` outer entries of exactly `EQ_BAND_COUNT` inner
    /// entries on load.
    #[serde(skip_serializing_if = "Option::is_none")]
    eq_presets: Option<Vec<Vec<f64>>>,
    /// `EQ_PRESET_COUNT` display names, in the same slot order as
    /// `eq_presets`; validated to exactly `EQ_PRESET_COUNT` entries on
    /// load, same posture as `eq_presets` itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    eq_preset_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    experimental_disclaimer_dismissed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    legal_page_hidden: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    volume_percent: Option<u8>,
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

// Plain persisted operator preferences, independent of each other — a
// state machine/enum refactor would add indirection without removing any
// real complexity here.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
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
    wireless_bluetooth_auto_connect: bool,
    /// Gains in dB for `GStreamer`'s `equalizer-10bands` element's
    /// `band0`..`band9` properties (ISO center frequencies, roughly
    /// 29 Hz to 15 kHz). All-zero is flat/unmodified — the default, so a
    /// fresh install sounds exactly like it did before this existed.
    eq_bands: [f64; EQ_BAND_COUNT],
    /// `EQ_PRESET_COUNT` saveable/recallable equalizer snapshots,
    /// independent of the live `eq_bands` above — see
    /// [`Self::eq_preset`]/[`Self::set_eq_preset`]'s doc comments.
    eq_presets: [[f64; EQ_BAND_COUNT]; EQ_PRESET_COUNT],
    /// Display name for each `eq_presets` slot (2026-08-22, Blake's
    /// request: a renamable preset editor) — defaults to `"Preset 1"`..
    /// `"Preset 4"`. See [`Self::eq_preset_name`]/[`Self::set_eq_preset_name`].
    eq_preset_names: [String; EQ_PRESET_COUNT],
    /// Whether the operator has already agreed to the head-unit-wide
    /// experimental-software disclaimer — either via the boot popup's
    /// "don't show this again" checkbox, or the Legal settings page's
    /// "I have read and accept this" checkbox (the same flag, reachable
    /// two ways). `false` (the default) means the boot popup is shown
    /// again every boot. See `gtk_dev_ui.rs`'s `build_experimental_disclaimer`
    /// doc comment for what it actually says and why (`RISK_REGISTER.md`
    /// R-021).
    experimental_disclaimer_dismissed: bool,
    /// Whether the operator has hidden the Legal settings page from the
    /// top-level menu (the "⋮" button on that menu brings it back). Only
    /// ever meaningful when `experimental_disclaimer_dismissed` is also
    /// `true` — see `legal_page_hidden()`'s doc comment.
    legal_page_hidden: bool,
    /// Applied to the `GStreamer` `volume` element in each audio
    /// pipeline (`crates/media-gstreamer/src/audio.rs`) — `100` (unity
    /// gain) is the default. Same "not live-applied mid-session"
    /// precedent as `eq_bands`.
    volume_percent: u8,
}

/// `equalizer-10bands`' own fixed band count (its name says so, and its
/// `GStreamer` documentation confirms `band0..band9`) — not this project's
/// choice, so not a `settings.rs`-local constant to second-guess later.
pub const EQ_BAND_COUNT: usize = 10;

/// Number of saveable equalizer presets (extras roadmap, 2026-08-22:
/// Blake asked for a fixed set of slots he can dial in and recall, rather
/// than only ever having the one live `eq_bands` snapshot). A fixed count
/// rather than an open-ended list, matching this project's existing
/// "small, closed set" precedent (see e.g. `Action`'s own doc comment) —
/// four is enough to be useful without needing a scrollable list on an
/// 800x480 panel.
pub const EQ_PRESET_COUNT: usize = 4;

/// A preset name longer than this is truncated on save — the button that
/// shows it sits in a narrow column next to the full-length band sliders
/// (`gtk_dev_ui.rs`'s equalizer page), so an unbounded name would either
/// overflow or force that column wider than the sliders can spare.
pub const MAX_EQ_PRESET_NAME_CHARS: usize = 20;

impl HeadUnitSettings {
    /// Double-tap opens settings (the discoverable, always-safe default);
    /// two-finger tap toggles fullscreen; three of the four swipe
    /// directions default to category-switch actions in a spatially
    /// obvious layout (up→navigation, down→media, matching how a map sits
    /// above and music sits below on a typical AA home screen; left→phone)
    /// — chosen so every gesture does something distinct out of the box,
    /// not because any one mapping is more "correct" than another.
    /// Long press defaults to `QuickControls` (2026-08-22, Blake's explicit
    /// request: a brightness/volume popup reachable by long press only, not
    /// buried in the full settings panel) — previously `FlipScreen`, which
    /// moved to swipe right instead of being dropped. Swipe right defaults
    /// to `FlipScreen` (genuinely functional, and no longer duplicated by
    /// long press) rather than `SwitchToRadio`: `SwitchToRadio` is
    /// real-hardware-confirmed to correctly navigate to Android Auto's own
    /// native radio screen (see `protocol_aap::RadioCapability`'s doc
    /// comment), but that screen is empty without a real tuner backend this
    /// project deliberately doesn't implement — not a good default for a
    /// fresh install. `SwitchToRadio` is still a fully selectable, working
    /// action. `Circle` defaults to `ScreenOff` — the gesture this action
    /// was added for (2026-08-17), originally a triple-finger double-tap
    /// replaced 2026-08-18 after real-hardware testing found three-finger
    /// coordination unreliable (see `platform_api::gesture`'s doc
    /// comment).
    #[must_use]
    pub fn defaults() -> Self {
        let mut mappings = HashMap::new();
        mappings.insert(GestureId::DoubleTap, Action::OpenSettings);
        mappings.insert(GestureId::TwoFingerTap, Action::ToggleFullscreen);
        mappings.insert(GestureId::LongPress, Action::QuickControls);
        mappings.insert(GestureId::SwipeUp, Action::SwitchToNavigation);
        mappings.insert(GestureId::SwipeDown, Action::SwitchToMedia);
        mappings.insert(GestureId::SwipeLeft, Action::SwitchToPhone);
        mappings.insert(GestureId::SwipeRight, Action::FlipScreen);
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
            wireless_bluetooth_auto_connect: true,
            eq_bands: [0.0; EQ_BAND_COUNT],
            eq_presets: [[0.0; EQ_BAND_COUNT]; EQ_PRESET_COUNT],
            eq_preset_names: std::array::from_fn(|index| format!("Preset {}", index + 1)),
            experimental_disclaimer_dismissed: false,
            legal_page_hidden: false,
            volume_percent: DEFAULT_VOLUME_PERCENT,
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

    /// Gains in dB for each of `equalizer-10bands`' 10 bands, applied at
    /// the next audio pipeline build (`start_audio_playback_pipeline`,
    /// `auth_discovery_probe.rs`) — like `audio_output_device` below,
    /// this does not live-apply to an already-running session, matching
    /// this project's existing precedent for audio-related settings.
    #[must_use]
    pub fn eq_bands(&self) -> [f64; EQ_BAND_COUNT] {
        self.eq_bands
    }

    /// Clamped to [`MIN_EQ_BAND_GAIN_DB`]..=[`MAX_EQ_BAND_GAIN_DB`], matching
    /// `equalizer-10bands`' own documented property range. `index` outside
    /// `0..EQ_BAND_COUNT` is silently ignored rather than panicking — the
    /// UI only ever calls this with a real slider's own fixed index, but a
    /// hard panic here would be a disproportionate failure mode for what
    /// is, worst case, a cosmetic settings-persistence no-op.
    pub fn set_eq_band(&mut self, index: usize, gain_db: f64) {
        if let Some(band) = self.eq_bands.get_mut(index) {
            *band = gain_db.clamp(MIN_EQ_BAND_GAIN_DB, MAX_EQ_BAND_GAIN_DB);
        }
    }

    /// One saved equalizer snapshot, `index` in `0..EQ_PRESET_COUNT`. All
    /// four presets default to flat (all-zero) — this project has no
    /// basis for guessing at "good" EQ curves nobody asked for, so every
    /// slot starts as a blank the operator dials in and saves themselves
    /// (see [`Self::set_eq_preset`]). `index` outside range returns flat
    /// rather than panicking, matching [`Self::set_eq_band`]'s own
    /// out-of-range handling.
    #[must_use]
    pub fn eq_preset(&self, index: usize) -> [f64; EQ_BAND_COUNT] {
        self.eq_presets
            .get(index)
            .copied()
            .unwrap_or([0.0; EQ_BAND_COUNT])
    }

    /// Overwrites preset `index` with `gains`, clamped per-band to the
    /// same [`MIN_EQ_BAND_GAIN_DB`]..=[`MAX_EQ_BAND_GAIN_DB`] range as
    /// [`Self::set_eq_band`]. `index` outside `0..EQ_PRESET_COUNT` is
    /// silently ignored, matching [`Self::set_eq_band`]'s reasoning: the
    /// UI only ever calls this with one of the four fixed preset slots.
    pub fn set_eq_preset(&mut self, index: usize, gains: [f64; EQ_BAND_COUNT]) {
        if let Some(preset) = self.eq_presets.get_mut(index) {
            for (slot, gain_db) in preset.iter_mut().zip(gains) {
                *slot = gain_db.clamp(MIN_EQ_BAND_GAIN_DB, MAX_EQ_BAND_GAIN_DB);
            }
        }
    }

    /// Display name for preset `index`, `index` in `0..EQ_PRESET_COUNT`.
    /// Out-of-range returns `"Preset"` rather than panicking, matching
    /// [`Self::eq_preset`]'s own out-of-range handling.
    #[must_use]
    pub fn eq_preset_name(&self, index: usize) -> &str {
        self.eq_preset_names
            .get(index)
            .map_or("Preset", String::as_str)
    }

    /// Renames preset `index`. Truncated to [`MAX_EQ_PRESET_NAME_CHARS`]
    /// and trimmed of leading/trailing whitespace; a blank result (empty
    /// input, or input that's only whitespace) resets to that slot's
    /// default `"Preset N"` name rather than leaving an empty button
    /// label. `index` outside `0..EQ_PRESET_COUNT` is silently ignored,
    /// matching [`Self::set_eq_preset`]'s own out-of-range handling.
    pub fn set_eq_preset_name(&mut self, index: usize, name: &str) {
        if let Some(slot) = self.eq_preset_names.get_mut(index) {
            let trimmed = name.trim();
            *slot = if trimmed.is_empty() {
                format!("Preset {}", index + 1)
            } else {
                trimmed.chars().take(MAX_EQ_PRESET_NAME_CHARS).collect()
            };
        }
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

    /// Whether `usb kiosk` keeps automatically scheduling another
    /// reconnect attempt after the operator closes AA
    /// (`gtk_dev_ui::end_kiosk_attempt`'s `window_close_requested`
    /// branch), versus going idle (`KioskWindow::awaiting_manual_reconnect`)
    /// until they deliberately reopen it (a repeat `GApplication`
    /// activation from opening the desktop icon again, wired up in
    /// `gtk_dev_ui::run_kiosk`'s `connect_activate` closure). Added
    /// 2026-08-23 at the operator's explicit request; went through two
    /// wrong shapes the same night before landing here — first gating
    /// only the Bluetooth active-push step (too narrow: a phone that had
    /// recently been actively connected still reconnected passively on
    /// its own, so turning the checkbox off looked like it did nothing),
    /// then widened into a master switch over whether wireless is
    /// attempted at all (too broad: it also blocked a deliberate manual
    /// open, which the operator was explicit must always try both wired
    /// and wireless immediately and actively regardless of this
    /// checkbox). Neither of those survived; this is the third and
    /// correct shape, confirmed against the operator's own restatement of
    /// the desired behavior. Never affects opening AA in the first place
    /// — only what happens automatically after a close. Errors and
    /// disconnects that aren't a deliberate close are unaffected either
    /// way: those always keep retrying, checkbox or not — this setting is
    /// specifically about the operator's own close action. Defaults on,
    /// unlike `launch_on_boot`: this isn't a can-silently-hang-on-boot
    /// risk, just the now-proven-correct way reconnection should behave
    /// by default.
    #[must_use]
    pub fn wireless_bluetooth_auto_connect(&self) -> bool {
        self.wireless_bluetooth_auto_connect
    }

    pub fn set_wireless_bluetooth_auto_connect(&mut self, enabled: bool) {
        self.wireless_bluetooth_auto_connect = enabled;
    }

    #[must_use]
    pub fn experimental_disclaimer_dismissed(&self) -> bool {
        self.experimental_disclaimer_dismissed
    }

    pub fn set_experimental_disclaimer_dismissed(&mut self, dismissed: bool) {
        self.experimental_disclaimer_dismissed = dismissed;
    }

    /// `true` only when the operator has *both* explicitly hidden the
    /// page *and* already agreed to the disclaimer it holds — a
    /// defensive `&&`, not just trusting the stored `legal_page_hidden`
    /// bit alone, so a hand-edited or otherwise inconsistent settings
    /// file can never hide the one page that explains the operator
    /// hasn't actually agreed to anything yet. The real UI flow already
    /// enforces this ordering (the "hide" checkbox is disabled until
    /// "read and accepted" is checked); this is the same guarantee
    /// enforced again at the data layer.
    #[must_use]
    pub fn legal_page_hidden(&self) -> bool {
        self.legal_page_hidden && self.experimental_disclaimer_dismissed
    }

    pub fn set_legal_page_hidden(&mut self, hidden: bool) {
        self.legal_page_hidden = hidden;
    }

    #[must_use]
    pub fn volume_percent(&self) -> u8 {
        self.volume_percent
    }

    /// Clamped to [`MIN_VOLUME_PERCENT`]..=[`MAX_VOLUME_PERCENT`].
    pub fn set_volume_percent(&mut self, percent: u8) {
        self.volume_percent = percent.clamp(MIN_VOLUME_PERCENT, MAX_VOLUME_PERCENT);
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
        if let Some(enabled) = raw.wireless_bluetooth_auto_connect {
            settings.set_wireless_bluetooth_auto_connect(enabled);
        }
        if let Some(dismissed) = raw.experimental_disclaimer_dismissed {
            settings.set_experimental_disclaimer_dismissed(dismissed);
        }
        if let Some(hidden) = raw.legal_page_hidden {
            settings.set_legal_page_hidden(hidden);
        }
        if let Some(percent) = raw.volume_percent {
            settings.set_volume_percent(percent);
        }
        // A wrong-length array (hand-edited file, or a future format
        // change) is simply ignored rather than treated as corruption —
        // matching this project's existing forward/backward-compat
        // posture for settings (see `GestureSettings::load`'s doc
        // comment) — so eq_bands quietly stays flat instead of the whole
        // file failing to load.
        if let Some(bands) = raw
            .eq_bands
            .and_then(|bands| <[f64; EQ_BAND_COUNT]>::try_from(bands).ok())
        {
            for (index, gain_db) in bands.into_iter().enumerate() {
                settings.set_eq_band(index, gain_db);
            }
        }
        // Same wrong-shape-is-ignored posture as eq_bands above, applied
        // one level deeper: a wrong outer length leaves every preset at
        // its flat default; a right outer length with one malformed
        // inner entry leaves just that one preset flat, loading the rest
        // normally.
        if let Some(presets) = raw.eq_presets {
            if let Ok(presets) = <[Vec<f64>; EQ_PRESET_COUNT]>::try_from(presets) {
                for (index, bands) in presets.into_iter().enumerate() {
                    if let Ok(bands) = <[f64; EQ_BAND_COUNT]>::try_from(bands) {
                        settings.set_eq_preset(index, bands);
                    }
                }
            }
        }
        if let Some(names) = raw
            .eq_preset_names
            .and_then(|names| <[String; EQ_PRESET_COUNT]>::try_from(names).ok())
        {
            for (index, name) in names.into_iter().enumerate() {
                settings.set_eq_preset_name(index, &name);
            }
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
        raw.eq_bands = Some(self.eq_bands.to_vec());
        raw.eq_presets = Some(self.eq_presets.iter().map(|bands| bands.to_vec()).collect());
        raw.eq_preset_names = Some(self.eq_preset_names.to_vec());
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
        raw.wireless_bluetooth_auto_connect = Some(self.wireless_bluetooth_auto_connect);
        raw.experimental_disclaimer_dismissed = Some(self.experimental_disclaimer_dismissed);
        raw.legal_page_hidden = Some(self.legal_page_hidden);
        raw.volume_percent = Some(self.volume_percent);
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
            Action::QuickControls
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
            Action::FlipScreen
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
        assert_eq!(Action::QuickControls.key_code(), None);
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

    #[test]
    fn wireless_bluetooth_auto_connect_defaults_on_and_round_trips_disabled() {
        let defaults = HeadUnitSettings::defaults();
        assert!(defaults.wireless_bluetooth_auto_connect());

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-bt-auto-connect-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_wireless_bluetooth_auto_connect(false);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert!(!loaded.wireless_bluetooth_auto_connect());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn experimental_disclaimer_dismissed_defaults_to_false_and_round_trips_true() {
        let defaults = HeadUnitSettings::defaults();
        assert!(!defaults.experimental_disclaimer_dismissed());

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-dashcam-disclaimer-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_experimental_disclaimer_dismissed(true);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert!(loaded.experimental_disclaimer_dismissed());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legal_page_hidden_defaults_to_false_and_round_trips_true() {
        let defaults = HeadUnitSettings::defaults();
        assert!(!defaults.legal_page_hidden());

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-legal-page-hidden-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_experimental_disclaimer_dismissed(true);
        settings.set_legal_page_hidden(true);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert!(loaded.legal_page_hidden());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn legal_page_hidden_reads_as_false_when_disclaimer_not_accepted() {
        // The defensive `&&` in `legal_page_hidden()`: a settings file
        // with `legal_page_hidden = true` but no acceptance recorded
        // (hand-edited, or a future bug elsewhere) must never actually
        // hide the one page that explains the operator hasn't agreed to
        // anything yet.
        let mut settings = HeadUnitSettings::defaults();
        settings.set_legal_page_hidden(true);
        assert!(!settings.experimental_disclaimer_dismissed());
        assert!(!settings.legal_page_hidden());
    }

    #[test]
    fn volume_percent_defaults_to_100_and_clamps_and_round_trips() {
        let defaults = HeadUnitSettings::defaults();
        assert_eq!(defaults.volume_percent(), 100);

        let mut settings = HeadUnitSettings::defaults();
        settings.set_volume_percent(255);
        assert_eq!(settings.volume_percent(), MAX_VOLUME_PERCENT);

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-volume-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_volume_percent(42);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.volume_percent(), 42);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn eq_presets_default_flat_clamp_and_are_independent_of_each_other() {
        let defaults = HeadUnitSettings::defaults();
        for index in 0..EQ_PRESET_COUNT {
            assert_eq!(defaults.eq_preset(index), [0.0; EQ_BAND_COUNT]);
        }

        let mut settings = HeadUnitSettings::defaults();
        let mut loud = [0.0; EQ_BAND_COUNT];
        loud[0] = 999.0;
        loud[1] = -999.0;
        settings.set_eq_preset(0, loud);
        let clamped = settings.eq_preset(0);
        assert_eq!(clamped[0], MAX_EQ_BAND_GAIN_DB);
        assert_eq!(clamped[1], MIN_EQ_BAND_GAIN_DB);

        // Untouched slots stay flat — presets don't bleed into each other.
        assert_eq!(settings.eq_preset(1), [0.0; EQ_BAND_COUNT]);

        // Out-of-range index is a harmless no-op/flat-read, matching
        // set_eq_band/eq_bands's own out-of-range handling.
        settings.set_eq_preset(EQ_PRESET_COUNT, [5.0; EQ_BAND_COUNT]);
        assert_eq!(settings.eq_preset(EQ_PRESET_COUNT), [0.0; EQ_BAND_COUNT]);
    }

    #[test]
    fn eq_presets_round_trip_through_disk() {
        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-eq-presets-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");

        let mut settings = HeadUnitSettings::defaults();
        let mut bass_boost = [0.0; EQ_BAND_COUNT];
        bass_boost[0] = 6.0;
        bass_boost[1] = 4.0;
        settings.set_eq_preset(0, bass_boost);
        let mut treble_boost = [0.0; EQ_BAND_COUNT];
        treble_boost[8] = 5.0;
        treble_boost[9] = 5.0;
        settings.set_eq_preset(3, treble_boost);
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.eq_preset(0), bass_boost);
        assert_eq!(loaded.eq_preset(1), [0.0; EQ_BAND_COUNT]);
        assert_eq!(loaded.eq_preset(2), [0.0; EQ_BAND_COUNT]);
        assert_eq!(loaded.eq_preset(3), treble_boost);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn eq_preset_names_default_trim_truncate_blank_reset_and_round_trip() {
        let defaults = HeadUnitSettings::defaults();
        assert_eq!(defaults.eq_preset_name(0), "Preset 1");
        assert_eq!(defaults.eq_preset_name(3), "Preset 4");
        // Out-of-range is a harmless flat read, matching eq_preset's own
        // out-of-range handling.
        assert_eq!(defaults.eq_preset_name(EQ_PRESET_COUNT), "Preset");

        let mut settings = HeadUnitSettings::defaults();
        settings.set_eq_preset_name(0, "  Bass Boost  ");
        assert_eq!(settings.eq_preset_name(0), "Bass Boost");

        settings.set_eq_preset_name(1, "This name is absolutely way too long for a button");
        assert_eq!(
            settings.eq_preset_name(1).chars().count(),
            MAX_EQ_PRESET_NAME_CHARS
        );

        // Blank (or whitespace-only) input resets to that slot's own
        // default name, not an empty button label.
        settings.set_eq_preset_name(0, "   ");
        assert_eq!(settings.eq_preset_name(0), "Preset 1");

        // Out-of-range index is a no-op.
        settings.set_eq_preset_name(EQ_PRESET_COUNT, "ignored");

        let dir = std::env::temp_dir().join(format!(
            "aa-headunit-settings-eq-preset-names-{}",
            std::process::id()
        ));
        let path = dir.join("settings.toml");
        settings.set_eq_preset_name(2, "Road Trip");
        settings.save(&path).expect("save succeeds");

        let loaded = HeadUnitSettings::load(&path);
        assert_eq!(loaded.eq_preset_name(0), "Preset 1");
        assert_eq!(loaded.eq_preset_name(2), "Road Trip");

        let _ = fs::remove_dir_all(&dir);
    }
}
