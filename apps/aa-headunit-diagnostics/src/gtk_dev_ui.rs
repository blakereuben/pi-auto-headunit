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
//! (`gesture_settings::GestureSettings`, persisted to
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
    Application, ApplicationWindow, Box as GtkBox, Button, DropDown, Label, Orientation, Overlay,
    Picture, SpinButton, glib,
};
use media_api::DecoderCapability;
use media_gstreamer::{GstreamerBackend, GstreamerError, RenderSink, VideoRenderPipeline};
use platform_api::{GestureEvent, GestureId, SharedArmWindow};
use platform_linux::touch::{Rotation, SharedRotation};
use transport_api::{AoaIdentification, AoaMachine};

use crate::CliError;
use crate::auth_discovery_probe::{Gtk4WindowHandoff, TouchSettingsHandoff, VideoRenderTarget};
use crate::cancellation::{self, CancellationFlag};
use crate::connection_state::{self, ConnectionState};
use crate::gesture_settings::{Action, DEFAULT_SETTINGS_PATH, GestureSettings};

const AOA_TRANSITION_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
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
    let hang_safety_net_seconds = hang_safety_net_seconds()?;
    let (capability_sender, capability_receiver) = mpsc::channel::<DecoderCapability>();
    let (pipeline_sender, pipeline_receiver) = mpsc::channel();
    let (session_result_sender, session_result_receiver) = mpsc::channel::<Result<(), CliError>>();
    let (rotation_sender, rotation_receiver) = mpsc::channel::<Option<SharedRotation>>();
    let (arm_window_sender, arm_window_receiver) = mpsc::channel::<SharedArmWindow>();
    let (gesture_sender, gesture_receiver) = mpsc::channel::<GestureEvent>();

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
        },
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
    gesture_settings: Rc<RefCell<GestureSettings>>,
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
        cancel,
        session_result_sender,
        capability_receiver,
        pipeline_sender,
        session_result_receiver,
        rotation_receiver,
        arm_window_receiver,
        gesture_receiver,
    } = state;

    let gesture_settings: Rc<RefCell<GestureSettings>> = Rc::new(RefCell::new(
        GestureSettings::load(Path::new(DEFAULT_SETTINGS_PATH)),
    ));

    let picture = Picture::new();
    let overlay = Overlay::new();
    overlay.set_child(Some(&picture));
    let settings_panel = build_settings_panel(gesture_settings.borrow().arm_window_seconds());
    overlay.add_overlay(&settings_panel.root);
    let armed_mask = build_armed_mask();
    overlay.add_overlay(&armed_mask.root);
    let window = ApplicationWindow::builder()
        .application(application)
        .title("pi-auto-headunit live session")
        .child(&overlay)
        .build();
    window.fullscreen();
    window.present();

    let rotation_handle: Rc<RefCell<Option<SharedRotation>>> = Rc::new(RefCell::new(None));
    let current_rotation: Rc<Cell<Rotation>> = Rc::new(Cell::new(Rotation::Rotate0));
    let arm_window_handle: Rc<RefCell<Option<SharedArmWindow>>> = Rc::new(RefCell::new(None));
    // The window is created fullscreen above.
    let is_fullscreen: Rc<Cell<bool>> = Rc::new(Cell::new(true));

    wire_settings_panel(
        &settings_panel,
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
#[allow(clippy::too_many_arguments)]
fn wire_gesture_poll(
    gesture_receiver: mpsc::Receiver<GestureEvent>,
    settings_panel: SettingsPanel,
    armed_mask: ArmedMask,
    window: ApplicationWindow,
    rotation_handle: Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: Rc<Cell<Rotation>>,
    is_fullscreen: Rc<Cell<bool>>,
    gesture_settings: Rc<RefCell<GestureSettings>>,
) -> glib::SourceId {
    glib::timeout_add_local(POLL_INTERVAL, move || {
        while let Ok(event) = gesture_receiver.try_recv() {
            match event {
                GestureEvent::Armed => armed_mask.root.set_visible(true),
                GestureEvent::Disarmed => armed_mask.root.set_visible(false),
                GestureEvent::Completed(gesture) => {
                    armed_mask.root.set_visible(false);
                    let action = gesture_settings.borrow().action_for(gesture);
                    dispatch_action(
                        action,
                        &settings_panel,
                        &window,
                        &rotation_handle,
                        &current_rotation,
                        &is_fullscreen,
                    );
                }
            }
        }
        glib::ControlFlow::Continue
    })
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

    let _capability_poll_id = glib::timeout_add_local(POLL_INTERVAL, move || {
        match capability_receiver.try_recv() {
            Ok(capability) => {
                let outcome = build_gtk4_pipeline(&capability, &picture);
                let _ = pipeline_sender.send(outcome);
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    let rotation_handle_for_poll = Rc::clone(&rotation_handle);
    let _rotation_poll_id =
        glib::timeout_add_local(POLL_INTERVAL, move || match rotation_receiver.try_recv() {
            Ok(handle) => {
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

    let _timeout_id = glib::timeout_add_seconds_local(hang_safety_net_seconds, move || {
        application.quit();
        glib::ControlFlow::Break
    });
}

/// The settings panel's widgets, kept together so `run()`'s closures can
/// show/hide it and read its controls without threading five separate
/// widget handles around. `Clone` is cheap (GTK widgets are themselves
/// reference-counted handles).
#[derive(Clone)]
struct SettingsPanel {
    root: GtkBox,
    rotation_label: Label,
    gesture_dropdowns: Rc<Vec<(GestureId, DropDown)>>,
    close_button: Button,
    toggle_fullscreen_button: Button,
    cycle_rotation_button: Button,
    arm_timeout_spin: SpinButton,
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

/// The dropdown's selected index is just this array's index — kept as one
/// shared source of truth (`Action::all()`) instead of a separately
/// maintained label list plus a hand-written index<->`Action` mapping.
fn action_dropdown_index(action: Action) -> u32 {
    u32::try_from(
        Action::all()
            .iter()
            .position(|candidate| *candidate == action)
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

fn action_from_dropdown_index(index: u32) -> Action {
    usize::try_from(index)
        .ok()
        .and_then(|index| Action::all().get(index).copied())
        .unwrap_or(Action::OpenSettings)
}

/// Builds the settings panel once, hidden — shown only when the settings
/// gesture's mapped action is [`Action::OpenSettings`]. A plain
/// semi-opaque `GtkBox` centered over the video via `Overlay`, not a
/// separate window: this is a dev diagnostic, not final product chrome.
fn build_settings_panel(initial_arm_window_seconds: u32) -> SettingsPanel {
    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_halign(gtk4::Align::Center);
    root.set_valign(gtk4::Align::Center);
    root.set_visible(false);
    root.add_css_class("background");
    root.set_margin_top(24);
    root.set_margin_bottom(24);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let title = Label::new(Some("Head unit settings"));
    root.append(&title);

    let rotation_label = Label::new(Some("Touch rotation: 0°"));
    root.append(&rotation_label);

    let cycle_rotation_button = Button::with_label("Cycle rotation");
    root.append(&cycle_rotation_button);

    let timeout_row = GtkBox::new(Orientation::Horizontal, 8);
    let timeout_label = Label::new(Some("Gesture timeout (seconds)"));
    let arm_timeout_spin = SpinButton::with_range(
        f64::from(crate::gesture_settings::MIN_ARM_WINDOW_SECONDS),
        f64::from(crate::gesture_settings::MAX_ARM_WINDOW_SECONDS),
        1.0,
    );
    arm_timeout_spin.set_value(f64::from(initial_arm_window_seconds));
    timeout_row.append(&timeout_label);
    timeout_row.append(&arm_timeout_spin);
    root.append(&timeout_row);

    let mappings_title = Label::new(Some("Gesture assignments"));
    root.append(&mappings_title);

    let action_labels: Vec<&str> = Action::all().iter().map(|action| action.label()).collect();
    let mut gesture_dropdowns = Vec::new();
    for gesture in GestureId::all() {
        let row = GtkBox::new(Orientation::Horizontal, 8);
        let label = Label::new(Some(crate::gesture_settings::gesture_label(gesture)));
        let dropdown = DropDown::from_strings(&action_labels);
        row.append(&label);
        row.append(&dropdown);
        root.append(&row);
        gesture_dropdowns.push((gesture, dropdown));
    }

    let close_button = Button::with_label("Close");
    root.append(&close_button);

    let toggle_fullscreen_button = Button::with_label("Return to desktop");
    root.append(&toggle_fullscreen_button);

    SettingsPanel {
        root,
        rotation_label,
        gesture_dropdowns: Rc::new(gesture_dropdowns),
        close_button,
        toggle_fullscreen_button,
        cycle_rotation_button,
        arm_timeout_spin,
    }
}

fn rotation_label_text(rotation: Rotation) -> &'static str {
    match rotation {
        Rotation::Rotate0 => "Touch rotation: 0°",
        Rotation::Rotate90 => "Touch rotation: 90°",
        Rotation::Rotate180 => "Touch rotation: 180°",
        Rotation::Rotate270 => "Touch rotation: 270°",
    }
}

fn next_rotation(rotation: Rotation) -> Rotation {
    match rotation {
        Rotation::Rotate0 => Rotation::Rotate90,
        Rotation::Rotate90 => Rotation::Rotate180,
        Rotation::Rotate180 => Rotation::Rotate270,
        Rotation::Rotate270 => Rotation::Rotate0,
    }
}

fn apply_rotation(
    rotation: Rotation,
    settings_panel: &SettingsPanel,
    rotation_handle: &Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: &Rc<Cell<Rotation>>,
) {
    current_rotation.set(rotation);
    settings_panel
        .rotation_label
        .set_text(rotation_label_text(rotation));
    if let Some(handle) = rotation_handle.borrow().as_ref() {
        handle.set(rotation);
    }
}

fn toggle_fullscreen_button_label(is_fullscreen: bool) -> &'static str {
    if is_fullscreen {
        "Return to desktop"
    } else {
        "Return to video"
    }
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
    settings_panel.root.set_visible(false);
}

/// Connects every control's click/selection handler once, right after the
/// panel is built. Reassigning a gesture's action saves the whole
/// `GestureSettings` immediately — small, infrequent writes, not worth
/// debouncing.
#[allow(clippy::too_many_arguments)]
fn wire_settings_panel(
    settings_panel: &SettingsPanel,
    window: &ApplicationWindow,
    rotation_handle: &Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: &Rc<Cell<Rotation>>,
    arm_window_handle: &Rc<RefCell<Option<SharedArmWindow>>>,
    is_fullscreen: &Rc<Cell<bool>>,
    gesture_settings: &Rc<RefCell<GestureSettings>>,
) {
    for (gesture, dropdown) in settings_panel.gesture_dropdowns.iter() {
        let initial = gesture_settings.borrow().action_for(*gesture);
        dropdown.set_selected(action_dropdown_index(initial));
        let gesture = *gesture;
        let gesture_settings = Rc::clone(gesture_settings);
        dropdown.connect_selected_notify(move |dropdown| {
            let action = action_from_dropdown_index(dropdown.selected());
            gesture_settings.borrow_mut().set_action(gesture, action);
            let _ = gesture_settings
                .borrow()
                .save(Path::new(DEFAULT_SETTINGS_PATH));
        });
    }

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

    let settings_panel_for_cycle = settings_panel.clone();
    let rotation_handle_for_cycle = Rc::clone(rotation_handle);
    let current_rotation_for_cycle = Rc::clone(current_rotation);
    settings_panel
        .cycle_rotation_button
        .connect_clicked(move |_| {
            let next = next_rotation(current_rotation_for_cycle.get());
            apply_rotation(
                next,
                &settings_panel_for_cycle,
                &rotation_handle_for_cycle,
                &current_rotation_for_cycle,
            );
        });

    let settings_panel_for_close = settings_panel.clone();
    settings_panel.close_button.connect_clicked(move |_| {
        settings_panel_for_close.root.set_visible(false);
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
    window: &ApplicationWindow,
    rotation_handle: &Rc<RefCell<Option<SharedRotation>>>,
    current_rotation: &Rc<Cell<Rotation>>,
    is_fullscreen: &Rc<Cell<bool>>,
) {
    match action {
        Action::OpenSettings => settings_panel.root.set_visible(true),
        Action::ToggleFullscreen => toggle_fullscreen(window, settings_panel, is_fullscreen),
        Action::CycleRotation => {
            let next = next_rotation(current_rotation.get());
            apply_rotation(next, settings_panel, rotation_handle, current_rotation);
        }
        // Sends a key event to the phone rather than touching local
        // window/rotation state, so it's dispatched directly from the
        // background protocol thread (the one with transport access) in
        // `auth_discovery_probe.rs::service_touch_input`, not here.
        Action::SwitchToMedia
        | Action::SwitchToNavigation
        | Action::SwitchToRadio
        | Action::SwitchToPhone => {}
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
