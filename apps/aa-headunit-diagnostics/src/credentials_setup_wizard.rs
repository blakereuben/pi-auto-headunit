//! `aa-headunit-diagnostics credentials setup` — a guided, Windows-installer-
//! style GTK4 wizard around the exact same [`credential_store`] functions
//! `credentials.rs`'s CLI `check`/`install` commands already use
//! (`validate_credentials`, `install_credentials`), for an operator who
//! isn't comfortable with a terminal. Built at the operator's explicit
//! request, 2026-08-23: "im used to windows so installing a exe will take
//! you thrugh the process" — welcome page, a file picker for the
//! certificate and private key, a validation result, then an install
//! result, each with Back/Next-style navigation via a plain [`Stack`]
//! (this project doesn't otherwise depend on libadwaita, so no
//! `AdwNavigationView`).
//!
//! Deliberately does not offer to replace an already-installed pair —
//! `install_credentials` itself refuses to overwrite
//! (`CredentialError::AlreadyInstalled`), by design
//! (`docs/development/credential-provisioning.md`: "Credential rotation
//! will be a separate explicit operation so an interrupted setup cannot
//! silently replace a working identity"). The wizard surfaces that same
//! refusal up front instead of working around it.
//!
//! Never reads a credential file's contents into anything this module
//! prints or logs beyond what [`credential_store::CredentialStatus`]
//! already exposes (validity dates, key file mode) — the same bounded
//! summary the CLI's own `print_status` shows.

use credential_store::{
    CredentialError, CredentialPaths, install_credentials, validate_credentials,
};
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, Label, ListBox, ListBoxRow, Orientation,
    ScrolledWindow, SelectionMode, Stack, glib,
};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use transport_bluetooth::PairingProgress;

use crate::credentials::{DEFAULT_CERTIFICATE_PATH, DEFAULT_PRIVATE_KEY_PATH};

const WINDOW_WIDTH: i32 = 560;
const WINDOW_HEIGHT: i32 = 420;
const PAGE_MARGIN: i32 = 24;
const ROW_SPACING: i32 = 12;

/// The exit code this process reports when the operator hit Cancel
/// (`build_cancel_row`) — arbitrary but distinct from `0`/`1`/`2`, the
/// exit codes a plain window-close or an ordinary error might already
/// produce. `packaging/setup.sh` checks for exactly this value to know
/// its own subsequent `apt install` step must be skipped, not just any
/// nonzero wizard exit — see `build_cancel_row`'s own comment for the
/// real-hardware finding that made this necessary.
const CANCELLED_EXIT_CODE: i32 = 42;

/// How long the pairing page waits for a phone before giving up and
/// letting the operator continue anyway (they can always pair later from
/// Bluetooth settings) — matches `packaging/setup.sh`'s own shell
/// implementation of the same wait.
const BLUETOOTH_PAIRING_TIMEOUT: Duration = Duration::from_secs(180);
/// How often the background pairing thread checks whether a phone has
/// paired yet.
const BLUETOOTH_PAIRING_CHECK_INTERVAL: Duration = Duration::from_secs(2);
/// How often the GTK page itself polls for a progress update from that
/// background thread — matches this app's other GTK/background-thread
/// bridges (`gtk_dev_ui.rs`'s own `POLL_INTERVAL`).
const BLUETOOTH_STATUS_POLL_INTERVAL: Duration = Duration::from_millis(200);

/// Where the wizard installs to — identical to `credentials.rs`'s own
/// `install` command destination, not independently chosen.
fn destination_paths() -> CredentialPaths {
    CredentialPaths {
        certificate: PathBuf::from(DEFAULT_CERTIFICATE_PATH),
        private_key: PathBuf::from(DEFAULT_PRIVATE_KEY_PATH),
    }
}

/// Per-attempt state shared across the file-picker/validate/install pages'
/// closures. `Rc<RefCell<..>>` throughout, matching this project's
/// existing GTK4 shared-state convention (`gtk_dev_ui.rs`).
#[derive(Default)]
struct WizardState {
    certificate: Option<PathBuf>,
    private_key: Option<PathBuf>,
}

