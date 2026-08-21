//! `usb gtk-dev-ui` — wires a real phone session into the GTK4 rendering
//! path the on-device spike
//! (`crates/media-gstreamer/examples/gtk_fullscreen_spike.rs`) already
//! proved on synthetic video. Reuses that spike's exact GTK4 setup
//! (full-screen window, `gtk::Picture`, `gtk4paintablesink` via
//! `RenderSink::Gtk4Paintable`) and `usb_auth_discovery_probe`'s existing
//! device-discovery/AOA-transition/transport-open body (`main.rs`)
//! unchanged, bridged into a real session via
//! `auth_discovery_probe::VideoRenderTarget::Gtk4Window` — see that type's
//! doc comment (`auth_discovery_probe.rs`) for why pipeline construction
//! must happen on this GTK thread rather than the background thread that
//! runs the actual protocol session (`gtk4paintablesink`'s `"paintable"`
//! property must be retrieved from the thread owning the default
//! `GLibMainContext`).
//!
//! One-shot, like `usb auth-discovery-probe` — not the
//! `session-supervisor` reconnect loop. Deliberately the smaller first
//! increment; automatic reconnect for this path is separate, undecided
//! future scope.
//!
//! Also owns the head unit's own settings panel
//! (`MILESTONE_CHECKLIST.md` M3's touch item): a four-finger-swipe-then-
//! follow-up-gesture (`platform_api::ArmedGestureDetector`, driven on the
//! background protocol thread from real touch frames) signals this thread
//! over `TouchSettingsHandoff::gesture_sender`; this thread looks up what
//! that gesture is currently mapped to
//! (`settings::HeadUnitSettings`, persisted to
//! `/var/lib/aa-headunit/settings.toml`) and either shows the settings
//! panel, toggles fullscreen (bidirectionally — see `Action`'s doc
//! comment for why; the session keeps running underneath either way, per
//! the operator's explicit choice not to tear it down), or cycles touch
//! rotation live via `TouchSettingsHandoff::rotation_sender`'s
//! `SharedRotation` handle.
//!
//! Several `mpsc` handoffs bridge the GTK (main) thread and the protocol
//! (background) thread: `Gtk4WindowHandoff` (defined in
//! `auth_discovery_probe.rs`, sends a negotiated `DecoderCapability` and
//! blocks for the built `VideoRenderPipeline`), the settings/rotation pair
//! in `TouchSettingsHandoff`, and a final, simpler pair carrying the
//! background thread's final `Result<(), CliError>` back so this command
//! keeps the same non-zero-exit-on-failure convention every other `usb *`
//! subcommand has. All are `glib::timeout_add_local` polls (100ms) rather
//! than blocking receives, since neither may ever block GTK's own event
//! loop.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CheckButton, DropDown, Grid, Label,
    Orientation, Overlay, Picture, PolicyType, Scale, ScrolledWindow, SpinButton, glib,
};
use media_api::DecoderCapability;
use media_gstreamer::{GstreamerBackend, GstreamerError, RenderSink, VideoRenderPipeline};
use platform_api::{GestureEvent, GestureId, SharedArmWindow};
use platform_linux::touch::{Rotation, SharedRotation};
use transport_api::{AoaIdentification, AoaMachine};

use crate::CliError;
use crate::auth_discovery_probe::{
    Gtk4WindowHandoff, SharedPanelVisibility, TouchSettingsHandoff, VideoRenderTarget,
};
use crate::cancellation::{self, CancellationFlag};
use crate::connection_state::{self, ConnectionState};
use crate::settings::{Action, DEFAULT_SETTINGS_PATH, HeadUnitSettings};

const AOA_TRANSITION_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
/// How long the brightness slider waits after the last drag tick before
/// persisting to disk — see `wire_brightness_and_device_settings`'s doc
/// comment for the real-hardware stutter this avoids. Comfortably longer
/// than a single drag tick (so rapid ticks keep cancelling and
/// rescheduling this instead of each one writing to disk) but short
/// enough that letting go of the slider still feels like it saved
/// immediately.
const BRIGHTNESS_SAVE_DEBOUNCE: Duration = Duration::from_millis(250);
/// Safety net only — normal shutdown happens the moment the session-result
/// poll receives the background thread's outcome. Generous enough to sit
/// above `auth_discovery_probe::run`'s own internal `PROBE_TIMEOUT` (30s)
/// plus AOA-transition margin.
const HANG_SAFETY_NET_SECONDS: u32 = 120;

/// A real gap this fixes: the hang safety net used to be the fixed
/// [`HANG_SAFETY_NET_SECONDS`] constant, unaware of
/// `AA_HEADUNIT_OBSERVATION_WINDOW_SECONDS` — real-hardware trial,
/// 2026-08-16: a 300-second observation window (set to leave enough time
/// for interactive settings-panel testing) was silently cut off at 120
/// seconds by this timer, quitting the whole GTK application — window,
/// process, everything — out from under the operator mid-session, which
/// looked exactly like "the settings gesture stopped working" but was
/// actually the whole process already gone. Now derived from the same
/// override `auth_discovery_probe::run` itself uses, plus a fixed margin,
/// so the two timeouts can never race like that again.
fn hang_safety_net_seconds() -> Result<u32, CliError> {
    match crate::auth_discovery_probe::read_observation_window_override()? {
        Some(duration) => {
            let margin = Duration::from_secs(60);
            Ok(u32::try_from((duration + margin).as_secs()).unwrap_or(u32::MAX))
        }
        None => Ok(HANG_SAFETY_NET_SECONDS),
    }
}

/// Everything `connect_activate`'s closure needs to move out exactly once.
/// `gtk::Application::connect_activate` requires `Fn`, not `FnOnce` — a
/// plain `move` capture can't be moved *out of* an `Fn` closure's own
/// environment, so this is bundled behind `RefCell<Option<..>>` and
/// `.take()`n once inside the callback, after which everything here is an
/// ordinary owned local free to move into the background thread/poll
/// closures.
struct ActivationState {
    selector: String,
    tls12_compatibility: bool,
    hang_safety_net_seconds: u32,
    handoff: Gtk4WindowHandoff,
    touch_settings: TouchSettingsHandoff,
    panel_visibility: SharedPanelVisibility,
    cancel: CancellationFlag,
    session_result_sender: mpsc::Sender<Result<(), CliError>>,
    capability_receiver: mpsc::Receiver<DecoderCapability>,
    pipeline_sender: mpsc::Sender<Result<VideoRenderPipeline, GstreamerError>>,
    session_result_receiver: mpsc::Receiver<Result<(), CliError>>,
    rotation_receiver: mpsc::Receiver<Option<SharedRotation>>,
    arm_window_receiver: mpsc::Receiver<SharedArmWindow>,
    gesture_receiver: mpsc::Receiver<GestureEvent>,
}

pub(crate) fn run(selector: &str, tls12_compatibility: bool) -> Result<(), CliError> {
    let cancel = cancellation::install_ctrlc_handler()?;
    run_with_cancel(selector, tls12_compatibility, cancel)
}

/// Same as [`run`], but takes an already-installed [`CancellationFlag`]
/// instead of installing its own — `ctrlc::set_handler` can only be
/// called once per process (`cancellation::install_ctrlc_handler`'s own
/// doc comment), so a caller that needs to run more than one session in
/// the same process (`usb kiosk`'s boot-time reconnect loop,
/// `main.rs`) has to install the handler itself, once, and reuse the
/// same flag across every session.
pub(crate) fn run_with_cancel(
    selector: &str,
    tls12_compatibility: bool,
    cancel: CancellationFlag,
) -> Result<(), CliError> {
    let hang_safety_net_seconds = hang_safety_net_seconds()?;
    let (capability_sender, capability_receiver) = mpsc::channel::<DecoderCapability>();
    let (pipeline_sender, pipeline_receiver) = mpsc::channel();
    let (session_result_sender, session_result_receiver) = mpsc::channel::<Result<(), CliError>>();
    let (rotation_sender, rotation_receiver) = mpsc::channel::<Option<SharedRotation>>();
    let (arm_window_sender, arm_window_receiver) = mpsc::channel::<SharedArmWindow>();
    let (gesture_sender, gesture_receiver) = mpsc::channel::<GestureEvent>();
    let panel_visibility = SharedPanelVisibility::new(false);

    let activation_state = RefCell::new(Some(ActivationState {
        selector: selector.to_string(),
        tls12_compatibility,
        hang_safety_net_seconds,
        handoff: Gtk4WindowHandoff {
            capability_sender,
            pipeline_receiver,
        },
        touch_settings: TouchSettingsHandoff {
            rotation_sender,
            arm_window_sender,
            gesture_sender,
            panel_visibility: panel_visibility.clone(),
        },
        panel_visibility,
        cancel,
        session_result_sender,
        capability_receiver,
        pipeline_sender,
        session_result_receiver,
        rotation_receiver,
        arm_window_receiver,
        gesture_receiver,
    }));

    let final_result: Rc<RefCell<Option<Result<(), CliError>>>> = Rc::new(RefCell::new(None));
    let final_result_for_activate = Rc::clone(&final_result);

    let application = Application::builder()
        .application_id("dev.pi-auto-headunit.gtk-dev-ui")
        .build();

    application.connect_activate(move |application| {
        let Some(state) = activation_state.borrow_mut().take() else {
            return;
        };
        let poll_state = activate_window(application, state, Rc::clone(&final_result_for_activate));
        wire_session_polls(poll_state);
    });

    // `Application::run()` would otherwise parse `std::env::args()` itself
    // as GLib command-line options — this binary's own `--device`/
    // `--allow-live-aap`/`--tls12-compat` flags, already parsed by
    // `main.rs`'s own dispatcher before this function is ever called,
    // aren't valid GApplication options and made a real trial fail
    // immediately with "Unknown option --device". An empty arg list
    // skips GLib's own parsing entirely.
    let _exit_code = application.run_with_args::<&str>(&[]);

    let result = final_result.borrow_mut().take().unwrap_or_else(|| {
        Err(CliError::Protocol(
            "gtk window closed via hang safety net before the session reported a result".into(),
        ))
    });
    if result.is_err() {
        connection_state::report(ConnectionState::Error);
    }
    result
}

/// Everything `wire_session_polls` needs — bundled into one struct purely
/// to keep `run()` itself under `clippy::too_many_lines`/
/// `clippy::too_many_arguments`, not because these pieces share any
/// behavior beyond "set up once inside `connect_activate`."
struct SessionPollState {
    application: Application,
    window: ApplicationWindow,
    picture: Picture,
    settings_panel: SettingsPanel,
    armed_mask: ArmedMask,
    rotation_handle: Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: Rc<Cell<Rotation>>,
    arm_window_handle: Rc<RefCell<Option<SharedArmWindow>>>,
    is_fullscreen: Rc<Cell<bool>>,
    gesture_settings: Rc<RefCell<HeadUnitSettings>>,
    selector: String,
    tls12_compatibility: bool,
    hang_safety_net_seconds: u32,
    handoff: Gtk4WindowHandoff,
    touch_settings: TouchSettingsHandoff,
    cancel: CancellationFlag,
    session_result_sender: mpsc::Sender<Result<(), CliError>>,
    capability_receiver: mpsc::Receiver<DecoderCapability>,
    pipeline_sender: mpsc::Sender<Result<VideoRenderPipeline, GstreamerError>>,
    session_result_receiver: mpsc::Receiver<Result<(), CliError>>,
    rotation_receiver: mpsc::Receiver<Option<SharedRotation>>,
    arm_window_receiver: mpsc::Receiver<SharedArmWindow>,
    gesture_receiver: mpsc::Receiver<GestureEvent>,
    final_result: Rc<RefCell<Option<Result<(), CliError>>>>,
}

/// Builds the fullscreen window (with the settings-panel overlay) and every
/// piece of state `wire_session_polls` needs, from one just-activated
/// `ActivationState`. Extracted from `connect_activate`'s closure purely
/// to keep `run()` itself under `clippy::too_many_lines`.
/// Loads a small stylesheet defining `.flipped-panel { transform:
/// rotate(180deg); }`, applied to the whole `GtkDisplay`. Real-hardware
/// finding (2026-08-18): flipping the *video* 180° (`set_rotation_degrees`)
/// left the settings panel — a separate GTK overlay drawn directly, never
/// touched by that `GStreamer`-side change — still right-side-up, which
/// the operator correctly flagged as making the flip look like it "made
/// no difference" while the panel filled most of the screen during
/// testing. `apply_rotation` toggles this class on `SettingsPanel::root`
/// to match. Idempotent (safe to call once per window, which is all this
/// needs) — `gtk4::style_context_add_provider_for_display` adds to the
/// display's provider list, not a single widget, so this only needs
/// calling once regardless of how many rotatable widgets end up using the
/// class.
fn install_flipped_panel_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(".flipped-panel { transform: rotate(180deg); }");
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// `picture` (the video widget) has no paintable set until a phone
/// finishes connecting and `build_gtk4_pipeline` runs — before that, and
/// again between sessions in the reconnect loop, it painted nothing at
/// all, showing through to GTK's own default window background (plain
/// white/light grey under the stock theme). Real-hardware feedback: an
/// operator staring at a stark white "waiting for phone" screen read as
/// indistinguishable from the actual white-screen bugs this session fixed
/// elsewhere. Fixed at the application-provider priority (lowest — an
/// operator theme's own `.video-picture` rule, loaded via `apply_theme` at
/// `STYLE_PROVIDER_PRIORITY_APPLICATION` too but installed *after* this
/// one, wins the cascade) so this is purely a sane default, not a ceiling:
/// a theme CSS file can set `background-image: url(...)` on
/// `.video-picture` for a real custom image, using the exact same
/// operator-authored-GTK-CSS mechanism `THEMES_DIR` already documents —
/// no separate image-asset pipeline needed. Once the video paintable is
/// set, `Picture`'s default `content-fit: contain` still lets this show
/// through as letterboxing if the stream's aspect ratio doesn't exactly
/// fill the panel.
fn install_default_video_background_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(".video-picture { background-color: #1a1a1a; }");
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// M6 accessibility/touch-target review, 2026-08-21: every `Button`/
/// `CheckButton` in this file (settings navigation, action picker, theme
/// picker, gesture remap) relied entirely on the stock Adwaita theme's own
/// padding for its tap area — no minimum size was ever set anywhere. On
/// the 800x480 physical panel this project targets (`ARCHITECTURE.md`'s
/// display profile), that padding-only sizing is well under the ~44-48px
/// touch-target minimum widely used for touchscreens (WCAG 2.5.5,
/// Material Design), and this is a head unit meant to be used while
/// driving, not a mouse-driven desktop app. Fixed at the CSS-node-name
/// level (`button`, `checkbutton`) rather than per-callsite so every
/// current and future button/check button in this file is covered
/// automatically, including the ones built in a loop (`build_themes_page`,
/// `build_action_picker`) where a per-callsite class is easy to forget.
/// `spinbutton`'s own internal up/down steppers are plain `button` nodes
/// too (GTK4's own `GtkSpinButton` implementation) and would otherwise
/// balloon to the same 56px alongside a single-line numeric entry, which
/// looks broken rather than accessible — reset back to intrinsic sizing
/// with a more specific `spinbutton button` rule immediately after, which
/// wins the cascade over the general rule regardless of add-order (CSS
/// specificity, not `STYLE_PROVIDER_PRIORITY_APPLICATION`'s add-order tie
/// break, decides between two rules at the same priority once one
/// selector is more specific than the other).
fn install_minimum_touch_target_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(
        "button, checkbutton { min-height: 56px; min-width: 56px; } \
         spinbutton button { min-height: 0; min-width: 0; }",
    );
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