// Every error this wizard can hit (a missing/invalid credential file, an
// already-installed pair) is shown on its own page instead of propagated —
// the whole point of a guided setup flow is that failure is something the
// operator sees and can act on inline, not a process exit code. The
// `Result` return type exists only to match `credentials_command`'s other
// arms' uniform signature.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn run() -> Result<(), crate::CliError> {
    let application = Application::builder()
        .application_id("dev.pi-auto-headunit.credentials-setup")
        .build();

    application.connect_activate(|application| {
        build_and_present_wizard(application);
    });

    let _exit_code = application.run_with_args::<&str>(&[]);
    Ok(())
}

fn build_and_present_wizard(application: &Application) {
    install_minimum_touch_target_css();

    let window = ApplicationWindow::builder()
        .application(application)
        .title("Android Auto Head Unit Setup")
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .resizable(true)
        .build();

    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);
    stack.set_vexpand(true);

    // A persistent row *outside* the stack, not duplicated into every
    // individual page's own button row — stays visible across every page
    // (including ones added to `stack` dynamically later, like the
    // validate/install result pages) without needing separate wiring in
    // each `build_*_page` function. Operator's explicit direction,
    // 2026-08-26: "a cancel installation button in the wizard on every
    // page so i can cancel at anytime, which will also act as a
    // uninstall."
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.append(&stack);
    root.append(&build_cancel_row());
    window.set_child(Some(&root));

    let state: Rc<RefCell<WizardState>> = Rc::new(RefCell::new(WizardState::default()));

    // Checked once, at launch, before showing anything else — an
    // already-installed pair means there's nothing this wizard can
    // usefully walk the operator through (see the module doc comment for
    // why it doesn't offer to replace one).
    if validate_credentials(&destination_paths(), true).is_ok() {
        stack.add_named(
            &build_already_installed_page(&window),
            Some("already-installed"),
        );
        stack.set_visible_child_name("already-installed");
    } else {
        stack.add_named(&build_welcome_page(&stack), Some("welcome"));
        stack.add_named(&build_pair_bluetooth_page(&stack), Some("pair-bluetooth"));
        stack.add_named(
            &build_choose_files_page(&stack, &state),
            Some("choose-files"),
        );
        stack.set_visible_child_name("welcome");
    }

    window.present();
}

/// The persistent "Cancel Installation" row shown under every page. A
/// separate, fixed row (not part of any individual page's own
/// `button_row`) — see [`build_and_present_wizard`]'s own comment for
/// why.
///
/// Real-hardware finding, 2026-08-26: `cancel_and_uninstall` (the
/// `pkexec`/`apt purge` call) genuinely worked the first time this was
/// tried (confirmed via `journalctl`: polkit authenticated and `apt
/// purge` ran successfully) — but running it synchronously right here in
/// the click handler blocked the GTK main thread for as long as the
/// operator took to answer the polkit password prompt, with the window
/// giving no sign anything was happening. Indistinguishable from
/// actually being broken. Same background-thread-plus-channel-plus-
/// `glib::timeout_add_local` bridge as [`build_pair_bluetooth_page`],
/// so the window stays responsive and visibly says what it's doing
/// instead of silently freezing.
fn build_cancel_row() -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    row.set_halign(gtk4::Align::Start);
    row.set_margin_start(PAGE_MARGIN);
    row.set_margin_end(PAGE_MARGIN);
    row.set_margin_bottom(PAGE_MARGIN);

    let cancel = Button::with_label("Cancel Installation");
    cancel.add_css_class("destructive-action");
    row.append(&cancel);

    let status = body_label("");
    row.append(&status);

    cancel.connect_clicked(move |button| {
        button.set_sensitive(false);
        status.set_label("Cancelling and uninstalling — check for a password prompt…");

        let (sender, receiver) = mpsc::channel::<()>();
        thread::spawn(move || {
            cancel_and_uninstall();
            let _ = sender.send(());
        });

        glib::timeout_add_local(BLUETOOTH_STATUS_POLL_INTERVAL, move || {
            match receiver.try_recv() {
                // Real-hardware finding, 2026-08-26: `packaging/setup.sh`'s
                // `"$wizard" credentials setup || true` swallows *any*
                // wizard exit code, so it unconditionally continued on to
                // `apt install --reinstall` right after — reinstalling
                // the very package Cancel had just purged, even though
                // the purge itself genuinely succeeded (confirmed via
                // `journalctl`). `window.close()` alone can't signal
                // "cancelled" to the shell script that launched this
                // process at all — only the process's own exit code can.
                // `std::process::exit` (not a normal `window.close()` and
                // GTK app-loop return) so this exact code reaches
                // `setup.sh` unconditionally, skipping GTK's own cleanup
                // since the process is ending regardless. `setup.sh`
                // checks for exactly this value to skip its own install
                // step instead of treating it like any other nonzero
                // wizard exit (e.g. the window just being closed without
                // finishing, which should still fall through to install
                // in case credentials were already staged).
                Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
                    std::process::exit(CANCELLED_EXIT_CODE);
                }
                Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            }
        });
    });

    row
}

/// Operator's explicit direction, 2026-08-26: Cancel means "undo
/// everything" — whatever this run of the wizard has staged, and (if
/// this is a later re-run of `credentials setup` against an
/// already-installed package, not the first-run `packaging/setup.sh`
/// flow) the installed package itself. `pkexec` (not `sudo`, which has
/// no terminal to prompt a password on from a GTK button): the exact
/// same mechanism the packaged "Uninstall Android Auto Head Unit" icon
/// uses (`packaging/labwc/aa-headunit-uninstall.desktop`), so Cancel
/// here and that icon always agree on how the app actually gets
/// removed. Best-effort throughout: if the package was never installed
/// yet, `apt purge` on it is a harmless no-op, not an error worth
/// surfacing — the operator is leaving either way.
fn cancel_and_uninstall() {
    if let Some(staging) = crate::credentials::staging_paths() {
        let _ = std::fs::remove_file(&staging.certificate);
        let _ = std::fs::remove_file(&staging.private_key);
    }
    let _ = std::process::Command::new("pkexec")
        .args(["apt", "purge", "-y", "aa-headunit-diagnostics"])
        .status();
}

fn page_container() -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, ROW_SPACING);
    page.set_margin_top(PAGE_MARGIN);
    page.set_margin_bottom(PAGE_MARGIN);
    page.set_margin_start(PAGE_MARGIN);
    page.set_margin_end(PAGE_MARGIN);
    page.set_valign(gtk4::Align::Center);
    page
}

fn title_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.add_css_class("title-2");
    label.set_wrap(true);
    label.set_xalign(0.0);
    label
}

fn body_label(text: &str) -> Label {
    let label = Label::new(Some(text));
    label.set_wrap(true);
    label.set_xalign(0.0);
    label
}

/// A right-aligned row of buttons, the way a Windows installer's
/// Back/Next/Cancel row reads left-to-right in visual position but is
/// built right-to-left in code (`set_halign(End)` on the row, buttons
/// appended in on-screen left-to-right order).
fn button_row(buttons: &[&Button]) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    row.set_halign(gtk4::Align::End);
    row.set_margin_top(ROW_SPACING);
    for button in buttons {
        row.append(*button);
    }
    row
}

fn build_welcome_page(stack: &Stack) -> GtkBox {
    let page = page_container();
    page.append(&title_label("Android Auto Head Unit Setup"));
    page.append(&body_label(
        "This installs the certificate and private key your Android Auto \
         receiver identity uses, so this head unit can connect to your \
         phone. You'll need a certificate file and a matching private key \
         file you already have on hand — this project doesn't provide, \
         generate, or download these for you.",
    ));

    let next = Button::with_label("Get Started");
    next.add_css_class("suggested-action");
    let stack_for_next = stack.clone();
    next.connect_clicked(move |_| {
        stack_for_next.set_visible_child_name("pair-bluetooth");
    });
    page.append(&button_row(&[&next]));
    page
}