/// Directory an operator drops custom GTK4 CSS theme files into — one
/// `.css` file per theme, discovered and offered on the Themes settings
/// page (`build_themes_page`). Same parent as `DEFAULT_SETTINGS_PATH`
/// (`/var/lib/aa-headunit`, already group-writable for the operator from
/// packaging) rather than a package-shipped location, since a theme is
/// operator-supplied content, not admin config — `settings.rs`'s module
/// doc comment explains the `/etc` vs `/var/lib` split this follows.
/// GTK4's own CSS support (`gtk4::CssProvider`, already used by
/// `install_flipped_panel_css` above) is the "common standard" this
/// builds on rather than a project-specific format: a theme is real,
/// ordinary GTK CSS, documented externally, not something this project
/// invents.
const THEMES_DIR: &str = "/var/lib/aa-headunit/themes";

/// Every `.css` file directly inside [`THEMES_DIR`], as theme names (the
/// file's stem — no directory, no extension), sorted for a stable menu
/// order. Empty on any failure (missing directory, unreadable) — matches
/// `list_pulse_devices`'s existing "empty on any failure" precedent; an
/// operator with no themes installed just sees "System default" alone.
fn list_theme_names() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(THEMES_DIR) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("css") {
                return None;
            }
            path.file_stem()?.to_str().map(str::to_string)
        })
        .collect();
    names.sort();
    names
}

/// Removes whichever theme `CssProvider` is currently active (if any)
/// and, if `name` is `Some`, loads and installs `{THEMES_DIR}/{name}.css`
/// in its place. `None` — or a name whose file no longer exists — leaves
/// the display with no custom theme (the ordinary GTK4 default) rather
/// than erroring, matching `build_device_dropdown`'s existing
/// "falls back to the default" precedent for a setting naming something
/// no longer present.
fn apply_theme(active_theme_provider: &Rc<RefCell<Option<gtk4::CssProvider>>>, name: Option<&str>) {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    if let Some(previous) = active_theme_provider.borrow_mut().take() {
        gtk4::style_context_remove_provider_for_display(&display, &previous);
    }
    let Some(name) = name else {
        return;
    };
    let path = Path::new(THEMES_DIR).join(format!("{name}.css"));
    if !path.is_file() {
        return;
    }
    let provider = gtk4::CssProvider::new();
    provider.load_from_path(&path);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    active_theme_provider.replace(Some(provider));
}

fn activate_window(
    application: &Application,
    state: ActivationState,
    final_result: Rc<RefCell<Option<Result<(), CliError>>>>,
) -> SessionPollState {
    let ActivationState {
        selector,
        tls12_compatibility,
        hang_safety_net_seconds,
        handoff,
        touch_settings,
        panel_visibility,
        cancel,
        session_result_sender,
        capability_receiver,
        pipeline_sender,
        session_result_receiver,
        rotation_receiver,
        arm_window_receiver,
        gesture_receiver,
    } = state;

    let gesture_settings: Rc<RefCell<HeadUnitSettings>> = Rc::new(RefCell::new(
        HeadUnitSettings::load(Path::new(DEFAULT_SETTINGS_PATH)),
    ));

    install_flipped_panel_css();
    install_default_video_background_css();
    install_minimum_touch_target_css();
    let picture = Picture::new();
    picture.add_css_class("video-picture");
    let overlay = Overlay::new();
    overlay.set_child(Some(&picture));
    let settings_panel = build_settings_panel_from(&gesture_settings);
    apply_theme(
        &settings_panel.active_theme_provider,
        gesture_settings.borrow().theme(),
    );
    overlay.add_overlay(&settings_panel.root);
    // `settings_panel.root`'s own visibility is the single source of
    // truth for whether the panel is showing (see `SharedPanelVisibility`'s
    // doc comment, `auth_discovery_probe.rs`, for why this is mirrored
    // into a cross-thread flag rather than read directly — the panel
    // lives on this thread, the touch forwarding it gates lives on the
    // background protocol thread). `connect_visible_notify` fires for
    // every `set_visible` call anywhere in this file (open, close,
    // fullscreen-toggle), so this one connection covers all of them.
    settings_panel
        .root
        .connect_visible_notify(move |root| panel_visibility.set(root.is_visible()));
    let armed_mask = build_armed_mask();
    overlay.add_overlay(&armed_mask.root);

    build_and_wire_experimental_disclaimer(&overlay, &gesture_settings);

    let window = ApplicationWindow::builder()
        .application(application)
        .title("pi-auto-headunit live session")
        .child(&overlay)
        .build();
    window.fullscreen();
    window.present();

    let rotation_handle: Rc<RefCell<Option<SharedRotation>>> = Rc::new(RefCell::new(None));
    // Loaded from settings, not always `Normal` — M5's persisted
    // rotation (`crate::settings`). `rotation_label`'s text is set to
    // match right below, and the loaded value is applied to the real
    // `SharedRotation` handle once it becomes available (see the
    // `rotation_receiver` poll below).
    let current_rotation: Rc<Cell<Rotation>> =
        Rc::new(Cell::new(gesture_settings.borrow().rotation()));
    settings_panel
        .rotation_label
        .set_text(rotation_label_text(current_rotation.get()));
    if current_rotation.get() == Rotation::Flipped180 {
        settings_panel.root.add_css_class("flipped-panel");
        armed_mask.root.add_css_class("flipped-panel");
    }
    let arm_window_handle: Rc<RefCell<Option<SharedArmWindow>>> = Rc::new(RefCell::new(None));
    // The window is created fullscreen above.
    let is_fullscreen: Rc<Cell<bool>> = Rc::new(Cell::new(true));

    wire_settings_panel(
        &settings_panel,
        &armed_mask,
        &window,
        &rotation_handle,
        &current_rotation,
        &arm_window_handle,
        &is_fullscreen,
        &gesture_settings,
    );

    SessionPollState {
        application: application.clone(),
        window,
        picture,
        settings_panel,
        armed_mask,
        rotation_handle,
        current_rotation,
        arm_window_handle,
        is_fullscreen,
        gesture_settings,
        selector,
        tls12_compatibility,
        hang_safety_net_seconds,
        handoff,
        touch_settings,
        cancel,
        session_result_sender,
        capability_receiver,
        pipeline_sender,
        session_result_receiver,
        rotation_receiver,
        arm_window_receiver,
        gesture_receiver,
        final_result,
    }
}