/// Pairing your phone before picking credential files, not after —
/// operator's explicit direction, 2026-08-26: turn Bluetooth on and make
/// it discoverable itself if it isn't already, as part of this same
/// guided flow, so Android Auto can find an already-paired phone and
/// start right up on first launch instead of the operator hitting a
/// black screen with nothing paired. Never blocks progress on actually
/// pairing — "Continue" is enabled from the start, matching
/// `packaging/setup.sh`'s own "setup continues either way" shell version
/// of the same step.
fn build_pair_bluetooth_page(stack: &Stack) -> GtkBox {
    let page = page_container();
    page.append(&title_label("Connect your phone over Bluetooth"));
    page.append(&body_label(
        "Pairing now means Android Auto can start right up once setup \
         finishes, instead of the first launch having nothing paired to \
         talk to. You can also pair later from Bluetooth settings.",
    ));

    let status = body_label("Turning on Bluetooth…");
    page.append(&status);

    let next = Button::with_label("Continue");
    next.add_css_class("suggested-action");
    let stack_for_next = stack.clone();
    next.connect_clicked(move |_| {
        stack_for_next.set_visible_child_name("choose-files");
    });
    page.append(&button_row(&[&next]));

    let (sender, receiver) = mpsc::channel::<PairingProgress>();
    thread::spawn(move || {
        transport_bluetooth::pair_phone_with_progress(
            BLUETOOTH_PAIRING_TIMEOUT,
            BLUETOOTH_PAIRING_CHECK_INTERVAL,
            &sender,
        );
    });

    let status_for_poll = status.clone();
    glib::timeout_add_local(BLUETOOTH_STATUS_POLL_INTERVAL, move || {
        match receiver.try_recv() {
            Ok(PairingProgress::Discoverable { device_name }) => {
                status_for_poll.set_label(&format!(
                    "On your phone: open Bluetooth settings and pair with \
                     \"{device_name}\". Waiting…"
                ));
                glib::ControlFlow::Continue
            }
            Ok(PairingProgress::Paired) => {
                status_for_poll.set_label("Phone paired.");
                glib::ControlFlow::Break
            }
            Ok(PairingProgress::TimedOut) => {
                status_for_poll
                    .set_label("No phone paired yet — you can pair later from Bluetooth settings.");
                glib::ControlFlow::Break
            }
            Ok(PairingProgress::Error(error)) => {
                status_for_poll.set_label(&format!("Couldn't start Bluetooth pairing: {error}."));
                glib::ControlFlow::Break
            }
            Err(mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    page
}

fn build_already_installed_page(window: &ApplicationWindow) -> GtkBox {
    let page = page_container();
    page.append(&title_label("Credentials already installed"));

    let detail = match validate_credentials(&destination_paths(), true) {
        Ok(status) => format!(
            "A certificate and private key are already installed at \
             {}, valid from {} to {}.\n\nTo replace them, remove those two \
             files yourself first, then run setup again — this is a \
             deliberate safeguard so setup can never silently overwrite a \
             working identity.",
            destination_paths().certificate.parent().map_or_else(
                || destination_paths().certificate.display().to_string(),
                |parent| parent.display().to_string()
            ),
            status.summary.not_before,
            status.summary.not_after,
        ),
        // Unreachable in practice (this page is only shown after a
        // successful validate_credentials call), but a WizardState-free
        // page still needs a body for the type checker's sake, and
        // silently mismatching the check that gated it here would be a
        // worse bug than an honest fallback message.
        Err(error) => format!("Credentials are already installed. ({error})"),
    };
    page.append(&body_label(&detail));

    let close = Button::with_label("Close");
    let window_for_close = window.clone();
    close.connect_clicked(move |_| {
        window_for_close.close();
    });
    page.append(&button_row(&[&close]));
    page
}

fn build_choose_files_page(stack: &Stack, state: &Rc<RefCell<WizardState>>) -> GtkBox {
    let page = page_container();
    page.append(&title_label("Step 1 of 2 — Choose your files"));
    page.append(&body_label(
        "Select the certificate (.crt/.pem) and private key (.key/.pem) \
         files for your Android Auto receiver identity.",
    ));

    let certificate_row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    let certificate_label = Label::new(Some("Certificate: not selected"));
    certificate_label.set_hexpand(true);
    certificate_label.set_xalign(0.0);
    certificate_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    let certificate_browse = Button::with_label("Browse…");
    certificate_row.append(&certificate_label);
    certificate_row.append(&certificate_browse);
    page.append(&certificate_row);

    let private_key_row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    let private_key_label = Label::new(Some("Private key: not selected"));
    private_key_label.set_hexpand(true);
    private_key_label.set_xalign(0.0);
    private_key_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    let private_key_browse = Button::with_label("Browse…");
    private_key_row.append(&private_key_label);
    private_key_row.append(&private_key_browse);
    page.append(&private_key_row);

    let next = Button::with_label("Next");
    next.add_css_class("suggested-action");
    next.set_sensitive(false);

    {
        let stack = stack.clone();
        let state = Rc::clone(state);
        let certificate_label = certificate_label.clone();
        let private_key_label = private_key_label.clone();
        let next = next.clone();
        certificate_browse.connect_clicked(move |_| {
            open_file_browser(
                &stack,
                &state,
                BrowseTarget::Certificate,
                certificate_label.clone(),
                private_key_label.clone(),
                next.clone(),
            );
        });
    }

    {
        let stack = stack.clone();
        let state = Rc::clone(state);
        let certificate_label = certificate_label.clone();
        let private_key_label = private_key_label.clone();
        let next = next.clone();
        private_key_browse.connect_clicked(move |_| {
            open_file_browser(
                &stack,
                &state,
                BrowseTarget::PrivateKey,
                certificate_label.clone(),
                private_key_label.clone(),
                next.clone(),
            );
        });
    }

    {
        let stack = stack.clone();
        let state = Rc::clone(state);
        next.connect_clicked(move |_| {
            let source = {
                let state = state.borrow();
                CredentialPaths {
                    certificate: state.certificate.clone().unwrap_or_default(),
                    private_key: state.private_key.clone().unwrap_or_default(),
                }
            };
            let page_name = "validate-result";
            if stack.child_by_name(page_name).is_some() {
                stack.remove(&stack.child_by_name(page_name).unwrap());
            }
            stack.add_named(
                &build_validate_result_page(&stack, &source),
                Some(page_name),
            );
            stack.set_visible_child_name(page_name);
        });
    }

    page.append(&button_row(&[&next]));
    page
}

/// Which of the two `choose-files` fields a `browse-files` page run is
/// picking for — needed because both Browse buttons share the same
/// picker implementation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BrowseTarget {
    Certificate,
    PrivateKey,
}

/// Real-hardware finding, 2026-08-28: the wizard's Browse buttons
/// originally opened GTK4's native `FileDialog` — its own internal file
/// list is a `GtkListView`/`GtkColumnView`-based widget with known touch-
/// gesture problems (taps not reliably registering as row activation,
/// unlike plain `Button`s, which `install_minimum_touch_target_css`
/// already covers). Confirmed on the actual touchscreen this app targets.
/// This replaces it with an in-wizard page built from `ListBox`/
/// `ListBoxRow` instead — a much simpler widget whose "row-activated"
/// signal fires directly off the same click/touch handling a `Button`
/// uses, with no separate gesture recognizer arbitrating between tap and
/// scroll the way the list-view-based file chooser's did. Built as a
/// `Stack` page in the *same* window rather than a second top-level
/// dialog window for the same reason `FileDialog` was replaced entirely,
/// not just restyled: this sidesteps any possibility of touch input
/// being routed to the wrong top-level surface under labwc, whatever the
/// `FileDialog`'s exact root cause turned out to be.
fn open_file_browser(
    stack: &Stack,
    state: &Rc<RefCell<WizardState>>,
    target: BrowseTarget,
    certificate_label: Label,
    private_key_label: Label,
    next: Button,
) {
    let page_name = "browse-files";
    if let Some(existing) = stack.child_by_name(page_name) {
        stack.remove(&existing);
    }
    stack.add_named(
        &build_file_browser_page(
            stack,
            state,
            target,
            certificate_label,
            private_key_label,
            next,
        ),
        Some(page_name),
    );
    stack.set_visible_child_name(page_name);
}

/// Where the browser starts — an operator's certificate/private key
/// pair usually lives on a USB drive (real-hardware finding: this is
/// how every setup so far has actually been done), so this prefers the
/// first mounted removable volume it finds under `/media/<user>/`
/// (where `gvfs-udisks2-volume-monitor`/udisks2 auto-mount removable
/// media, both installed specifically for this) and only falls back to
/// the operator's own home directory if none is mounted yet.
fn initial_browse_dir() -> PathBuf {
    if let Ok(username) = std::env::var("USER") {
        let media_dir = PathBuf::from("/media").join(username);
        if let Ok(entries) = fs::read_dir(&media_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    return entry.path();
                }
            }
        }
    }
    std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from)
}

fn build_file_browser_page(
    stack: &Stack,
    state: &Rc<RefCell<WizardState>>,
    target: BrowseTarget,
    certificate_label: Label,
    private_key_label: Label,
    next: Button,
) -> GtkBox {
    let page = page_container();
    page.set_valign(gtk4::Align::Fill);
    page.set_vexpand(true);

    page.append(&title_label(match target {
        BrowseTarget::Certificate => "Choose certificate file",
        BrowseTarget::PrivateKey => "Choose private key file",
    }));

    let path_label = body_label("");
    path_label.set_ellipsize(gtk4::pango::EllipsizeMode::Start);
    page.append(&path_label);

    let header_row = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    let up_button = Button::with_label("⬆ Up");
    let home_button = Button::with_label("🏠 Home");
    header_row.append(&up_button);
    header_row.append(&home_button);
    page.append(&header_row);

    let list_box = ListBox::new();
    list_box.set_selection_mode(SelectionMode::None);

    let scroller = ScrolledWindow::new();
    scroller.set_child(Some(&list_box));
    scroller.set_vexpand(true);
    scroller.set_min_content_height(240);
    page.append(&scroller);

    let cancel = Button::with_label("Cancel");
    let stack_for_cancel = stack.clone();
    cancel.connect_clicked(move |_| {
        stack_for_cancel.set_visible_child_name("choose-files");
    });
    page.append(&button_row(&[&cancel]));

    let current_dir: Rc<RefCell<PathBuf>> = Rc::new(RefCell::new(initial_browse_dir()));
    // Kept in the same order rows are appended to `list_box`, so a
    // `ListBoxRow`'s `index()` looks its entry up directly — simpler
    // than attaching data to each row widget individually.
    let row_entries: Rc<RefCell<Vec<(bool, PathBuf)>>> = Rc::new(RefCell::new(Vec::new()));

    {
        let list_box = list_box.clone();
        let path_label = path_label.clone();
        let current_dir = Rc::clone(&current_dir);
        let row_entries = Rc::clone(&row_entries);
        refresh_file_browser_list(&list_box, &path_label, &current_dir, &row_entries);

        let list_box_for_activate = list_box.clone();
        let path_label_for_activate = path_label.clone();
        let current_dir_for_activate = Rc::clone(&current_dir);
        let row_entries_for_activate = Rc::clone(&row_entries);
        let stack = stack.clone();
        let state = Rc::clone(state);
        list_box.connect_row_activated(move |_, row| {
            let index = usize::try_from(row.index()).unwrap_or(usize::MAX);
            let Some((is_dir, path)) = row_entries_for_activate.borrow().get(index).cloned() else {
                return;
            };
            if is_dir {
                *current_dir_for_activate.borrow_mut() = path;
                refresh_file_browser_list(
                    &list_box_for_activate,
                    &path_label_for_activate,
                    &current_dir_for_activate,
                    &row_entries_for_activate,
                );
                return;
            }
            match target {
                BrowseTarget::Certificate => {
                    certificate_label.set_text(&format!("Certificate: {}", path.display()));
                    state.borrow_mut().certificate = Some(path);
                }
                BrowseTarget::PrivateKey => {
                    private_key_label.set_text(&format!("Private key: {}", path.display()));
                    state.borrow_mut().private_key = Some(path);
                }
            }
            let both_chosen = {
                let state = state.borrow();
                state.certificate.is_some() && state.private_key.is_some()
            };
            next.set_sensitive(both_chosen);
            stack.set_visible_child_name("choose-files");
        });
    }

    {
        let list_box = list_box.clone();
        let path_label = path_label.clone();
        let current_dir = Rc::clone(&current_dir);
        let row_entries = Rc::clone(&row_entries);
        up_button.connect_clicked(move |_| {
            let parent = current_dir
                .borrow()
                .parent()
                .map(std::path::Path::to_path_buf);
            if let Some(parent) = parent {
                *current_dir.borrow_mut() = parent;
                refresh_file_browser_list(&list_box, &path_label, &current_dir, &row_entries);
            }
        });
    }

    {
        home_button.connect_clicked(move |_| {
            *current_dir.borrow_mut() =
                std::env::var("HOME").map_or_else(|_| PathBuf::from("/"), PathBuf::from);
            refresh_file_browser_list(&list_box, &path_label, &current_dir, &row_entries);
        });
    }

    page
}