/// Polls `gesture_receiver` and reacts to each `GestureEvent`: shows/hides
/// the armed-state mask, and for a completed gesture, looks up and
/// dispatches its currently-mapped [`Action`]. Extracted from
/// `wire_session_polls` purely to keep it under `clippy::too_many_lines`.
///
/// Self-terminates (`glib::ControlFlow::Break`) once `gesture_receiver`
/// disconnects, matching every other per-session poll in this file
/// (`capability`/`rotation`/`arm_window`) — previously always returned
/// `Continue` regardless, which was harmless for `run`/`run_with_cancel`
/// (the whole `Application` quits shortly after any one session ends
/// anyway) but would leak one perpetual timer per reconnect once
/// `usb kiosk`'s window started persisting across many sessions
/// (2026-08-21, `run_kiosk`) — found and fixed while building that,
/// before it ever shipped.
#[allow(clippy::too_many_arguments)]
fn wire_gesture_poll(
    gesture_receiver: mpsc::Receiver<GestureEvent>,
    settings_panel: SettingsPanel,
    armed_mask: ArmedMask,
    window: ApplicationWindow,
    rotation_handle: Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: Rc<Cell<Rotation>>,
    is_fullscreen: Rc<Cell<bool>>,
    gesture_settings: Rc<RefCell<HeadUnitSettings>>,
) -> glib::SourceId {
    glib::timeout_add_local(POLL_INTERVAL, move || {
        loop {
            match gesture_receiver.try_recv() {
                Ok(event) => match event {
                    GestureEvent::Armed => armed_mask.root.set_visible(true),
                    GestureEvent::Disarmed => armed_mask.root.set_visible(false),
                    GestureEvent::Completed(gesture) => {
                        armed_mask.root.set_visible(false);
                        let action = gesture_settings.borrow().action_for(gesture);
                        dispatch_action(
                            action,
                            &settings_panel,
                            &armed_mask,
                            &window,
                            &rotation_handle,
                            &current_rotation,
                            &is_fullscreen,
                            &gesture_settings,
                        );
                    }
                },
                Err(mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
            }
        }
    })
}

/// Real-hardware finding (2026-08-19): this used to be a bare one-shot
/// `glib::timeout_add_seconds_local` inlined at the end of
/// `wire_session_polls`, unconditionally calling `application.quit()`
/// once `hang_safety_net_seconds` elapsed — full stop, with no check of
/// whether the session was actually hung. Every session, healthy or not,
/// got force-quit at exactly that mark; `usb kiosk`'s own outer
/// reconnect loop immediately relaunched a fresh one, so a real phone
/// session looked like a recurring white-screen blip every
/// `HANG_SAFETY_NET_SECONDS` (120s) rather than a one-time crash —
/// confirmed against the operator's own real-hardware report ("white
/// screen... after 2 mins of AA being active", matching
/// `HANG_SAFETY_NET_SECONDS` to the second). This module's own doc
/// comment on `HANG_SAFETY_NET_SECONDS` already stated the intended
/// design — "Safety net only — normal shutdown happens the moment the
/// session-result poll receives the background thread's outcome" — the
/// bug was that the timer never actually got cancelled once that normal
/// path was confirmed working. The returned `SourceId` handle is
/// cancelled by `wire_session_polls`'s capability poll the first time it
/// sees a real `DecoderCapability` (proof video setup — the AOA
/// transition, protocol handshake, and TLS/service-discovery — completed
/// and the session is no longer just sitting in early setup); after
/// that, only an actual session-result (a real error, or an
/// operator-requested stop) can end the session, matching the doc
/// comment's intent. Sessions that never get that far (a genuine early
/// hang) are still protected exactly as before.
fn install_hang_safety_net(
    application: &Application,
    hang_safety_net_seconds: u32,
) -> Rc<RefCell<Option<glib::SourceId>>> {
    let application = application.clone();
    Rc::new(RefCell::new(Some(glib::timeout_add_seconds_local(
        hang_safety_net_seconds,
        move || {
            application.quit();
            glib::ControlFlow::Break
        },
    ))))
}

/// Spawns the background protocol thread and wires every poll bridging it
/// (and the settings/rotation gesture machinery) back to the GTK main
/// thread. Extracted from `connect_activate`'s closure purely to keep
/// `run()` itself under `clippy::too_many_lines`.
fn wire_session_polls(state: SessionPollState) {
    let SessionPollState {
        application,
        window,
        picture,
        settings_panel,
        armed_mask,
        rotation_handle,
        current_rotation,
        arm_window_handle,
        is_fullscreen,
        gesture_settings,
        selector,
        tls12_compatibility,
        hang_safety_net_seconds,
        handoff,
        touch_settings,
        cancel,
        session_result_sender,
        capability_receiver,
        pipeline_sender,
        session_result_receiver,
        rotation_receiver,
        arm_window_receiver,
        gesture_receiver,
        final_result,
    } = state;

    let safety_net_id = install_hang_safety_net(&application, hang_safety_net_seconds);

    let safety_net_id_for_capability = Rc::clone(&safety_net_id);
    let _capability_poll_id = glib::timeout_add_local(POLL_INTERVAL, move || {
        match capability_receiver.try_recv() {
            Ok(capability) => {
                if let Some(id) = safety_net_id_for_capability.borrow_mut().take() {
                    id.remove();
                }
                let outcome = build_gtk4_pipeline(&capability, &picture);
                let _ = pipeline_sender.send(outcome);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    let rotation_handle_for_poll = Rc::clone(&rotation_handle);
    let current_rotation_for_poll = Rc::clone(&current_rotation);
    let _rotation_poll_id =
        glib::timeout_add_local(POLL_INTERVAL, move || match rotation_receiver.try_recv() {
            Ok(handle) => {
                // Apply the settings-loaded rotation the instant a real
                // `SharedRotation` becomes available — the touch reader
                // thread only starts reporting frames through it from
                // here on, so this is the earliest point a persisted
                // non-zero rotation can actually take effect.
                if let Some(handle) = &handle {
                    handle.set(current_rotation_for_poll.get());
                }
                *rotation_handle_for_poll.borrow_mut() = handle;
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });

    let arm_window_handle_for_poll = Rc::clone(&arm_window_handle);
    let _arm_window_poll_id = glib::timeout_add_local(POLL_INTERVAL, move || {
        match arm_window_receiver.try_recv() {
            Ok(handle) => {
                *arm_window_handle_for_poll.borrow_mut() = Some(handle);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    let _gesture_poll_id = wire_gesture_poll(
        gesture_receiver,
        settings_panel.clone(),
        armed_mask.clone(),
        window.clone(),
        Rc::clone(&rotation_handle),
        Rc::clone(&current_rotation),
        Rc::clone(&is_fullscreen),
        Rc::clone(&gesture_settings),
    );

    thread::spawn(move || {
        let result = run_session(
            &selector,
            tls12_compatibility,
            handoff,
            touch_settings,
            &cancel,
        );
        let _ = session_result_sender.send(result);
    });

    let final_result_for_poll = Rc::clone(&final_result);
    let application_for_poll = application.clone();
    let _result_poll_id = glib::timeout_add_local(POLL_INTERVAL, move || {
        match session_result_receiver.try_recv() {
            Ok(result) => {
                *final_result_for_poll.borrow_mut() = Some(result);
                application_for_poll.quit();
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

/// How often `usb kiosk` polls for a phone when none is currently
/// connected — unchanged from the discovery interval `usb_kiosk` always
/// used in `main.rs` before its reconnect loop moved here.
const KIOSK_DISCOVERY_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// How long `usb kiosk` waits after one session attempt ends (success or
/// failure) before starting the next — unchanged from `main.rs`'s
/// previous `RECONNECT_DELAY`.
const KIOSK_RECONNECT_DELAY_SECONDS: u32 = 2;

/// Everything built exactly once, for the whole lifetime of the
/// `usb kiosk` process — see [`run_kiosk`]'s doc comment for why this now
/// persists across reconnects instead of being rebuilt every cycle the
/// way [`run`]/[`run_with_cancel`] still do (deliberately unchanged
/// there: `usb gtk-dev-ui --device X` targets one specific
/// operator-supplied device for one session and should still exit, not
/// loop forever).
struct KioskWindow {
    application: Application,
    window: ApplicationWindow,
    picture: Picture,
    settings_panel: SettingsPanel,
    armed_mask: ArmedMask,
    rotation_handle: Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: Rc<Cell<Rotation>>,
    arm_window_handle: Rc<RefCell<Option<SharedArmWindow>>>,
    is_fullscreen: Rc<Cell<bool>>,
    gesture_settings: Rc<RefCell<HeadUnitSettings>>,
    panel_visibility: SharedPanelVisibility,
}

/// Builds the fullscreen window, its overlays, and every widget
/// `usb kiosk` needs — once, not once per reconnect. Mirrors the
/// non-session parts of [`activate_window`] exactly (CSS, `Picture`,
/// `Overlay`, `SettingsPanel`, `ArmedMask`, the experimental disclaimer,
/// the fullscreen `ApplicationWindow`, `wire_settings_panel`); kept as
/// its own function rather than reused directly because `activate_window`
/// intertwines this with per-session channel setup in a way that can't
/// be split without changing its own still-correct behavior for
/// [`run`]/[`run_with_cancel`].
fn build_kiosk_window(application: &Application) -> KioskWindow {
    let gesture_settings: Rc<RefCell<HeadUnitSettings>> = Rc::new(RefCell::new(
        HeadUnitSettings::load(Path::new(DEFAULT_SETTINGS_PATH)),
    ));

    install_flipped_panel_css();
    install_default_video_background_css();
    install_minimum_touch_target_css();
    let picture = Picture::new();
    picture.add_css_class("video-picture");
    let overlay = Overlay::new();
    overlay.set_child(Some(&picture));
    let settings_panel = build_settings_panel_from(&gesture_settings);
    apply_theme(
        &settings_panel.active_theme_provider,
        gesture_settings.borrow().theme(),
    );
    overlay.add_overlay(&settings_panel.root);
    let panel_visibility = SharedPanelVisibility::new(false);
    let panel_visibility_for_notify = panel_visibility.clone();
    settings_panel
        .root
        .connect_visible_notify(move |root| panel_visibility_for_notify.set(root.is_visible()));
    let armed_mask = build_armed_mask();
    overlay.add_overlay(&armed_mask.root);

    build_and_wire_experimental_disclaimer(&overlay, &gesture_settings);

    let window = ApplicationWindow::builder()
        .application(application)
        .title("pi-auto-headunit live session")
        .child(&overlay)
        .build();
    window.fullscreen();
    window.present();

    let rotation_handle: Rc<RefCell<Option<SharedRotation>>> = Rc::new(RefCell::new(None));
    let current_rotation: Rc<Cell<Rotation>> =
        Rc::new(Cell::new(gesture_settings.borrow().rotation()));
    settings_panel
        .rotation_label
        .set_text(rotation_label_text(current_rotation.get()));
    if current_rotation.get() == Rotation::Flipped180 {
        settings_panel.root.add_css_class("flipped-panel");
        armed_mask.root.add_css_class("flipped-panel");
    }
    let arm_window_handle: Rc<RefCell<Option<SharedArmWindow>>> = Rc::new(RefCell::new(None));
    let is_fullscreen: Rc<Cell<bool>> = Rc::new(Cell::new(true));

    wire_settings_panel(
        &settings_panel,
        &armed_mask,
        &window,
        &rotation_handle,
        &current_rotation,
        &arm_window_handle,
        &is_fullscreen,
        &gesture_settings,
    );

    KioskWindow {
        application: application.clone(),
        window,
        picture,
        settings_panel,
        armed_mask,
        rotation_handle,
        current_rotation,
        arm_window_handle,
        is_fullscreen,
        gesture_settings,
        panel_visibility,
    }
}

/// Same discovery strategy `main.rs`'s original `find_candidate_device`
/// used before `usb kiosk`'s reconnect loop moved here (2026-08-21, the
/// real black-screen hang fix — see [`run_kiosk`]'s doc comment): prefer
/// a device already in AOA accessory mode (the common case right after a
/// reconnect), otherwise probe every other non-root-hub device with the
/// documented AOA `GetProtocol` vendor request. `Ok(None)` (not an
/// error) means no candidate yet — the expected steady state for a head
/// unit at boot with no phone plugged in.
fn find_kiosk_candidate_device()
-> Result<Option<transport_api::UsbDeviceId>, transport_api::AoaError> {
    use transport_api::AoaBackend;
    const LINUX_FOUNDATION_ROOT_HUB_VENDOR_ID: u16 = 0x1d6b;

    let mut backend = transport_usb::LibUsbAoaBackend::new()?;
    let devices = backend.list_devices()?;
    if let Some(device) = devices
        .iter()
        .find(|device| transport_usb::is_accessory_id(device.vendor_id, device.product_id))
    {
        return Ok(Some(device.clone()));
    }
    for device in &devices {
        if device.vendor_id == LINUX_FOUNDATION_ROOT_HUB_VENDOR_ID {
            continue;
        }
        if backend.query_protocol(device).is_ok() {
            return Ok(Some(device.clone()));
        }
    }
    Ok(None)
}

/// The kiosk background thread's full body for one attempt: poll for a
/// candidate device, then run one `AAP` session against it. Combines what
/// used to be `usb_kiosk`'s own discovery loop (`main.rs`) with
/// `run_session` — moved here so the whole reconnect loop lives on one
/// background thread per attempt instead of a synchronous loop in
/// `main.rs` that used to rebuild the GTK window every cycle.
fn discover_and_run_kiosk_session(
    tls12_compatibility: bool,
    handoff: Gtk4WindowHandoff,
    touch_settings: TouchSettingsHandoff,
    cancel: &CancellationFlag,
) -> Result<(), CliError> {
    let selector = loop {
        if cancel.is_set() {
            return Ok(());
        }
        match find_kiosk_candidate_device() {
            Ok(Some(device)) => break format!("{}:{}", device.bus, device.address),
            Ok(None) => thread::sleep(KIOSK_DISCOVERY_POLL_INTERVAL),
            Err(error) => {
                println!("probe_state=kiosk_discovery_failed error={error}");
                thread::sleep(KIOSK_DISCOVERY_POLL_INTERVAL);
            }
        }
    };
    println!("probe_state=kiosk_device_selected selector={selector}");
    run_session(
        &selector,
        tls12_compatibility,
        handoff,
        touch_settings,
        cancel,
    )
}

/// Installs this attempt's hang safety net — same purpose and same
/// derived duration as [`install_hang_safety_net`], but on firing it ends
/// *this attempt* via [`end_kiosk_attempt`] (schedule a retry, or quit if
/// cancelled) instead of unconditionally quitting the whole application,
/// since the window now outlives any single attempt.
#[allow(clippy::too_many_arguments)]
fn install_kiosk_hang_safety_net(
    window: Rc<KioskWindow>,
    tls12_compatibility: bool,
    hang_safety_net_seconds: u32,
    cancel: CancellationFlag,
    final_result: Rc<RefCell<Option<Result<(), CliError>>>>,
    attempt_ended: Rc<Cell<bool>>,
) -> Rc<RefCell<Option<glib::SourceId>>> {
    let safety_net_id: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let safety_net_id_for_install = Rc::clone(&safety_net_id);
    let safety_net_id_for_end = Rc::clone(&safety_net_id);
    *safety_net_id_for_install.borrow_mut() = Some(glib::timeout_add_seconds_local(
        hang_safety_net_seconds,
        move || {
            end_kiosk_attempt(
                Rc::clone(&window),
                tls12_compatibility,
                hang_safety_net_seconds,
                cancel.clone(),
                Rc::clone(&final_result),
                &attempt_ended,
                &safety_net_id_for_end,
                Err(CliError::Protocol(
                    "kiosk session attempt hit the hang safety net with no result".into(),
                )),
            );
            glib::ControlFlow::Break
        },
    ));
    safety_net_id
}

/// Runs exactly once per session attempt, however it ended (a real
/// result from `session_result_receiver`, or the hang safety net firing
/// with none ever arriving) — guarded by `attempt_ended` against running
/// twice for the same attempt (both can legitimately fire; only the
/// first should act). Safe without a lock: every closure this file
/// registers runs on the GTK main thread, and `glib` never runs two
/// callbacks concurrently. Resets the window back to a clean
/// "waiting for a phone" state (clears the video paintable, drops the
/// now-stale rotation/arm-window handles from the ended session) without
/// touching the window/`Application` themselves, then either quits (if
/// cancelled) or schedules the next attempt after
/// [`KIOSK_RECONNECT_DELAY_SECONDS`] via [`start_kiosk_session`] — the
/// core of what this whole redesign is for: reconnecting never rebuilds
/// the window.
#[allow(clippy::too_many_arguments)]
fn end_kiosk_attempt(
    window: Rc<KioskWindow>,
    tls12_compatibility: bool,
    hang_safety_net_seconds: u32,
    cancel: CancellationFlag,
    final_result: Rc<RefCell<Option<Result<(), CliError>>>>,
    attempt_ended: &Rc<Cell<bool>>,
    safety_net_id: &Rc<RefCell<Option<glib::SourceId>>>,
    result: Result<(), CliError>,
) {
    if attempt_ended.replace(true) {
        return;
    }
    if let Some(id) = safety_net_id.borrow_mut().take() {
        id.remove();
    }
    match &result {
        Ok(()) => println!("probe_state=kiosk_session_ended"),
        Err(error) => println!("probe_state=kiosk_session_ended error={error}"),
    }

    window
        .picture
        .set_paintable(Option::<&gtk4::gdk::Paintable>::None);
    *window.rotation_handle.borrow_mut() = None;
    *window.arm_window_handle.borrow_mut() = None;

    if cancel.is_set() {
        *final_result.borrow_mut() = Some(result);
        window.application.quit();
        return;
    }

    glib::timeout_add_seconds_local(KIOSK_RECONNECT_DELAY_SECONDS, move || {
        start_kiosk_session(
            &window,
            tls12_compatibility,
            hang_safety_net_seconds,
            &cancel,
            &final_result,
        );
        glib::ControlFlow::Break
    });
}

/// Starts one `AAP` session attempt against the persistent
/// [`KioskWindow`]: discovers a device and runs the session on a
/// background thread, and wires every poll needed to bridge that thread
/// back to the (already-existing, reused) window widgets — mirrors
/// [`wire_session_polls`] closely, differing only in reusing `window`'s
/// widgets instead of building fresh ones, and in routing session-end
/// through [`end_kiosk_attempt`] instead of unconditionally quitting.
/// Re-invoked by `end_kiosk_attempt` for every subsequent reconnect, so
/// the whole `usb kiosk` process only ever calls [`build_kiosk_window`]
/// once.
/// Wires the rotation and arm-window one-shot handle deliveries into
/// `window`'s persistent state — split out of [`start_kiosk_session`]
/// purely to keep it under `clippy::too_many_lines`. Each poll
/// self-terminates once it delivers (`Ok` → `Break`) or the sending
/// session ends without ever delivering one (`Disconnected` → `Break`),
/// same as every other per-session poll in this file.
fn wire_kiosk_handle_polls(
    window: &Rc<KioskWindow>,
    rotation_receiver: mpsc::Receiver<Option<SharedRotation>>,
    arm_window_receiver: mpsc::Receiver<SharedArmWindow>,
) {
    let rotation_handle_for_poll = Rc::clone(&window.rotation_handle);
    let current_rotation_for_poll = Rc::clone(&window.current_rotation);
    let _rotation_poll_id =
        glib::timeout_add_local(POLL_INTERVAL, move || match rotation_receiver.try_recv() {
            Ok(handle) => {
                if let Some(handle) = &handle {
                    handle.set(current_rotation_for_poll.get());
                }
                *rotation_handle_for_poll.borrow_mut() = handle;
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        });

    let arm_window_handle_for_poll = Rc::clone(&window.arm_window_handle);
    let _arm_window_poll_id = glib::timeout_add_local(POLL_INTERVAL, move || {
        match arm_window_receiver.try_recv() {
            Ok(handle) => {
                *arm_window_handle_for_poll.borrow_mut() = Some(handle);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

fn start_kiosk_session(
    window: &Rc<KioskWindow>,
    tls12_compatibility: bool,
    hang_safety_net_seconds: u32,
    cancel: &CancellationFlag,
    final_result: &Rc<RefCell<Option<Result<(), CliError>>>>,
) {
    let (capability_sender, capability_receiver) = mpsc::channel::<DecoderCapability>();
    let (pipeline_sender, pipeline_receiver) = mpsc::channel();
    let (session_result_sender, session_result_receiver) = mpsc::channel::<Result<(), CliError>>();
    let (rotation_sender, rotation_receiver) = mpsc::channel::<Option<SharedRotation>>();
    let (arm_window_sender, arm_window_receiver) = mpsc::channel::<SharedArmWindow>();
    let (gesture_sender, gesture_receiver) = mpsc::channel::<GestureEvent>();

    let handoff = Gtk4WindowHandoff {
        capability_sender,
        pipeline_receiver,
    };
    let touch_settings = TouchSettingsHandoff {
        rotation_sender,
        arm_window_sender,
        gesture_sender,
        panel_visibility: window.panel_visibility.clone(),
    };

    let attempt_ended = Rc::new(Cell::new(false));
    let safety_net_id = install_kiosk_hang_safety_net(
        Rc::clone(window),
        tls12_compatibility,
        hang_safety_net_seconds,
        cancel.clone(),
        Rc::clone(final_result),
        Rc::clone(&attempt_ended),
    );

    let safety_net_id_for_capability = Rc::clone(&safety_net_id);
    let picture_for_capability = window.picture.clone();
    let _capability_poll_id = glib::timeout_add_local(POLL_INTERVAL, move || {
        match capability_receiver.try_recv() {
            Ok(capability) => {
                if let Some(id) = safety_net_id_for_capability.borrow_mut().take() {
                    id.remove();
                }
                let outcome = build_gtk4_pipeline(&capability, &picture_for_capability);
                let _ = pipeline_sender.send(outcome);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    wire_kiosk_handle_polls(window, rotation_receiver, arm_window_receiver);

    let _gesture_poll_id = wire_gesture_poll(
        gesture_receiver,
        window.settings_panel.clone(),
        window.armed_mask.clone(),
        window.window.clone(),
        Rc::clone(&window.rotation_handle),
        Rc::clone(&window.current_rotation),
        Rc::clone(&window.is_fullscreen),
        Rc::clone(&window.gesture_settings),
    );

    let cancel_for_thread = cancel.clone();
    thread::spawn(move || {
        let result = discover_and_run_kiosk_session(
            tls12_compatibility,
            handoff,
            touch_settings,
            &cancel_for_thread,
        );
        let _ = session_result_sender.send(result);
    });

    let window_for_result = Rc::clone(window);
    let final_result_for_result = Rc::clone(final_result);
    let cancel_for_result = cancel.clone();
    let attempt_ended_for_result = Rc::clone(&attempt_ended);
    let safety_net_id_for_result = Rc::clone(&safety_net_id);
    let _result_poll_id = glib::timeout_add_local(POLL_INTERVAL, move || {
        match session_result_receiver.try_recv() {
            Ok(result) => {
                end_kiosk_attempt(
                    Rc::clone(&window_for_result),
                    tls12_compatibility,
                    hang_safety_net_seconds,
                    cancel_for_result.clone(),
                    Rc::clone(&final_result_for_result),
                    &attempt_ended_for_result,
                    &safety_net_id_for_result,
                    result,
                );
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

/// Entry point for `usb kiosk --allow-live-aap [--tls12-compat]` — the
/// unattended, boot-time, reconnect-forever equivalent of
/// [`run`]/[`run_with_cancel`]. Builds the fullscreen window exactly once
/// ([`build_kiosk_window`]) and keeps it alive for the whole process
/// lifetime, running one `AAP` session attempt after another against it
/// ([`start_kiosk_session`]) without ever tearing down or recreating the
/// window/`Application` in between.
///
/// **Why this exists (2026-08-21)**: the previous design called
/// `run_with_cancel` fresh for every single reconnect attempt from a loop
/// in `main.rs` — each call built a brand new `gtk4::Application` and
/// window from scratch and tore it down (`application.quit()`) when the
/// attempt ended, whether it succeeded or failed. A real black-screen
/// hang, needing a physical reboot to recover from, was reproduced when
/// this hit the known reconnect-race
/// (`docs/protocol/session-supervisor-reconnect-race.md`) from a cold
/// start: several consecutive failed cycles meant creating and
/// destroying a full window/Wayland surface every 2-4 seconds, many
/// times in a row — far more rapid and repetitive than any
/// previously-tested long-lived-session scenario. The working
/// hypothesis: if that teardown isn't fully synchronous with the
/// compositor, hammering it that fast can desync or exhaust `labwc`
/// itself. This redesign removes the repeated window churn entirely —
/// only the protocol session (background thread, video pipeline,
/// per-session channels) gets rebuilt on reconnect; the window, its
/// overlays, and their GTK state persist untouched for the whole process
/// lifetime.
///
/// **Unconfirmed on real hardware as of this writing** — the hypothesis
/// this fixes was never proven, only reasoned from code; treat this the
/// way this project treats every other reverse-engineered/unconfirmed
/// behavior: a real-hardware trial is needed before trusting it,
/// including specifically retrying the reconnect-race scenario that
/// caused the original hang.
pub(crate) fn run_kiosk(
    tls12_compatibility: bool,
    cancel: &CancellationFlag,
) -> Result<(), CliError> {
    let hang_safety_net_seconds = hang_safety_net_seconds()?;
    let final_result: Rc<RefCell<Option<Result<(), CliError>>>> = Rc::new(RefCell::new(None));

    let application = Application::builder()
        .application_id("dev.pi-auto-headunit.gtk-dev-ui")
        .build();

    // `connect_activate` requires `Fn`, callable more than once in
    // principle — this guard (mirroring `run`/`run_with_cancel`'s own
    // `RefCell<Option<..>>.take()` guard against the same theoretical
    // double-activation) ensures the window is still only ever built
    // once even if it were.
    let already_started = Rc::new(Cell::new(false));
    let final_result_for_activate = Rc::clone(&final_result);
    let cancel_for_activate = cancel.clone();
    application.connect_activate(move |application| {
        if already_started.replace(true) {
            return;
        }
        let window = Rc::new(build_kiosk_window(application));
        start_kiosk_session(
            &window,
            tls12_compatibility,
            hang_safety_net_seconds,
            &cancel_for_activate,
            &final_result_for_activate,
        );
    });

    let _exit_code = application.run_with_args::<&str>(&[]);

    let result = final_result.borrow_mut().take().unwrap_or(Ok(()));
    if result.is_err() {
        connection_state::report(ConnectionState::Error);
    }
    result
}

/// One gesture's action-assignment control: a plain `Button` labeled with
/// the gesture's currently assigned action. Tapping it opens
/// [`ActionPicker`] — see that type's doc comment for why this ended up
/// as the reliable design after two more elaborate attempts each broke a
/// different way on real touch hardware.
#[derive(Clone)]
struct GestureSelector {
    change_button: Button,
}

/// Every gesture's action-assignment control, for every gesture.
type GestureSelectors = Vec<(GestureId, GestureSelector)>;

/// A full-panel action picker: tapping a gesture's [`GestureSelector`]
/// hides [`SettingsPanel::gestures_page`] and shows this instead, with
/// seven big, ordinary `Button`s — one per `Action` — plus a "Back"
/// button. Tapping an action applies it to whichever gesture is
/// currently being edited ([`Self::editing_gesture`], set the moment the
/// picker opens) and returns to the gestures page; "Back" returns
/// without changing anything. Both pages share the settings panel's
/// single outer `ScrolledWindow` — real-hardware feedback, 2026-08-16:
/// two earlier designs (a popup `DropDown`, then a `MenuButton`+`Popover`
/// holding a `ListBox` in its own nested `ScrolledWindow` with a
/// hand-rolled touch-drag-to-scroll gesture) each broke in a different,
/// real way on this project's touchscreen — the `DropDown` clipped
/// options off screen, and the popover version's nested
/// scroll-vs-row-click gesture arbitration misfired, silently
/// reassigning several gestures to the wrong action while the operator
/// was trying to scroll. Plain, ordinary buttons in one already-reliable
/// scrollable page have no such gesture to arbitrate — every tap
/// unambiguously means "select this."
#[derive(Clone)]
struct ActionPicker {
    root: GtkBox,
    title: Label,
    action_buttons: Rc<Vec<(Action, Button)>>,
    back_button: Button,
    editing_gesture: Rc<Cell<Option<GestureId>>>,
}

/// Every page the settings panel can show. At the operator's explicit
/// request (2026-08-19), the panel opened by the arm-swipe-then-gesture
/// is a top-level menu of buttons, each leading to its own page, rather
/// than one long flat page of every control — the gesture-assignment
/// controls and the previously-flat display/audio/mtp/night-mode
/// controls split out into their own `Gestures`/`Display` pages;
/// `Themes` is a real page (`build_themes_page`) picking a GTK4 CSS
/// stylesheet from `THEMES_DIR`; the remaining four sibling pages exist
/// as placeholders for planned features not yet implemented.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    Menu,
    Gestures,
    Display,
    Themes,
    Equalizer,
    RearCamera,
    DashCam,
    ScreenMirroring,
    Legal,
}

/// One of the settings menu's not-yet-implemented sibling pages (EQ,
/// Rear Camera, `DashCam`, Screen Mirroring) — just a name and a Back
/// button for now; real controls land here as each feature is actually
/// built. All four share this identical shape, so they're built and
/// wired in a loop (see `build_stub_page`) rather than as four separate
/// named struct fields.
#[derive(Clone)]
struct StubPage {
    page: SettingsPage,
    root: GtkBox,
    open_button: Button,
    back_button: Button,
}

/// The settings panel's widgets, kept together so `run()`'s closures can
/// show/hide it and read its controls without threading five separate
/// widget handles around. `Clone` is cheap (GTK widgets are themselves
/// reference-counted handles).
#[derive(Clone)]
struct SettingsPanel {
    root: ScrolledWindow,
    menu_page: GtkBox,
    gestures_page: GtkBox,
    gestures_button: Button,
    gestures_back_button: Button,
    display_page: GtkBox,
    display_button: Button,
    display_back_button: Button,
    themes_page: GtkBox,
    themes_button: Button,
    themes_back_button: Button,
    theme_buttons: Rc<Vec<(Option<String>, Button)>>,
    active_theme_provider: Rc<RefCell<Option<gtk4::CssProvider>>>,
    equalizer_page: GtkBox,
    equalizer_button: Button,
    equalizer_back_button: Button,
    /// One `Scale` per `equalizer-10bands` band, in `band0..band9` order —
    /// see `build_equalizer_page`'s doc comment.
    eq_scales: Rc<Vec<Scale>>,
    stub_pages: Rc<Vec<StubPage>>,
    rotation_label: Label,
    gesture_selectors: Rc<GestureSelectors>,
    picker: ActionPicker,
    /// The "Android Auto" entry in the top-level settings menu grid —
    /// dismisses the settings panel to reveal the live AA view
    /// underneath, exactly like the plain "Close" button this replaced
    /// (2026-08-21, extras roadmap: AA is a menu entry like any other
    /// page — Gestures/Display/Themes/etc. — not a fixed backdrop
    /// everything else sits on top of). The live video/audio pipeline
    /// was never paused by the settings panel being open in the first
    /// place, so this is a pure navigation change — nothing to restart.
    android_auto_button: Button,
    toggle_fullscreen_button: Button,
    flip_screen_button: Button,
    arm_timeout_spin: SpinButton,
    mtp_suppression_check: CheckButton,
    launch_on_boot_check: CheckButton,
    brightness_scale: Scale,
    audio_output_dropdown: DropDown,
    audio_output_devices: Rc<Vec<String>>,
    microphone_input_dropdown: DropDown,
    microphone_input_devices: Rc<Vec<String>>,
    night_mode_gpio_enabled_check: CheckButton,
    night_mode_gpio_spin: SpinButton,
}

/// Hides every settings page except `page`, including the top-level
/// menu itself — navigating to any page always leaves exactly one
/// visible. Does not touch [`ActionPicker`], which is shown/hidden
/// separately by the gesture-editing flow (it isn't reachable from the
/// top-level menu, only from within [`SettingsPage::Gestures`]).
fn show_settings_page(settings_panel: &SettingsPanel, page: SettingsPage) {
    settings_panel
        .menu_page
        .set_visible(page == SettingsPage::Menu);
    settings_panel
        .gestures_page
        .set_visible(page == SettingsPage::Gestures);
    settings_panel
        .display_page
        .set_visible(page == SettingsPage::Display);
    settings_panel
        .themes_page
        .set_visible(page == SettingsPage::Themes);
    settings_panel
        .equalizer_page
        .set_visible(page == SettingsPage::Equalizer);
    for stub in settings_panel.stub_pages.iter() {
        stub.root.set_visible(stub.page == page);
    }
}

/// A full-overlay opaque indicator shown for exactly as long as
/// `ArmedGestureDetector` reports the head unit armed (waiting for a
/// follow-up gesture) — added at Blake's explicit real-hardware feedback
/// (2026-08-16): without visible confirmation, an operator who completes
/// only the four-finger arming swipe has no way to know anything
/// registered before attempting the follow-up gesture.
#[derive(Clone)]
struct ArmedMask {
    root: GtkBox,
}

fn build_armed_mask() -> ArmedMask {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_halign(gtk4::Align::Fill);
    root.set_valign(gtk4::Align::Fill);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.set_visible(false);
    root.add_css_class("background");
    let label = Label::new(Some("Listening for gesture…"));
    label.set_halign(gtk4::Align::Center);
    label.set_valign(gtk4::Align::Center);
    label.set_vexpand(true);
    root.append(&label);
    ArmedMask { root }
}

/// Shown on the boot popup itself — deliberately short (`RISK_REGISTER.md`
/// R-021 calls for a "prominent" disclaimer, not a wall of text nobody
/// reads) and deliberately general, covering the head unit as a whole
/// rather than one feature, since Blake's own reasoning (2026-08-21) was
/// that a separate disclaimer per experimental feature (dashcam, `DeX`,
/// wireless AA, ...) would just stack up over time. The full detail
/// lives on [`SettingsPage::Legal`] instead — see
/// [`LEGAL_PAGE_DETAILED_TEXT`].
const EXPERIMENTAL_DISCLAIMER_SUMMARY_TEXT: &str = "This head unit and its experimental \
features (including dashcam/rear camera, the equalizer, DeX, and wireless Android Auto) are \
provided with no warranty and used at your own risk. See Settings → Legal for full details.";

/// Plain-language draft text, not reviewed by a lawyer — see this
/// feature's own design discussion (2026-08-21) for why: it deliberately
/// does not claim anything about court admissibility (software can't
/// dictate that), and instead names the two concrete things this project
/// genuinely cannot guarantee for dashcam/rear-camera footage specifically
/// — that recording is continuous or complete, and that any timestamp/GPS
/// data attached to a clip is accurate for that specific recording. Take
/// this to a lawyer before relying on it as real liability protection;
/// this is a starting point, not the real thing. Shown in full on
/// [`SettingsPage::Legal`], reachable any time — not just on the boot
/// popup — so an operator can review it whenever they want, not only in
/// the moment they're asked to agree to it.
const LEGAL_PAGE_DETAILED_TEXT: &str = "This head unit is independent, community software, \
not affiliated with or certified by any vehicle or phone manufacturer. It is provided with no \
warranty, under the terms of the GNU General Public License (see the project's own LICENSE \
file). Use while driving is at your own risk — this software has no safety-critical controls \
and should never be relied upon as one.\n\n\
Dashcam and rear-camera recording are experimental features. Recording may fail, be \
interrupted, or be incomplete. Timestamps and GPS/location data attached to a recording are \
not independently verified and may be inaccurate for that specific clip. Do not rely on this \
footage or its metadata as an accurate record of when or where something happened, including \
for insurance, legal, or law-enforcement purposes.\n\n\
Other experimental features on this head unit (including but not limited to the equalizer, \
Samsung DeX support, and wireless Android Auto) carry the same no-warranty, use-at-your-own-risk \
terms and may be incomplete or unreliable.";

/// The head-unit-wide experimental-software disclaimer, shown on every
/// boot (see `RISK_REGISTER.md` R-021, "prominent experimental
/// disclaimer") until the operator explicitly agrees — clicking "I
/// Agree" is the only way past it; there is no other way to close or
/// bypass this overlay, by design (2026-08-21, Blake's explicit
/// requirement: the operator must agree before being able to use the
/// head unit at all). Whether that agreement is remembered across future
/// boots is a separate, off-by-default choice
/// (`dont_show_again_check`) — defaulting to "ask again next boot" is
/// the more conservative choice absent further direction.
#[derive(Clone)]
struct ExperimentalDisclaimer {
    root: GtkBox,
    dont_show_again_check: CheckButton,
    agree_button: Button,
}

fn build_experimental_disclaimer() -> ExperimentalDisclaimer {
    let root = GtkBox::new(Orientation::Vertical, 16);
    root.set_halign(gtk4::Align::Fill);
    root.set_valign(gtk4::Align::Fill);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.set_visible(false);
    root.add_css_class("background");
    root.set_margin_top(24);
    root.set_margin_bottom(24);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let title = Label::new(Some("Experimental Software — Use At Your Own Risk"));
    title.set_halign(gtk4::Align::Center);
    root.append(&title);

    let body = Label::new(Some(EXPERIMENTAL_DISCLAIMER_SUMMARY_TEXT));
    body.set_wrap(true);
    body.set_justify(gtk4::Justification::Center);
    body.set_vexpand(true);
    body.set_valign(gtk4::Align::Center);
    root.append(&body);

    let dont_show_again_check = CheckButton::with_label("Don't show this again");
    dont_show_again_check.set_halign(gtk4::Align::Center);
    root.append(&dont_show_again_check);

    let agree_button = Button::with_label("I Agree");
    root.append(&agree_button);

    ExperimentalDisclaimer {
        root,
        dont_show_again_check,
        agree_button,
    }
}

/// Builds the disclaimer, adds it to `overlay`, sets its initial
/// visibility from the persisted setting, and wires the "I Agree"
/// button — split out of `activate_window` purely to keep it under
/// `clippy::too_many_lines`. Nothing else in this function offers a way
/// to hide the overlay — see [`ExperimentalDisclaimer`]'s doc comment
/// for why that's deliberate.
fn build_and_wire_experimental_disclaimer(
    overlay: &Overlay,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    let disclaimer = build_experimental_disclaimer();
    disclaimer.root.set_visible(
        !gesture_settings
            .borrow()
            .experimental_disclaimer_dismissed(),
    );
    overlay.add_overlay(&disclaimer.root);

    let gesture_settings_for_disclaimer = Rc::clone(gesture_settings);
    let disclaimer_root_for_agree = disclaimer.root.clone();
    let dont_show_again_check_for_agree = disclaimer.dont_show_again_check.clone();
    disclaimer.agree_button.connect_clicked(move |_| {
        if dont_show_again_check_for_agree.is_active() {
            gesture_settings_for_disclaimer
                .borrow_mut()
                .set_experimental_disclaimer_dismissed(true);
            let _ = gesture_settings_for_disclaimer
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
        }
        disclaimer_root_for_agree.set_visible(false);
    });
}

/// How far a press has to move before it's treated as a scroll drag
/// rather than a tap — small enough that a genuine tap never triggers it,
/// large enough that an ordinary drag reliably does.
const SCROLL_DRAG_CLAIM_THRESHOLD_PIXELS: f64 = 10.0;
/// Total straight-line distance from the press point a drag has to cover
/// before its direction (vertical vs. horizontal) is judged at all — see
/// `enable_touch_drag_scroll`'s doc comment for the real-hardware finding
/// this exists for: `GestureDrag`'s offsets are cumulative from the
/// press point, so very early in *any* drag both axes are still small
/// and noisy (an imprecise touch-down, a slightly diagonal first
/// movement), and judging direction from that noise alone produced false
/// claims on genuine horizontal slider drags. Comfortably larger than
/// [`SCROLL_DRAG_CLAIM_THRESHOLD_PIXELS`] so the direction check only
/// ever runs once a drag has clearly committed to some direction.
const SCROLL_DRAG_MIN_TOTAL_PIXELS: f64 = 24.0;

/// Manually drives `scroller`'s vertical scroll position from an ordinary
/// press-drag-release sequence, instead of relying on `ScrolledWindow`'s
/// own built-in kinetic touch scrolling. Real-hardware feedback,
/// 2026-08-16: on this project's touchscreen/compositor combination,
/// dragging content with a finger did not scroll it at all — only
/// grabbing and dragging the scrollbar's own thumb did, which the
/// operator explicitly rejected as unusable ("it MUST be a touch scroll
/// inside"). That symptom pattern (ordinary clicks fine, kinetic
/// drag-to-scroll never recognized) matches a known class of
/// embedded-compositor issue where touch arrives to GTK as emulated
/// pointer clicks rather than genuine touch-sequence events, which
/// `GtkScrolledWindow`'s kinetic scrolling specifically requires. A plain
/// [`gtk4::GestureDrag`] tracks any press-drag-release sequence regardless
/// of whether it originated as real touch or emulated pointer input, so
/// driving the adjustment from it directly works either way.
///
/// An earlier attempt at exactly this technique, applied to a small
/// popover holding a `ListBox`, misfired and silently mis-selected rows
/// while the operator tried to scroll (see [`ActionPicker`]'s doc
/// comment) — that popover/`ListBox` design is gone now, replaced by
/// this plain full-page-of-`Button`s layout, so the only competing
/// gesture here is an ordinary `Button` click, not a list row's
/// activate-and-close-popover behavior. Only claims the gesture (blocking
/// the drag from also completing as a click) once movement exceeds
/// [`SCROLL_DRAG_CLAIM_THRESHOLD_PIXELS`] — below that, an ordinary tap
/// still reaches its target normally.
///
/// Claims only when vertical movement clearly *dominates* horizontal,
/// never on absolute vertical distance alone — real-hardware finding
/// (2026-08-18), diagnosed with temporary per-gesture logging: this
/// gesture runs in `Capture` phase, meaning it sees every drag on any
/// descendant *before* that descendant's own gesture handling does, and
/// claiming steals the rest of the touch sequence from whatever child
/// gesture was already handling it. The brightness slider
/// (`SettingsPanel::brightness_scale`, a horizontal `Scale`) has its own
/// internal drag handling for exactly this purpose. A first fix dropped
/// `offset_x` from the claim check entirely (this panel never scrolls
/// horizontally — `hscrollbar_policy(PolicyType::Never)`,
/// `build_settings_panel`) but that alone wasn't enough: the logging
/// showed every real slider drag *still* got claimed, at only ~11-13px
/// of accumulated vertical drift over 289-454px of horizontal travel —
/// ordinary finger wobble on a long horizontal drag, not an intentional
/// vertical scroll, but still enough to cross the flat
/// [`SCROLL_DRAG_CLAIM_THRESHOLD_PIXELS`] threshold on its own. A second
/// fix added exactly that — require `offset_y` to also exceed
/// `offset_x` — but the *same* logging showed it still wasn't enough:
/// `GestureDrag`'s offsets are cumulative from the press point, so very
/// early in *any* drag (an imprecise touch-down, a slightly diagonal
/// first movement) both axes are still small, and the ratio between two
/// small noisy numbers is itself noisy — real claims were logged at
/// offsets like `(7, -11)` and `(2, -20)`, comfortably vertical-dominant
/// in that instant despite the drag going on to travel hundreds of
/// pixels horizontally. [`SCROLL_DRAG_MIN_TOTAL_PIXELS`] gates the whole
/// direction check behind a minimum total straight-line distance, so it
/// only ever runs once a drag has clearly committed to a direction —
/// real-hardware-confirmed: repeated full-range brightness drags no
/// longer stuck at all, while a genuine vertical scroll (logged during
/// the same trial at offsets like `(-6, -48)`, clearly vertical from the
/// start) still claimed correctly.
fn enable_touch_drag_scroll(scroller: &ScrolledWindow) {
    let drag = gtk4::GestureDrag::new();
    drag.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let vadjustment = scroller.vadjustment();
    let start_value = Rc::new(Cell::new(0.0_f64));

    let vadjustment_for_begin = vadjustment.clone();
    let start_value_for_begin = Rc::clone(&start_value);
    drag.connect_drag_begin(move |_, _, _| {
        start_value_for_begin.set(vadjustment_for_begin.value());
    });

    let start_value_for_update = Rc::clone(&start_value);
    drag.connect_drag_update(move |gesture, offset_x, offset_y| {
        let total = offset_x.hypot(offset_y);
        if offset_y.abs() > SCROLL_DRAG_CLAIM_THRESHOLD_PIXELS
            && total > SCROLL_DRAG_MIN_TOTAL_PIXELS
            && offset_y.abs() > offset_x.abs()
        {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        }
        vadjustment.set_value(start_value_for_update.get() - offset_y);
    });

    scroller.add_controller(drag);
}

/// Builds the full-panel action picker — see [`ActionPicker`]'s doc
/// comment for the design and the two real-hardware failures it
/// replaced. Built once; which gesture it's editing is set fresh each
/// time it's opened (`editing_gesture`).
fn build_action_picker() -> ActionPicker {
    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_visible(false);

    let back_button = Button::with_label("Back");
    root.append(&back_button);

    let title = Label::new(None);
    root.append(&title);

    // A two-column grid rather than one tall column — real-hardware
    // feedback, 2026-08-16: once the panel filled the whole window, a
    // single column of seven big buttons still didn't fit without
    // scrolling, and the operator pointed out a full-screen page has
    // enough width to lay them out instead of needing to scroll at all.
    // A `FlowBox` (auto-wrapping based on its own per-child natural-width
    // heuristics) was tried first and, on this same touchscreen/compositor
    // combination, didn't actually wrap into two columns — real-hardware
    // feedback, 2026-08-16: still rendered as one column. `Grid`'s
    // explicit row/column placement (`row = index / 2, column = index %
    // 2`, computed directly from `Action::all()`'s fixed order) has no
    // such heuristic to misbehave.
    let action_grid = Grid::new();
    action_grid.set_row_spacing(8);
    action_grid.set_column_spacing(8);
    action_grid.set_column_homogeneous(true);
    action_grid.set_hexpand(true);
    root.append(&action_grid);

    let mut action_buttons = Vec::new();
    for (index, action) in Action::all().into_iter().enumerate() {
        let button = Button::with_label(action.label());
        button.set_hexpand(true);
        let index = i32::try_from(index).unwrap_or(0);
        action_grid.attach(&button, index % 2, index / 2, 1, 1);
        action_buttons.push((action, button));
    }

    ActionPicker {
        root,
        title,
        action_buttons: Rc::new(action_buttons),
        back_button,
        editing_gesture: Rc::new(Cell::new(None)),
    }
}

/// Builds the settings panel once, hidden — shown only when the settings
/// gesture's mapped action is [`Action::OpenSettings`]. A plain
/// semi-opaque panel filling the whole window over the video via
/// `Overlay`, not a separate window: this is a dev diagnostic, not final
/// product chrome.
///
/// Lists `PulseAudio`/`PipeWire`-`pulse` device names via `pactl list
/// short <kind>` (`kind` is `"sinks"` or `"sources"`) — the second
/// tab-separated column of each line is the device's stable name, the
/// same string `pulsesink`/`pulsesrc`'s `device` property expects
/// (`media_gstreamer::AudioPlaybackPipeline`/`MicrophoneCapturePipeline`).
/// `pactl` itself comes from `pulseaudio-utils`, *not* the `pipewire`/
/// `wireplumber` packages this project's actual audio path already
/// depends on — real-hardware-required (2026-08-18): the reference Pi 5
/// image has `wpctl`/`pw-cli` but not `pactl` installed by default, so
/// this returned nothing at first despite working `GStreamer` `PulseAudio`
/// playback (`libpulse0`, the client library `pulsesink`/`pulsesrc` use
/// directly, doesn't need the separate CLI tool). Added as an explicit
/// `Depends` (`packaging/debian/control`), matching this project's
/// existing convention of declaring exactly the external tools it
/// actually shells out to (`adduser` is already one). Chose adding the
/// dependency over parsing `wpctl status`/`wpctl inspect` instead: `pactl`
/// speaks the exact `PulseAudio` compatibility surface `pulsesink`'s
/// `device` property does by definition, where `wpctl`'s device
/// descriptions are a human-readable label, not the raw `node.name`
/// `GStreamer` needs — correct, not just convenient.
///
/// Empty on any failure (missing `pactl`, no reachable `PipeWire`
/// session) rather than erroring — populating the settings panel with an
/// empty device list (just "System default") is a reasonable degraded
/// state, not a reason to fail building the whole panel.
fn list_pulse_devices(kind: &str) -> Vec<String> {
    let Ok(output) = std::process::Command::new("pactl")
        .args(["list", "short", kind])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split('\t').nth(1))
        .map(str::to_string)
        .collect()
}

/// Builds a device-selection `DropDown`: "System default" (index 0, maps
/// to the persisted setting's `None`) followed by every device
/// `list_pulse_devices` found, pre-selecting whichever entry matches
/// `initial_device` (falling back to "System default" if it names a
/// device that's no longer present — e.g. unplugged since last saved).
fn build_device_dropdown(devices: &[String], initial_device: Option<&str>) -> DropDown {
    let mut labels: Vec<&str> = vec!["System default"];
    labels.extend(devices.iter().map(String::as_str));
    let dropdown = DropDown::from_strings(&labels);
    let selected = initial_device
        .and_then(|initial| devices.iter().position(|device| device == initial))
        .map_or(0, |index| index + 1);
    dropdown.set_selected(u32::try_from(selected).unwrap_or(0));
    dropdown
}

/// Reads whichever device name `dropdown` currently has selected back out
/// as a persistable setting value — `None` for "System default" (index
/// `0`) or an out-of-range/`GTK_INVALID_LIST_POSITION` selection,
/// `Some(name)` otherwise.
fn selected_device(dropdown: &DropDown, devices: &[String]) -> Option<String> {
    let selected = dropdown.selected();
    if selected == 0 || selected == gtk4::INVALID_LIST_POSITION {
        return None;
    }
    devices
        .get(usize::try_from(selected).ok()?.checked_sub(1)?)
        .cloned()
}

/// Builds the brightness slider and the two audio-device dropdowns and
/// appends them to the Display page — split out of `build_settings_panel`
/// purely to keep it under `clippy::too_many_lines`. None of the three
/// live-apply: brightness takes effect the next time the screen is
/// turned off/on (no live-brightness-while-lit handle exists the way
/// rotation/arm-timeout have `SharedRotation`/`SharedArmWindow` — building
/// one would mean a new shared channel threaded through
/// `Gtk4WindowHandoff` purely for this, not justified yet for a setting
/// that already takes effect on the very next natural screen-power
/// event), and a device change only takes effect the next time its
/// pipeline (re)starts (a fresh session, or a channel `Start`
/// reconfiguration), matching every other pipeline construction
/// parameter in this project (format, codec, ...), none of which are
/// hot-swappable mid-stream.
#[allow(clippy::too_many_arguments)]
fn build_media_settings_controls(
    display_page: &GtkBox,
    initial_display_brightness_percent: u8,
    initial_audio_output_device: Option<&str>,
    initial_microphone_input_device: Option<&str>,
    initial_night_mode_gpio_line: Option<u32>,
) -> (
    Scale,
    DropDown,
    Vec<String>,
    DropDown,
    Vec<String>,
    CheckButton,
    SpinButton,
) {
    let brightness_row = GtkBox::new(Orientation::Horizontal, 8);
    let brightness_label = Label::new(Some("Screen brightness (%)"));
    let brightness_scale = Scale::with_range(
        Orientation::Horizontal,
        f64::from(crate::settings::MIN_DISPLAY_BRIGHTNESS_PERCENT),
        f64::from(crate::settings::MAX_DISPLAY_BRIGHTNESS_PERCENT),
        1.0,
    );
    brightness_scale.set_hexpand(true);
    brightness_scale.set_value(f64::from(initial_display_brightness_percent));
    brightness_row.append(&brightness_label);
    brightness_row.append(&brightness_scale);
    display_page.append(&brightness_row);

    let audio_output_devices = list_pulse_devices("sinks");
    let audio_output_row = GtkBox::new(Orientation::Horizontal, 8);
    let audio_output_label = Label::new(Some("Audio output device"));
    let audio_output_dropdown =
        build_device_dropdown(&audio_output_devices, initial_audio_output_device);
    audio_output_dropdown.set_hexpand(true);
    audio_output_row.append(&audio_output_label);
    audio_output_row.append(&audio_output_dropdown);
    display_page.append(&audio_output_row);

    let microphone_input_devices = list_pulse_devices("sources");
    let microphone_input_row = GtkBox::new(Orientation::Horizontal, 8);
    let microphone_input_label = Label::new(Some("Microphone input device"));
    let microphone_input_dropdown =
        build_device_dropdown(&microphone_input_devices, initial_microphone_input_device);
    microphone_input_dropdown.set_hexpand(true);
    microphone_input_row.append(&microphone_input_label);
    microphone_input_row.append(&microphone_input_dropdown);
    display_page.append(&microphone_input_row);

    // Unchecked/0 (the default) means night mode is disabled — see
    // `HeadUnitSettings::night_mode_gpio_line`'s doc comment. The spin
    // button stays enabled regardless of the checkbox state so a value
    // can be picked before enabling, rather than forcing enable-then-set.
    let night_mode_row = GtkBox::new(Orientation::Horizontal, 8);
    let night_mode_gpio_enabled_check = CheckButton::with_label("Night mode via GPIO line");
    night_mode_gpio_enabled_check.set_active(initial_night_mode_gpio_line.is_some());
    let night_mode_gpio_spin = SpinButton::with_range(
        f64::from(crate::settings::MIN_NIGHT_MODE_GPIO_LINE),
        f64::from(crate::settings::MAX_NIGHT_MODE_GPIO_LINE),
        1.0,
    );
    night_mode_gpio_spin.set_value(f64::from(
        initial_night_mode_gpio_line.unwrap_or(crate::settings::MIN_NIGHT_MODE_GPIO_LINE),
    ));
    night_mode_row.append(&night_mode_gpio_enabled_check);
    night_mode_row.append(&night_mode_gpio_spin);
    display_page.append(&night_mode_row);

    (
        brightness_scale,
        audio_output_dropdown,
        audio_output_devices,
        microphone_input_dropdown,
        microphone_input_devices,
        night_mode_gpio_enabled_check,
        night_mode_gpio_spin,
    )
}

impl SettingsPage {
    /// The label shown both on the top-level menu's button for this page
    /// and as that page's own title.
    fn label(self) -> &'static str {
        match self {
            SettingsPage::Menu => "Settings",
            SettingsPage::Gestures => "Gestures",
            SettingsPage::Display => "Display",
            SettingsPage::Themes => "Themes",
            SettingsPage::Equalizer => "EQ",
            SettingsPage::RearCamera => "Rear Camera",
            SettingsPage::DashCam => "DashCam",
            SettingsPage::ScreenMirroring => "Screen Mirroring",
            SettingsPage::Legal => "Legal",
        }
    }
}

/// Builds one of the settings menu's not-yet-implemented sibling pages
/// (EQ, Rear Camera, `DashCam`, Screen Mirroring) — a title and a Back
/// button, nothing else yet; real controls land here as each feature is
/// actually built. `open_button` (what the top-level menu grid actually
/// holds) is built here too so a page's full navigation wiring lives in
/// one place.
fn build_stub_page(page: SettingsPage) -> StubPage {
    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_visible(false);

    let title = Label::new(Some(page.label()));
    root.append(&title);

    let coming_soon = Label::new(Some("Coming soon."));
    root.append(&coming_soon);

    let back_button = Button::with_label("Back");
    root.append(&back_button);

    let open_button = Button::with_label(page.label());
    open_button.set_hexpand(true);

    StubPage {
        page,
        root,
        open_button,
        back_button,
    }
}

/// The detailed, always-reachable counterpart to the boot-time
/// disclaimer popup — see [`LEGAL_PAGE_DETAILED_TEXT`] for what it says
/// and why. Reuses [`StubPage`]'s shape (it's the same
/// title/content/Back layout, just with real content instead of "Coming
/// soon.") rather than inventing a near-identical struct.
fn build_legal_page() -> StubPage {
    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_visible(false);

    let title = Label::new(Some(SettingsPage::Legal.label()));
    root.append(&title);

    let body = Label::new(Some(LEGAL_PAGE_DETAILED_TEXT));
    body.set_wrap(true);
    root.append(&body);

    let back_button = Button::with_label("Back");
    root.append(&back_button);

    let open_button = Button::with_label(SettingsPage::Legal.label());
    open_button.set_hexpand(true);

    StubPage {
        page: SettingsPage::Legal,
        root,
        open_button,
        back_button,
    }
}

/// Everything [`build_gestures_page`] builds — bundled into a struct
/// (rather than a long tuple) purely for readability at the call site.
struct GesturesPageBuild {
    page: GtkBox,
    open_button: Button,
    back_button: Button,
    selectors: Vec<(GestureId, GestureSelector)>,
}

/// Builds the Gestures page — split out of `build_settings_panel` purely
/// to keep it under `clippy::too_many_lines`.
fn build_gestures_page() -> GesturesPageBuild {
    let page = GtkBox::new(Orientation::Vertical, 8);
    page.set_visible(false);

    let mappings_title = Label::new(Some("Gesture assignments"));
    page.append(&mappings_title);

    // Two columns of gesture rows rather than one tall column — at the
    // operator's explicit request, 2026-08-16, so the page fits the
    // full-screen panel without needing to scroll at all.
    let gesture_grid = Grid::new();
    gesture_grid.set_row_spacing(8);
    gesture_grid.set_column_spacing(16);
    gesture_grid.set_column_homogeneous(true);
    gesture_grid.set_hexpand(true);
    page.append(&gesture_grid);

    let mut selectors = Vec::new();
    for (index, gesture) in GestureId::all().into_iter().enumerate() {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.set_hexpand(true);
        let label = Label::new(Some(crate::settings::gesture_label(gesture)));
        let change_button = Button::new();
        change_button.set_hexpand(true);
        row.append(&label);
        row.append(&change_button);
        let index = i32::try_from(index).unwrap_or(0);
        gesture_grid.attach(&row, index % 2, index / 2, 1, 1);
        selectors.push((gesture, GestureSelector { change_button }));
    }

    let back_button = Button::with_label("Back");
    page.append(&back_button);

    let open_button = Button::with_label(SettingsPage::Gestures.label());
    open_button.set_hexpand(true);

    GesturesPageBuild {
        page,
        open_button,
        back_button,
        selectors,
    }
}

/// Everything [`build_display_page`] builds — bundled into a struct
/// (rather than a long tuple) purely for readability at the call site.
struct DisplayPageBuild {
    page: GtkBox,
    open_button: Button,
    back_button: Button,
    rotation_label: Label,
    flip_screen_button: Button,
    arm_timeout_spin: SpinButton,
    mtp_suppression_check: CheckButton,
    launch_on_boot_check: CheckButton,
    brightness_scale: Scale,
    audio_output_dropdown: DropDown,
    audio_output_devices: Vec<String>,
    microphone_input_dropdown: DropDown,
    microphone_input_devices: Vec<String>,
    night_mode_gpio_enabled_check: CheckButton,
    night_mode_gpio_spin: SpinButton,
}

/// Builds the Display page — split out of `build_settings_panel` purely
/// to keep it under `clippy::too_many_lines`.
#[allow(clippy::too_many_arguments)]
fn build_display_page(
    initial_arm_window_seconds: u32,
    initial_mtp_popup_suppression_enabled: bool,
    initial_launch_on_boot: bool,
    initial_display_brightness_percent: u8,
    initial_audio_output_device: Option<&str>,
    initial_microphone_input_device: Option<&str>,
    initial_night_mode_gpio_line: Option<u32>,
) -> DisplayPageBuild {
    let page = GtkBox::new(Orientation::Vertical, 8);
    page.set_visible(false);

    let display_title = Label::new(Some(SettingsPage::Display.label()));
    page.append(&display_title);

    let rotation_label = Label::new(Some("Screen: normal"));
    page.append(&rotation_label);

    let flip_screen_button = Button::with_label("Flip screen");
    page.append(&flip_screen_button);

    let timeout_row = GtkBox::new(Orientation::Horizontal, 8);
    let timeout_label = Label::new(Some("Gesture timeout (seconds)"));
    let arm_timeout_spin = SpinButton::with_range(
        f64::from(crate::settings::MIN_ARM_WINDOW_SECONDS),
        f64::from(crate::settings::MAX_ARM_WINDOW_SECONDS),
        1.0,
    );
    arm_timeout_spin.set_value(f64::from(initial_arm_window_seconds));
    timeout_row.append(&timeout_label);
    timeout_row.append(&arm_timeout_spin);
    page.append(&timeout_row);

    // Off by default — see `mtp_popup_suppression_enabled`'s doc comment
    // for the real-hardware finding and the file-browsing trade-off.
    let mtp_suppression_check =
        CheckButton::with_label("Suppress phone file-browser popups on reconnect");
    mtp_suppression_check.set_active(initial_mtp_popup_suppression_enabled);
    page.append(&mtp_suppression_check);

    // On by default (`HeadUnitSettings::launch_on_boot`'s doc comment) —
    // an operator turns this off to get a plain desktop on boot instead
    // of the fullscreen app, e.g. while testing. The desktop shortcut
    // (`packaging/labwc/aa-headunit.desktop`) still launches it manually
    // either way.
    let launch_on_boot_check = CheckButton::with_label("Launch on boot");
    launch_on_boot_check.set_active(initial_launch_on_boot);
    page.append(&launch_on_boot_check);

    let (
        brightness_scale,
        audio_output_dropdown,
        audio_output_devices,
        microphone_input_dropdown,
        microphone_input_devices,
        night_mode_gpio_enabled_check,
        night_mode_gpio_spin,
    ) = build_media_settings_controls(
        &page,
        initial_display_brightness_percent,
        initial_audio_output_device,
        initial_microphone_input_device,
        initial_night_mode_gpio_line,
    );

    let back_button = Button::with_label("Back");
    page.append(&back_button);

    let open_button = Button::with_label(SettingsPage::Display.label());
    open_button.set_hexpand(true);

    DisplayPageBuild {
        page,
        open_button,
        back_button,
        rotation_label,
        flip_screen_button,
        arm_timeout_spin,
        mtp_suppression_check,
        launch_on_boot_check,
        brightness_scale,
        audio_output_dropdown,
        audio_output_devices,
        microphone_input_dropdown,
        microphone_input_devices,
        night_mode_gpio_enabled_check,
        night_mode_gpio_spin,
    }
}

/// Everything [`build_equalizer_page`] builds.
struct EqualizerPageBuild {
    page: GtkBox,
    open_button: Button,
    back_button: Button,
    /// `band0..band9` order, matching `equalizer-10bands`' own property
    /// naming and `crate::settings::HeadUnitSettings::eq_bands`'s array
    /// order — see that method's doc comment.
    band_scales: Vec<Scale>,
}

/// Builds the Equalizer page: one vertical `Scale` per
/// `equalizer-10bands` band (extras roadmap, 2026-08-21 — see
/// `crates/media-gstreamer/src/audio.rs`'s matching pipeline change).
/// Labeled generically ("Band 1".."Band 10", lowest to highest
/// frequency) rather than asserting specific center-frequency numbers
/// this project hasn't confirmed against a pinned `GStreamer` source —
/// same evidentiary standard this project applies to the AAP protocol
/// itself, just applied to a dependency's own documented behavior
/// instead. Settings persist immediately (debounced, matching the
/// brightness slider) but do not live-apply to an already-running
/// session — see `HeadUnitSettings::eq_bands`'s doc comment for why
/// that's a deliberate, already-established precedent for audio
/// settings, not a shortcut taken here.
fn build_equalizer_page(
    initial_eq_bands: [f64; crate::settings::EQ_BAND_COUNT],
) -> EqualizerPageBuild {
    let page = GtkBox::new(Orientation::Vertical, 8);
    page.set_visible(false);

    let title = Label::new(Some(SettingsPage::Equalizer.label()));
    page.append(&title);

    let band_grid = Grid::new();
    band_grid.set_row_spacing(8);
    band_grid.set_column_spacing(8);
    band_grid.set_hexpand(true);
    page.append(&band_grid);

    let mut band_scales = Vec::with_capacity(crate::settings::EQ_BAND_COUNT);
    for (index, initial_gain_db) in initial_eq_bands.into_iter().enumerate() {
        let column = i32::try_from(index).unwrap_or(0);
        let label = Label::new(Some(&format!("Band {}", index + 1)));
        band_grid.attach(&label, column, 0, 1, 1);

        let scale = Scale::with_range(
            Orientation::Vertical,
            crate::settings::MIN_EQ_BAND_GAIN_DB,
            crate::settings::MAX_EQ_BAND_GAIN_DB,
            1.0,
        );
        scale.set_value(initial_gain_db);
        scale.set_vexpand(true);
        scale.set_inverted(true);
        band_grid.attach(&scale, column, 1, 1, 1);
        band_scales.push(scale);
    }

    let back_button = Button::with_label("Back");
    page.append(&back_button);

    let open_button = Button::with_label(SettingsPage::Equalizer.label());
    open_button.set_hexpand(true);

    EqualizerPageBuild {
        page,
        open_button,
        back_button,
        band_scales,
    }
}

/// Everything [`build_themes_page`] builds — bundled into a struct for
/// readability at the call site. `theme_buttons` pairs each button with
/// the persistable value tapping it should apply: `None` for "System
/// default", `Some(name)` for a discovered theme file.
struct ThemesPageBuild {
    page: GtkBox,
    open_button: Button,
    back_button: Button,
    theme_buttons: Vec<(Option<String>, Button)>,
}

/// Builds the Themes page: "System default" plus one button per
/// `.css` file [`list_theme_names`] finds in [`THEMES_DIR`] — split out
/// of `build_settings_panel` purely to keep it under
/// `clippy::too_many_lines`.
fn build_themes_page() -> ThemesPageBuild {
    let page = GtkBox::new(Orientation::Vertical, 8);
    page.set_visible(false);

    let title = Label::new(Some(SettingsPage::Themes.label()));
    page.append(&title);

    let theme_grid = Grid::new();
    theme_grid.set_row_spacing(8);
    theme_grid.set_column_spacing(8);
    theme_grid.set_column_homogeneous(true);
    theme_grid.set_hexpand(true);
    page.append(&theme_grid);

    let mut theme_buttons: Vec<(Option<String>, Button)> =
        vec![(None, Button::with_label("System default"))];
    for name in list_theme_names() {
        theme_buttons.push((Some(name.clone()), Button::with_label(&name)));
    }
    for (index, (_, button)) in theme_buttons.iter().enumerate() {
        button.set_hexpand(true);
        let index = i32::try_from(index).unwrap_or(0);
        theme_grid.attach(button, index % 2, index / 2, 1, 1);
    }

    let back_button = Button::with_label("Back");
    page.append(&back_button);

    let open_button = Button::with_label(SettingsPage::Themes.label());
    open_button.set_hexpand(true);

    ThemesPageBuild {
        page,
        open_button,
        back_button,
        theme_buttons,
    }
}

/// Reads every setting `build_settings_panel` needs out of `gesture_settings`
/// and calls it — split out of `activate_window` purely to keep it under
/// `clippy::too_many_lines`, not because this borrowing sequence does
/// anything `activate_window` itself couldn't.
fn build_settings_panel_from(gesture_settings: &Rc<RefCell<HeadUnitSettings>>) -> SettingsPanel {
    build_settings_panel(
        gesture_settings.borrow().arm_window_seconds(),
        gesture_settings.borrow().mtp_popup_suppression_enabled(),
        gesture_settings.borrow().launch_on_boot(),
        gesture_settings.borrow().display_brightness_percent(),
        gesture_settings.borrow().audio_output_device(),
        gesture_settings.borrow().microphone_input_device(),
        gesture_settings.borrow().night_mode_gpio_line(),
        gesture_settings.borrow().eq_bands(),
    )
}

/// Builds the top-level settings menu page: a title, a two-column grid of
/// `buttons` (one per destination page — Android Auto, Gestures, Display,
/// etc.), and the "Return to desktop" fullscreen toggle below it. Split
/// out of `build_settings_panel` purely to keep it under
/// `clippy::too_many_lines`. At the operator's explicit request
/// (2026-08-19): the panel opened by the arm-swipe-then-gesture is a page
/// full of buttons, each leading to its own page, rather than one long
/// flat page of every control.
fn build_menu_page(buttons: &[&Button]) -> (GtkBox, Button) {
    let menu_page = GtkBox::new(Orientation::Vertical, 8);

    let title = Label::new(Some("Head unit settings"));
    menu_page.append(&title);

    // A two-column grid, like the gesture/action grids elsewhere in this
    // panel — real-hardware feedback (2026-08-16) already ruled out
    // `FlowBox` for this touchscreen/compositor combination (it didn't
    // actually wrap into two columns), so `Grid`'s explicit row/column
    // placement is used here too.
    let menu_grid = Grid::new();
    menu_grid.set_row_spacing(8);
    menu_grid.set_column_spacing(8);
    menu_grid.set_column_homogeneous(true);
    menu_grid.set_hexpand(true);
    menu_page.append(&menu_grid);

    for (index, button) in buttons.iter().enumerate() {
        let index = i32::try_from(index).unwrap_or(0);
        menu_grid.attach(*button, index % 2, index / 2, 1, 1);
    }

    let toggle_fullscreen_button = Button::with_label("Return to desktop");
    menu_page.append(&toggle_fullscreen_button);

    (menu_page, toggle_fullscreen_button)
}

#[allow(clippy::too_many_arguments)]
fn build_settings_panel(
    initial_arm_window_seconds: u32,
    initial_mtp_popup_suppression_enabled: bool,
    initial_launch_on_boot: bool,
    initial_display_brightness_percent: u8,
    initial_audio_output_device: Option<&str>,
    initial_microphone_input_device: Option<&str>,
    initial_night_mode_gpio_line: Option<u32>,
    initial_eq_bands: [f64; crate::settings::EQ_BAND_COUNT],
) -> SettingsPanel {
    let gestures = build_gestures_page();
    let display = build_display_page(
        initial_arm_window_seconds,
        initial_mtp_popup_suppression_enabled,
        initial_launch_on_boot,
        initial_display_brightness_percent,
        initial_audio_output_device,
        initial_microphone_input_device,
        initial_night_mode_gpio_line,
    );

    let themes = build_themes_page();
    let equalizer = build_equalizer_page(initial_eq_bands);

    // --- The three still-not-yet-implemented sibling pages ---
    let mut stub_pages: Vec<StubPage> = [
        SettingsPage::RearCamera,
        SettingsPage::DashCam,
        SettingsPage::ScreenMirroring,
    ]
    .into_iter()
    .map(build_stub_page)
    .collect();
    // Reuses the exact same `StubPage` shape (title/content/Back) as the
    // stubs above — real content instead of "Coming soon.", but no
    // dedicated `SettingsPanel` fields needed since the existing stub
    // visibility/open/back-button wiring already handles it generically.
    stub_pages.push(build_legal_page());

    // "Android Auto" leads the grid — the live AA view is the primary
    // destination, the rest are settings pages, matching the OpenAuto-style
    // "AA is one of the buttons" menu the operator asked for.
    let android_auto_button = Button::with_label("Android Auto");
    let mut menu_buttons: Vec<&Button> = vec![
        &android_auto_button,
        &gestures.open_button,
        &display.open_button,
        &themes.open_button,
        &equalizer.open_button,
    ];
    menu_buttons.extend(stub_pages.iter().map(|stub| &stub.open_button));
    let (menu_page, toggle_fullscreen_button) = build_menu_page(&menu_buttons);

    let picker = build_action_picker();

    let content = GtkBox::new(Orientation::Vertical, 8);
    content.append(&menu_page);
    content.append(&gestures.page);
    content.append(&display.page);
    content.append(&themes.page);
    content.append(&equalizer.page);
    for stub in &stub_pages {
        content.append(&stub.root);
    }
    content.append(&picker.root);

    let root = ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(PolicyType::Never)
        .build();
    // Fills the whole window rather than floating as a small centered
    // box — real-hardware feedback, 2026-08-16: a fixed-size centered
    // panel left too little room for seven gesture rows on the 800x480
    // panel, and even filling the whole window, the picker page (back
    // button + title + seven action buttons) still doesn't fit without
    // scrolling. `enable_touch_drag_scroll` below is what actually makes
    // that scrolling usable — the plain scrollbar thumb was explicitly
    // rejected as an unusable fallback.
    enable_touch_drag_scroll(&root);
    root.set_halign(gtk4::Align::Fill);
    root.set_valign(gtk4::Align::Fill);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.set_visible(false);
    root.add_css_class("background");
    root.set_margin_top(24);
    root.set_margin_bottom(24);
    root.set_margin_start(24);
    root.set_margin_end(24);

    SettingsPanel {
        root,
        menu_page,
        gestures_page: gestures.page,
        gestures_button: gestures.open_button,
        gestures_back_button: gestures.back_button,
        display_page: display.page,
        display_button: display.open_button,
        display_back_button: display.back_button,
        themes_page: themes.page,
        themes_button: themes.open_button,
        themes_back_button: themes.back_button,
        theme_buttons: Rc::new(themes.theme_buttons),
        active_theme_provider: Rc::new(RefCell::new(None)),
        equalizer_page: equalizer.page,
        equalizer_button: equalizer.open_button,
        equalizer_back_button: equalizer.back_button,
        eq_scales: Rc::new(equalizer.band_scales),
        stub_pages: Rc::new(stub_pages),
        rotation_label: display.rotation_label,
        gesture_selectors: Rc::new(gestures.selectors),
        picker,
        android_auto_button,
        toggle_fullscreen_button,
        flip_screen_button: display.flip_screen_button,
        arm_timeout_spin: display.arm_timeout_spin,
        mtp_suppression_check: display.mtp_suppression_check,
        launch_on_boot_check: display.launch_on_boot_check,
        brightness_scale: display.brightness_scale,
        audio_output_dropdown: display.audio_output_dropdown,
        audio_output_devices: Rc::new(display.audio_output_devices),
        microphone_input_dropdown: display.microphone_input_dropdown,
        microphone_input_devices: Rc::new(display.microphone_input_devices),
        night_mode_gpio_enabled_check: display.night_mode_gpio_enabled_check,
        night_mode_gpio_spin: display.night_mode_gpio_spin,
    }
}

fn rotation_label_text(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::Normal => "Screen: normal",
        Rotation::Flipped180 => "Screen: flipped 180°",
    }
}

fn next_rotation(rotation: Rotation) -> Rotation {
    match rotation {
        Rotation::Normal => Rotation::Flipped180,
        Rotation::Flipped180 => Rotation::Normal,
    }
}

fn apply_rotation(
    rotation: Rotation,
    settings_panel: &SettingsPanel,
    armed_mask: &ArmedMask,
    rotation_handle: &Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: &Rc<Cell<Rotation>>,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    println!("probe_state=rotation_requested rotation={rotation:?}");
    current_rotation.set(rotation);
    settings_panel
        .rotation_label
        .set_text(rotation_label_text(rotation));
    // Both overlays are GTK widgets, never touched by the video
    // pipeline's own `set_rotation_degrees` — see
    // `install_flipped_panel_css`'s doc comment for the real-hardware
    // finding this fixes. The armed-state mask ("Listening for
    // gesture…") needs the same treatment as the settings panel — both
    // are overlaid directly on top of the (separately, correctly)
    // flipped video.
    if rotation == Rotation::Flipped180 {
        settings_panel.root.add_css_class("flipped-panel");
        armed_mask.root.add_css_class("flipped-panel");
    } else {
        settings_panel.root.remove_css_class("flipped-panel");
        armed_mask.root.remove_css_class("flipped-panel");
    }
    if let Some(handle) = rotation_handle.borrow().as_ref() {
        handle.set(rotation);
    }
    gesture_settings.borrow_mut().set_rotation(rotation);
    let _ = gesture_settings
        .borrow()
        .save(Path::new(DEFAULT_SETTINGS_PATH));
}

fn toggle_fullscreen_button_label(is_fullscreen: bool) -> &'static str {
    if is_fullscreen {
        "Return to desktop"
    } else {
        "Return to video"
    }
}

/// Hides the whole settings panel and resets it back to
/// [`SettingsPage::Menu`] — so reopening it later never resumes showing
/// a stale [`ActionPicker`] or sub-page left open from a previous visit.
fn close_settings_panel(settings_panel: &SettingsPanel) {
    settings_panel.root.set_visible(false);
    settings_panel.picker.root.set_visible(false);
    show_settings_page(settings_panel, SettingsPage::Menu);
}

/// Flips between fullscreen video and the plain desktop, always closing
/// the settings panel too. Bidirectional deliberately — see [`Action`]'s
/// doc comment for the real-hardware trial that found the one-directional
/// predecessor left the operator stuck with no way back.
fn toggle_fullscreen(
    window: &ApplicationWindow,
    settings_panel: &SettingsPanel,
    is_fullscreen: &Rc<Cell<bool>>,
) {
    let now_fullscreen = !is_fullscreen.get();
    if now_fullscreen {
        window.fullscreen();
    } else {
        window.unfullscreen();
    }
    is_fullscreen.set(now_fullscreen);
    settings_panel
        .toggle_fullscreen_button
        .set_label(toggle_fullscreen_button_label(now_fullscreen));
    close_settings_panel(settings_panel);
}

/// Connects every control's click/selection handler once, right after the
/// panel is built. Reassigning a gesture's action saves the whole
/// `HeadUnitSettings` immediately — small, infrequent writes, not worth
/// debouncing.
#[allow(clippy::too_many_arguments)]
/// Split out of `wire_settings_panel` purely to keep it under
/// `clippy::too_many_lines`.
fn wire_mtp_suppression_toggle(
    settings_panel: &SettingsPanel,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    let gesture_settings = Rc::clone(gesture_settings);
    settings_panel
        .mtp_suppression_check
        .connect_toggled(move |check| {
            let enabled = check.is_active();
            gesture_settings
                .borrow_mut()
                .set_mtp_popup_suppression_enabled(enabled);
            let _ = gesture_settings
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
            crate::mtp_suppression::sync(enabled);
        });
}

/// Mirrors [`wire_mtp_suppression_toggle`] for the "Launch on boot" check
/// button — see `HeadUnitSettings::launch_on_boot`'s doc comment for what
/// this actually gates.
fn wire_launch_on_boot_toggle(
    settings_panel: &SettingsPanel,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    let gesture_settings = Rc::clone(gesture_settings);
    settings_panel
        .launch_on_boot_check
        .connect_toggled(move |check| {
            gesture_settings
                .borrow_mut()
                .set_launch_on_boot(check.is_active());
            let _ = gesture_settings
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
        });
}

/// Persists the brightness slider and the two device dropdowns on every
/// change. The two device dropdowns still don't live-apply (see
/// `build_settings_panel`'s doc comment on why — a device change only
/// takes effect at the next pipeline start). Brightness *does* live-apply
/// now — real-hardware feedback (2026-08-18): "the slider does nothing"
/// is a bad experience for a brightness control specifically, unlike the
/// device dropdowns, which have no meaningful "preview" concept anyway.
/// Calls `auth_discovery_probe::set_screen_power` directly from this (GTK)
/// thread rather than round-tripping through the background protocol
/// thread via a new shared-handle channel (the `SharedRotation`/
/// `SharedArmWindow` pattern) — safe because the backlight sysfs write
/// itself has no shared mutable state to race on, and because the
/// settings panel can only be open while the screen is already on (the
/// `ScreenOff` gesture swallows all touch, including the arm-swipe that
/// would open this panel, while the screen is off), so there is no
/// "adjusting brightness while asleep" case to worry about.
///
/// The settings-file save is debounced ([`BRIGHTNESS_SAVE_DEBOUNCE`]),
/// not run on every tick — a second real-hardware finding (2026-08-18,
/// after fixing `set_screen_power`'s own backlight-lookup caching still
/// left the slider not smooth): a full `HeadUnitSettings::save` does a
/// TOML serialize plus an `fs::write` of the whole settings file, which
/// on every single value-changed event during an active drag was enough
/// disk I/O on this (GTK) thread to visibly stutter the drag.
///
/// A same-day attempt to also throttle the live backlight write itself
/// (skip writes closer together than ~16ms) made real-hardware dragging
/// *worse*, not better, and was reverted — the live write stays
/// unconditional, on every tick. Not fully explained; recorded so a
/// future session doesn't re-attempt the identical throttle without new
/// evidence for what it actually cost.
///
/// The residual "sticks partway through" symptom neither of the above
/// two fixes addressed turned out to have nothing to do with this
/// function at all — see `enable_touch_drag_scroll`'s doc comment for
/// the actual root cause (the settings panel's own scroll-to-scroll
/// gesture claiming the touch sequence out from under this slider's
/// drag), found via temporary per-gesture diagnostic logging rather than
/// further guessing here.
fn wire_brightness_and_device_settings(
    settings_panel: &SettingsPanel,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    let gesture_settings_for_brightness = Rc::clone(gesture_settings);
    let pending_brightness_save: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    settings_panel
        .brightness_scale
        .connect_value_changed(move |scale| {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let percent = scale.value() as u8;
            gesture_settings_for_brightness
                .borrow_mut()
                .set_display_brightness_percent(percent);
            crate::auth_discovery_probe::set_screen_power(true, percent);

            if let Some(source_id) = pending_brightness_save.take() {
                source_id.remove();
            }
            let gesture_settings_for_save = Rc::clone(&gesture_settings_for_brightness);
            let pending_brightness_save_for_timeout = Rc::clone(&pending_brightness_save);
            let source_id = glib::timeout_add_local(BRIGHTNESS_SAVE_DEBOUNCE, move || {
                let _ = gesture_settings_for_save
                    .borrow()
                    .save(Path::new(DEFAULT_SETTINGS_PATH));
                pending_brightness_save_for_timeout.set(None);
                glib::ControlFlow::Break
            });
            pending_brightness_save.set(Some(source_id));
        });

    let gesture_settings_for_audio = Rc::clone(gesture_settings);
    let audio_output_devices = Rc::clone(&settings_panel.audio_output_devices);
    let audio_output_dropdown = settings_panel.audio_output_dropdown.clone();
    settings_panel
        .audio_output_dropdown
        .connect_selected_notify(move |_| {
            let device = selected_device(&audio_output_dropdown, &audio_output_devices);
            gesture_settings_for_audio
                .borrow_mut()
                .set_audio_output_device(device);
            let _ = gesture_settings_for_audio
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
        });

    let gesture_settings_for_mic = Rc::clone(gesture_settings);
    let microphone_input_devices = Rc::clone(&settings_panel.microphone_input_devices);
    let microphone_input_dropdown = settings_panel.microphone_input_dropdown.clone();
    settings_panel
        .microphone_input_dropdown
        .connect_selected_notify(move |_| {
            let device = selected_device(&microphone_input_dropdown, &microphone_input_devices);
            gesture_settings_for_mic
                .borrow_mut()
                .set_microphone_input_device(device);
            let _ = gesture_settings_for_mic
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
        });

    // Also persist-only, like the two device dropdowns above — the GPIO
    // line is only ever opened once, at probe start
    // (`open_night_mode_gpio_source`), so there is nothing to live-apply
    // here either. The spin button stays readable/settable regardless of
    // the checkbox, so both handlers below read the checkbox for whether
    // to persist `Some`/`None` and the spin for which line.
    let gesture_settings_for_night_mode_check = Rc::clone(gesture_settings);
    let night_mode_gpio_spin_for_check = settings_panel.night_mode_gpio_spin.clone();
    settings_panel
        .night_mode_gpio_enabled_check
        .connect_toggled(move |check| {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let line = check
                .is_active()
                .then(|| night_mode_gpio_spin_for_check.value() as u32);
            gesture_settings_for_night_mode_check
                .borrow_mut()
                .set_night_mode_gpio_line(line);
            let _ = gesture_settings_for_night_mode_check
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
        });

    let gesture_settings_for_night_mode_spin = Rc::clone(gesture_settings);
    let night_mode_gpio_enabled_check_for_spin =
        settings_panel.night_mode_gpio_enabled_check.clone();
    settings_panel
        .night_mode_gpio_spin
        .connect_value_changed(move |spin| {
            if !night_mode_gpio_enabled_check_for_spin.is_active() {
                return;
            }
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let value = spin.value() as u32;
            gesture_settings_for_night_mode_spin
                .borrow_mut()
                .set_night_mode_gpio_line(Some(value));
            let _ = gesture_settings_for_night_mode_spin
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
        });
}

/// Wires the 10 equalizer band sliders — persist-only, like the audio
/// device dropdowns above, not the live-applying brightness slider
/// (`HeadUnitSettings::eq_bands`'s doc comment explains why: EQ settings
/// are picked up at the next audio pipeline build, not mid-session).
/// Debounced ([`BRIGHTNESS_SAVE_DEBOUNCE`]) with one shared timer for all
/// 10 sliders, not one per band — every slider's handler saves the
/// *whole* current settings snapshot regardless of which band moved, so
/// a single shared debounce is already correct: rapid drags across
/// multiple sliders collapse into one save, same as rapid drags on one.
fn wire_equalizer_sliders(
    settings_panel: &SettingsPanel,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    let pending_eq_save: Rc<Cell<Option<glib::SourceId>>> = Rc::new(Cell::new(None));
    for (index, scale) in settings_panel.eq_scales.iter().enumerate() {
        let gesture_settings = Rc::clone(gesture_settings);
        let pending_eq_save = Rc::clone(&pending_eq_save);
        scale.connect_value_changed(move |scale| {
            gesture_settings
                .borrow_mut()
                .set_eq_band(index, scale.value());

            if let Some(source_id) = pending_eq_save.take() {
                source_id.remove();
            }
            let gesture_settings_for_save = Rc::clone(&gesture_settings);
            let pending_eq_save_for_timeout = Rc::clone(&pending_eq_save);
            let source_id = glib::timeout_add_local(BRIGHTNESS_SAVE_DEBOUNCE, move || {
                let _ = gesture_settings_for_save
                    .borrow()
                    .save(Path::new(DEFAULT_SETTINGS_PATH));
                pending_eq_save_for_timeout.set(None);
                glib::ControlFlow::Break
            });
            pending_eq_save.set(Some(source_id));
        });
    }
}

/// Wires the gesture-assignment flow: tapping a gesture's selector opens
/// [`ActionPicker`] over the Gestures page, tapping an action applies it
/// and returns, and the picker's own Back button returns unchanged —
/// split out of `wire_settings_panel` purely to keep it under
/// `clippy::too_many_lines`.
fn wire_gesture_editing(
    settings_panel: &SettingsPanel,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    for (gesture, selector) in settings_panel.gesture_selectors.iter() {
        let initial = gesture_settings.borrow().action_for(*gesture);
        selector.change_button.set_label(initial.label());
        let gesture = *gesture;
        let gestures_page = settings_panel.gestures_page.clone();
        let picker = settings_panel.picker.clone();
        selector.change_button.connect_clicked(move |_| {
            picker.editing_gesture.set(Some(gesture));
            picker.title.set_text(&format!(
                "{}: choose an action",
                crate::settings::gesture_label(gesture)
            ));
            gestures_page.set_visible(false);
            picker.root.set_visible(true);
        });
    }

    let gestures_page_for_back = settings_panel.gestures_page.clone();
    let picker_for_back = settings_panel.picker.clone();
    settings_panel.picker.back_button.connect_clicked(move |_| {
        picker_for_back.root.set_visible(false);
        gestures_page_for_back.set_visible(true);
    });

    for (action, action_button) in settings_panel.picker.action_buttons.iter() {
        let action = *action;
        let picker = settings_panel.picker.clone();
        let gestures_page = settings_panel.gestures_page.clone();
        let gesture_settings = Rc::clone(gesture_settings);
        let gesture_selectors = Rc::clone(&settings_panel.gesture_selectors);
        action_button.connect_clicked(move |_| {
            let Some(gesture) = picker.editing_gesture.get() else {
                return;
            };
            gesture_settings.borrow_mut().set_action(gesture, action);
            let _ = gesture_settings
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
            for (candidate_gesture, selector) in gesture_selectors.iter() {
                if *candidate_gesture == gesture {
                    selector.change_button.set_label(action.label());
                }
            }
            picker.root.set_visible(false);
            gestures_page.set_visible(true);
        });
    }
}

/// Wires the top-level settings menu's navigation: each of Gestures/
/// Display/Themes and the four placeholder pages opens on its own
/// button and returns to the menu on its own Back button — split out of
/// `wire_settings_panel` purely to keep it under `clippy::too_many_lines`.
fn wire_settings_navigation(settings_panel: &SettingsPanel) {
    let settings_panel_for_gestures_open = settings_panel.clone();
    settings_panel.gestures_button.connect_clicked(move |_| {
        show_settings_page(&settings_panel_for_gestures_open, SettingsPage::Gestures);
    });
    let settings_panel_for_gestures_back = settings_panel.clone();
    settings_panel
        .gestures_back_button
        .connect_clicked(move |_| {
            show_settings_page(&settings_panel_for_gestures_back, SettingsPage::Menu);
        });

    let settings_panel_for_display_open = settings_panel.clone();
    settings_panel.display_button.connect_clicked(move |_| {
        show_settings_page(&settings_panel_for_display_open, SettingsPage::Display);
    });
    let settings_panel_for_display_back = settings_panel.clone();
    settings_panel
        .display_back_button
        .connect_clicked(move |_| {
            show_settings_page(&settings_panel_for_display_back, SettingsPage::Menu);
        });

    let settings_panel_for_themes_open = settings_panel.clone();
    settings_panel.themes_button.connect_clicked(move |_| {
        show_settings_page(&settings_panel_for_themes_open, SettingsPage::Themes);
    });
    let settings_panel_for_themes_back = settings_panel.clone();
    settings_panel.themes_back_button.connect_clicked(move |_| {
        show_settings_page(&settings_panel_for_themes_back, SettingsPage::Menu);
    });

    let settings_panel_for_equalizer_open = settings_panel.clone();
    settings_panel.equalizer_button.connect_clicked(move |_| {
        show_settings_page(&settings_panel_for_equalizer_open, SettingsPage::Equalizer);
    });
    let settings_panel_for_equalizer_back = settings_panel.clone();
    settings_panel
        .equalizer_back_button
        .connect_clicked(move |_| {
            show_settings_page(&settings_panel_for_equalizer_back, SettingsPage::Menu);
        });

    for stub in settings_panel.stub_pages.iter() {
        let page = stub.page;
        let settings_panel_for_open = settings_panel.clone();
        stub.open_button.connect_clicked(move |_| {
            show_settings_page(&settings_panel_for_open, page);
        });
        let settings_panel_for_back = settings_panel.clone();
        stub.back_button.connect_clicked(move |_| {
            show_settings_page(&settings_panel_for_back, SettingsPage::Menu);
        });
    }
}

/// Wires the Themes page: tapping a theme button applies it immediately
/// (like the brightness slider, not the audio/mic dropdowns — a theme
/// picker with no visible preview would be a bad experience) and
/// persists the choice — split out of `wire_settings_panel` purely to
/// keep it under `clippy::too_many_lines`.
fn wire_theme_selection(
    settings_panel: &SettingsPanel,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    for (name, button) in settings_panel.theme_buttons.iter() {
        let name = name.clone();
        let gesture_settings = Rc::clone(gesture_settings);
        let active_theme_provider = Rc::clone(&settings_panel.active_theme_provider);
        button.connect_clicked(move |_| {
            println!(
                "probe_state=theme_selected theme={}",
                name.as_deref().unwrap_or("system_default")
            );
            apply_theme(&active_theme_provider, name.as_deref());
            gesture_settings.borrow_mut().set_theme(name.clone());
            let _ = gesture_settings
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn wire_settings_panel(
    settings_panel: &SettingsPanel,
    armed_mask: &ArmedMask,
    window: &ApplicationWindow,
    rotation_handle: &Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: &Rc<Cell<Rotation>>,
    arm_window_handle: &Rc<RefCell<Option<SharedArmWindow>>>,
    is_fullscreen: &Rc<Cell<bool>>,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    wire_gesture_editing(settings_panel, gesture_settings);
    wire_settings_navigation(settings_panel);
    wire_theme_selection(settings_panel, gesture_settings);

    let arm_window_handle_for_spin = Rc::clone(arm_window_handle);
    let gesture_settings_for_spin = Rc::clone(gesture_settings);
    settings_panel
        .arm_timeout_spin
        .connect_value_changed(move |spin| {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let seconds = spin.value() as u32;
            gesture_settings_for_spin
                .borrow_mut()
                .set_arm_window_seconds(seconds);
            let _ = gesture_settings_for_spin
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
            if let Some(handle) = arm_window_handle_for_spin.borrow().as_ref() {
                handle.set_micros(u64::from(seconds) * 1_000_000);
            }
        });

    wire_mtp_suppression_toggle(settings_panel, gesture_settings);
    wire_launch_on_boot_toggle(settings_panel, gesture_settings);
    wire_brightness_and_device_settings(settings_panel, gesture_settings);
    wire_equalizer_sliders(settings_panel, gesture_settings);

    let settings_panel_for_cycle = settings_panel.clone();
    let armed_mask_for_cycle = armed_mask.clone();
    let rotation_handle_for_cycle = Rc::clone(rotation_handle);
    let current_rotation_for_cycle = Rc::clone(current_rotation);
    let gesture_settings_for_cycle = Rc::clone(gesture_settings);
    settings_panel.flip_screen_button.connect_clicked(move |_| {
        let next = next_rotation(current_rotation_for_cycle.get());
        apply_rotation(
            next,
            &settings_panel_for_cycle,
            &armed_mask_for_cycle,
            &rotation_handle_for_cycle,
            &current_rotation_for_cycle,
            &gesture_settings_for_cycle,
        );
    });

    let settings_panel_for_close = settings_panel.clone();
    settings_panel
        .android_auto_button
        .connect_clicked(move |_| {
            close_settings_panel(&settings_panel_for_close);
        });

    let window_for_toggle = window.clone();
    let settings_panel_for_toggle = settings_panel.clone();
    let is_fullscreen_for_toggle = Rc::clone(is_fullscreen);
    settings_panel
        .toggle_fullscreen_button
        .connect_clicked(move |_| {
            toggle_fullscreen(
                &window_for_toggle,
                &settings_panel_for_toggle,
                &is_fullscreen_for_toggle,
            );
        });
}

/// Executes whichever [`Action`] the settings gesture that just fired is
/// currently mapped to. `ToggleFullscreen` never ends the session — the
/// background protocol thread, its channels, and every media pipeline
/// keep running exactly as before regardless of window state, per the
/// operator's explicit choice that a settings gesture should minimize
/// (bidirectionally), not tear down, a live session.
#[allow(clippy::too_many_arguments)]
fn dispatch_action(
    action: Action,
    settings_panel: &SettingsPanel,
    armed_mask: &ArmedMask,
    window: &ApplicationWindow,
    rotation_handle: &Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: &Rc<Cell<Rotation>>,
    is_fullscreen: &Rc<Cell<bool>>,
    gesture_settings: &Rc<RefCell<HeadUnitSettings>>,
) {
    match action {
        Action::OpenSettings => settings_panel.root.set_visible(true),
        Action::ToggleFullscreen => toggle_fullscreen(window, settings_panel, is_fullscreen),
        Action::FlipScreen => {
            let next = next_rotation(current_rotation.get());
            apply_rotation(
                next,
                settings_panel,
                armed_mask,
                rotation_handle,
                current_rotation,
                gesture_settings,
            );
        }
        // All no-ops here, dispatched instead from the background protocol
        // thread (`auth_discovery_probe.rs::service_touch_input`), for two
        // different reasons: `SwitchTo*` sends a key event to the phone,
        // so it needs that thread's `transport` access; `ScreenOff`
        // touches neither the phone nor GTK window state, but must still
        // run there since that thread's `service_touch_input` is the only
        // place that can swallow the touch used to wake the screen back
        // up — see `settings::Action::ScreenOff`'s doc comment and
        // `auth_discovery_probe.rs`'s `ScreenPowerState`.
        Action::SwitchToMedia
        | Action::SwitchToNavigation
        | Action::SwitchToRadio
        | Action::SwitchToPhone
        | Action::ScreenOff => {}
    }
}

/// Builds a `RenderSink::Gtk4Paintable` pipeline and wires it into
/// `picture` — the exact build → retrieve-paintable → set-on-`Picture` →
/// start ordering the GTK4 spike proved correct
/// (`crates/media-gstreamer/examples/gtk_fullscreen_spike.rs`), just
/// called from a poll callback instead of directly inside
/// `connect_activate`. Must run on the GTK/main thread — see
/// `VideoRenderTarget::Gtk4Window`'s doc comment.
fn build_gtk4_pipeline(
    capability: &DecoderCapability,
    picture: &Picture,
) -> Result<VideoRenderPipeline, GstreamerError> {
    let backend = GstreamerBackend::new()?;
    let pipeline = backend.build_video_render_pipeline(capability, RenderSink::Gtk4Paintable)?;
    let paintable = pipeline
        .gtk4_paintable_property()
        .ok_or_else(|| {
            GstreamerError::PipelineConstruction(
                "gtk4paintablesink element missing after pipeline construction".into(),
            )
        })?
        .get::<gtk4::gdk::Paintable>()
        .map_err(|_| {
            GstreamerError::PipelineConstruction(
                "\"paintable\" property was not a GdkPaintable".into(),
            )
        })?;
    picture.set_paintable(Some(&paintable));
    // See `VideoRenderPipeline::set_window_size`'s doc comment — keeps
    // the sink informed of the actual rendering surface size, last set
    // (staler than ideal, but real) here at build time. `toggle_fullscreen`
    // refreshes this again after every fullscreen↔windowed transition,
    // which is the point this actually matters most.
    let width = u32::try_from(picture.width()).unwrap_or(0);
    let height = u32::try_from(picture.height()).unwrap_or(0);
    pipeline.set_window_size(width, height);
    pipeline.start()?;
    Ok(pipeline)
}

/// The protocol/background thread's body: mirrors
/// `usb_auth_discovery_probe`'s existing device-discovery/AOA-transition/
/// transport-open sequence (`main.rs`) exactly, differing only in the
/// final `VideoRenderTarget` passed to `auth_discovery_probe::run`.
fn run_session(
    selector: &str,
    tls12_compatibility: bool,
    handoff: Gtk4WindowHandoff,
    touch_settings: TouchSettingsHandoff,
    cancel: &CancellationFlag,
) -> Result<(), CliError> {
    connection_state::report(ConnectionState::Ready);
    let paths = credential_store::CredentialPaths::from(
        credential_store::load_config(Path::new("/etc/aa-headunit/config.toml"))
            .map_err(|error| CliError::Credentials(error.to_string()))?,
    );
    let credentials = credential_store::load_credentials(&paths, true)
        .map_err(|error| CliError::Credentials(error.to_string()))?;

    let (bus, address) = transport_usb::parse_bus_address(selector).map_err(CliError::Aoa)?;
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let candidate = backend
        .list_devices()
        .map_err(CliError::Aoa)?
        .into_iter()
        .find(|device| device.bus == bus && device.address == address)
        .ok_or(CliError::Aoa(transport_api::AoaError::Unplugged))?;

    println!("probe_authorization=operator_confirmed");
    println!("probe_payload_logging=disabled");
    println!("probe_state=preparing_accessory_transport");
    connection_state::report(ConnectionState::Connecting);
    let mut aoa = AoaMachine::new(backend, AOA_TRANSITION_TIMEOUT);
    let outcome = aoa
        .run(candidate, &AoaIdentification::receiver_probe())
        .map_err(CliError::Aoa)?;
    let backend = transport_usb::LibUsbAoaBackend::new().map_err(CliError::Aoa)?;
    let mut transport = backend
        .open_claimed_session_transport(&outcome.transport.device)
        .map_err(CliError::Aoa)?;
    crate::auth_discovery_probe::run(
        &mut transport,
        tls12_compatibility,
        credentials.material,
        VideoRenderTarget::Gtk4Window(handoff, touch_settings),
        cancel,
    )
}