/// Rebuilds `list_box`'s rows from `current_dir`'s contents — directories
/// first, then files, both alphabetical, dotfiles hidden (matching how
/// GTK's own native file chooser behaves by default). Large touch
/// targets throughout: each row's own margins give it well over the
/// 48px minimum `install_minimum_touch_target_css` sets for buttons,
/// without needing a separate CSS rule for a widget type that isn't a
/// `Button`.
fn refresh_file_browser_list(
    list_box: &ListBox,
    path_label: &Label,
    current_dir: &Rc<RefCell<PathBuf>>,
    row_entries: &Rc<RefCell<Vec<(bool, PathBuf)>>>,
) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }
    row_entries.borrow_mut().clear();

    let dir = current_dir.borrow().clone();
    path_label.set_text(&dir.display().to_string());

    let mut entries: Vec<_> = match fs::read_dir(&dir) {
        Ok(read_dir) => read_dir.flatten().collect(),
        Err(_) => Vec::new(),
    };
    entries.sort_by_key(|entry| {
        let is_dir = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        (!is_dir, entry.file_name())
    });

    if entries.is_empty() {
        let row = ListBoxRow::new();
        row.set_activatable(false);
        row.set_child(Some(&build_browser_row_label("(empty or unreadable)")));
        list_box.append(&row);
        return;
    }

    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = entry.file_type().is_ok_and(|file_type| file_type.is_dir());
        let icon = if is_dir { "📁" } else { "📄" };

        let row = ListBoxRow::new();
        row.set_activatable(true);
        row.set_child(Some(&build_browser_row_label(&format!("{icon} {name}"))));
        list_box.append(&row);
        row_entries.borrow_mut().push((is_dir, entry.path()));
    }
}

fn build_browser_row_label(text: &str) -> GtkBox {
    let row_box = GtkBox::new(Orientation::Horizontal, ROW_SPACING);
    row_box.set_margin_top(14);
    row_box.set_margin_bottom(14);
    row_box.set_margin_start(PAGE_MARGIN);
    row_box.set_margin_end(PAGE_MARGIN);
    let label = Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    row_box.append(&label);
    row_box
}

fn build_validate_result_page(stack: &Stack, source: &CredentialPaths) -> GtkBox {
    let page = page_container();

    // `false`: this previews the freshly *picked* source pair, which
    // `install_credentials` (`credential-store/src/linux.rs`) rewrites to
    // the real destination with correct `0600` permissions regardless of
    // the source's own mode — see that function's own comment. Matching
    // its relaxed check here means this preview page doesn't reject a
    // pair the actual install step would accept.
    match validate_credentials(source, false) {
        Ok(status) => {
            page.append(&title_label("Step 2 of 2 — Files look valid"));
            page.append(&body_label(&format!(
                "Certificate valid from {} to {}.",
                status.summary.not_before, status.summary.not_after
            )));

            let back = Button::with_label("Back");
            let stack_for_back = stack.clone();
            back.connect_clicked(move |_| {
                stack_for_back.set_visible_child_name("choose-files");
            });

            let install = Button::with_label("Install");
            install.add_css_class("suggested-action");
            let stack_for_install = stack.clone();
            let source = source.clone();
            install.connect_clicked(move |_| {
                let page_name = "install-result";
                if let Some(existing) = stack_for_install.child_by_name(page_name) {
                    stack_for_install.remove(&existing);
                }
                stack_for_install.add_named(&build_install_result_page(&source), Some(page_name));
                stack_for_install.set_visible_child_name(page_name);
            });

            page.append(&button_row(&[&back, &install]));
        }
        Err(error) => {
            page.append(&title_label(credential_error_title(&error)));
            page.append(&body_label(&describe_credential_error(&error)));

            let back = Button::with_label("Back");
            let stack_for_back = stack.clone();
            back.connect_clicked(move |_| {
                stack_for_back.set_visible_child_name("choose-files");
            });
            page.append(&button_row(&[&back]));
        }
    }

    page
}

fn build_install_result_page(source: &CredentialPaths) -> GtkBox {
    let page = page_container();

    // Always stages rather than installing straight to `destination_paths()`
    // — this wizard can run before the package (and the `aa-headunit`
    // group/directory that owns the real destination) is even installed
    // (`packaging/setup.sh`). `postinst`, or the main app's own startup as
    // a safety net, adopts the staged pair for real afterward
    // (`credentials::adopt_staged_credentials_if_present`).
    let result = match crate::credentials::staging_paths() {
        Some(staging) => install_credentials(source, &staging),
        None => Err(CredentialError::Config(
            "could not determine a home directory to save to".into(),
        )),
    };

    match result {
        Ok(_) => {
            page.append(&title_label("Credentials saved"));
            page.append(&body_label(
                "These will be installed automatically the next time the \
                 head unit app starts.",
            ));
        }
        Err(error) => {
            page.append(&title_label(credential_error_title(&error)));
            page.append(&body_label(&describe_credential_error(&error)));
        }
    }

    let finish = Button::with_label("Finish");
    finish.add_css_class("suggested-action");
    let finish_for_click = finish.clone();
    finish.connect_clicked(move |_| {
        if let Some(window) = finish_for_click
            .root()
            .and_then(|root| root.downcast::<ApplicationWindow>().ok())
        {
            window.close();
        }
    });
    page.append(&button_row(&[&finish]));
    page
}

/// A title specific to *what actually went wrong*, shown above
/// [`describe_credential_error`]'s detail text. Previously every branch of
/// [`build_validate_page`] (and, separately, [`build_install_result_page`])
/// hard-coded "These files aren't a valid pair"/"Installation failed" for
/// any [`CredentialError`] at all — so a pure file-permissions problem (no
/// mismatch between the certificate and key whatsoever) read as if the
/// files didn't pair, misleading an operator who already knows their
/// cert/key really do match. Real-hardware finding, 2026-08-26: a fresh
/// install's picked-from-USB-stick private key hit exactly this — 0644
/// permissions inherited from a FAT-formatted drive, reported as "aren't a
/// valid pair".
fn credential_error_title(error: &CredentialError) -> &'static str {
    match error {
        CredentialError::Missing(_) => "File not found",
        CredentialError::InvalidFile(_) => "That file isn't usable",
        CredentialError::InvalidCredentials(_) => "These files aren't a valid pair",
        CredentialError::InsecurePrivateKeyPermissions(_) => "Private key permissions are too open",
        CredentialError::AlreadyInstalled(_) => "Already installed",
        CredentialError::Io(_) => "Couldn't read that file",
        CredentialError::Config(_) => "Setup problem",
    }
}

/// Plain-language versions of [`CredentialError`] for the wizard's own
/// pages — the CLI's `Display` impl (`credential-store/src/linux.rs`) is
/// already reasonably plain, this just avoids the `Debug`-shaped prefix
/// wording ("configuration:", "invalid credential file:") that reads fine
/// in a terminal but not in a dialog.
fn describe_credential_error(error: &CredentialError) -> String {
    match error {
        CredentialError::Missing(path) => {
            format!("{} doesn't exist or couldn't be read.", path.display())
        }
        CredentialError::InvalidFile(reason) => format!("That file isn't usable: {reason}."),
        CredentialError::InvalidCredentials(reason) => {
            format!("The certificate and private key don't form a valid pair: {reason}.")
        }
        CredentialError::InsecurePrivateKeyPermissions(mode) => format!(
            "The private key file's permissions ({mode:04o}) allow other users to read it — \
             it must be exactly 0600 (readable only by you). Fix this before continuing: \
             open a terminal and run `chmod 600` on the private key file, then try again. \
             If that file is on a USB drive or SD card reader formatted FAT32/exFAT, `chmod` \
             won't take effect there — those filesystems don't support Unix permissions at \
             all — copy the file onto this device first (e.g. into your home folder), `chmod \
             600` the copy, and pick that copy instead."
        ),
        CredentialError::AlreadyInstalled(path) => {
            format!("{} already exists.", path.display())
        }
        CredentialError::Io(io_error) => format!("Couldn't read that file: {io_error}."),
        CredentialError::Config(reason) => format!("Setup configuration problem: {reason}."),
    }
}

fn install_minimum_touch_target_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_data("button { min-height: 48px; min-width: 48px; }");
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
